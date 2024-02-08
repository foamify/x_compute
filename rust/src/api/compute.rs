use bytemuck::{Pod, Zeroable};
use std::{borrow::Cow, num::NonZeroU64};
use wgpu::util::DeviceExt;

// #[cfg_attr(test, allow(dead_code))]
// async fn run(points: &[Vec2], rect: &ComputeRect) {
//     let points_in_rect = execute_gpu(&points, &rect).await.unwrap();

//     let disp_points: Vec<String> = points_in_rect
//         .iter()
//         .map(|point| format!("{:?}", point))
//         .collect();

//     println!("points: [{}]", disp_points.join(", "));
//     #[cfg(target_arch = "wasm32")]
//     log::info!("points: [{}]", disp_points.join(", "));
// }

#[cfg_attr(test, allow(dead_code))]
async fn execute_gpu(points: &[Vec2], rect: &ComputeRect) -> Option<Vec<Vec2>> {
    // Instantiates instance of WebGPU
    
    let instance = wgpu::Instance::default();

    // `request_adapter` instantiates the general connection to the GPU
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await?;

    // `request_device` instantiates the feature specific connection to the GPU, defining some parameters,
    //  `features` being the available features.
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
            },
            None,
        )
        .await
        .unwrap();

    execute_gpu_inner(&device, &queue, points, rect).await
}

async fn execute_gpu_inner(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    points: &[Vec2],
    rect: &ComputeRect,
) -> Option<Vec<Vec2>> {
    // Load the shader from WGSL
    let cs_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
    });

    // Create the storage buffer for points
    let points_size = std::mem::size_of_val(points) as wgpu::BufferAddress;
    let points_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Points Buffer"),
        contents: bytemuck::cast_slice(points),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });

    // Create the uniform buffer for the rectangle
    let rect_size = std::mem::size_of::<ComputeRect>() as wgpu::BufferAddress;
    let rect_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Rectangle Buffer"),
        contents: bytemuck::bytes_of(rect),
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::UNIFORM
            | wgpu::BufferUsages::COPY_DST,
    });

    // Create the output buffer for points
    let output_points_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Output Points Buffer"),
        size: points_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    // Create the output buffer for empty points
    let empty_points_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Empty Points Buffer"),
        size: points_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    // Instantiates the pipeline.
    let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &cs_module,
        entry_point: "main",
    });

    // Create the bind group layout and bind group
    let bind_group_layout = compute_pipeline.get_bind_group_layout(0);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: points_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &rect_buffer,
                    offset: 0,
                    size: NonZeroU64::new(rect_size),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: output_points_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: empty_points_buffer.as_entire_binding(),
            },
        ],
    });

    // Create the command encoder and begin the compute pass
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        cpass.set_pipeline(&compute_pipeline);
        cpass.set_bind_group(0, &bind_group, &[]);
        cpass.dispatch_workgroups(points.len() as u32, 1, 1); // Adjusted for workgroup size
    }

    // Copy the results back to the CPU
    let staging_buffer_output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Output Points Staging Buffer"),
        size: points_size, // Double the size to account for both output buffers
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let staging_buffer_empty = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Empty Points Staging Buffer"),
        size: points_size, // Double the size to account for both output buffers
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Copy the output_points_buffer to the staging_buffer_output
    encoder.copy_buffer_to_buffer(
        &output_points_buffer,
        0,
        &staging_buffer_output,
        0,
        points_size,
    );

    // Copy the empty_points_buffer to the staging_buffer_empty
    encoder.copy_buffer_to_buffer(
        &empty_points_buffer,
        0,
        &staging_buffer_empty,
        0,
        points_size,
    );

    // Submit the commands
    queue.submit(Some(encoder.finish()));

    // Map the staging buffers and await the results
    let buffer_slice_output = staging_buffer_output.slice(..);
    let buffer_slice_empty = staging_buffer_empty.slice(..);
    let (sender_output, receiver_output) = flume::bounded(1);
    let (sender_empty, receiver_empty) = flume::bounded(1);

    buffer_slice_output.map_async(wgpu::MapMode::Read, move |v| sender_output.send(v).unwrap());
    buffer_slice_empty.map_async(wgpu::MapMode::Read, move |v| sender_empty.send(v).unwrap());

    device.poll(wgpu::Maintain::wait()).panic_on_timeout();

    let result_output: Vec<Vec2>;

    // Receive the results and convert them back to Vec<Vec2>
    if let Ok(Ok(())) = receiver_output.recv_async().await {
        let data_output = buffer_slice_output.get_mapped_range();
        result_output = bytemuck::cast_slice(&data_output).to_vec();
        drop(data_output);
        staging_buffer_output.unmap();
    } else {
        panic!("Failed to read output points from GPU!");
    }

    if let Ok(Ok(())) = receiver_empty.recv_async().await {
        let data_empty = buffer_slice_empty.get_mapped_range();
        let result_empty: Vec<i32> = bytemuck::cast_slice(&data_empty).to_vec();
        drop(data_empty);
        staging_buffer_empty.unmap();
        let mut result: Vec<Vec2> = Vec::new();

        for i in 0..result_empty.len() - 1 {
            if result_empty[i] == -1 {
                result.push(result_output[i]);
            }
        }

        return Some(result);
    } else {
        panic!("Failed to read empty points from GPU!");
    }
}

pub async fn run_collatz(points: Vec<Vec2>, rect: ComputeRect) -> Vec<Vec2> {
    // std::env::set_var("RUST_BACKTRACE", "1");
    // let now = Instant::now();
    // #[cfg(not(target_arch = "wasm32"))]
    // {
    //     pollster::block_on(run(&points, &rect));
    // }
    // #[cfg(target_arch = "wasm32")]
    // {
    //     wasm_bindgen_futures::spawn_local(run(input));
    // }
    // let elapsed = now.elapsed();
    // println!("Elapsed: {:.2?}", elapsed);
    return execute_gpu(&points, &rect).await.unwrap();
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ComputeRect {
    pub min: Vec2,
    pub max: Vec2,
}

pub type Vec2 = [f32; 2];
