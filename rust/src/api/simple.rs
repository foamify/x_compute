use core::panic;
use std::{
    borrow::Cow,
    collections::HashMap,
    io::Cursor,
    num::NonZeroU64,
    sync::{
        mpsc::{self, Receiver, Sender},
        RwLock,
    },
    thread,
};

use bytemuck::{Pod, Zeroable};
use tokio::runtime::Runtime;
use wgpu::util::DeviceExt;

lazy_static::lazy_static! {
    static ref COMPUTES: RwLock<HashMap<String, WgpuContext>> = {
        RwLock::new(HashMap::new())
    };
}

const COMPUTE_KEY: &str = "0";

static INITIALIZED: std::sync::Once = std::sync::Once::new();

#[flutter_rust_bridge::frb(init)]
pub async fn init_app() {
    // Default utilities - feel free to customize
    flutter_rust_bridge::setup_default_user_utils();

    #[cfg(not(target_arch = "wasm32"))]
    {
        INITIALIZED.call_once(|| {
            env_logger::init();
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        INITIALIZED.call_once(|| {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init().expect("could not initialize logger");
        });
    }
    run_compute_thread().await
}

async fn run_compute_thread() {
    let (compute_request_tx, compute_request_rx): (
        Sender<ComputeRequest>,
        Receiver<ComputeRequest>,
    ) = mpsc::channel();
    let (compute_response_tx, compute_response_rx): (
        Sender<ComputeResponse>,
        Receiver<ComputeResponse>,
    ) = mpsc::channel();

    let instance = WgpuCompute::new().await;
    let context = WgpuContext {
        request_tx: compute_request_tx,
        response_rx: compute_response_rx,
    };

    {
        let mut map = COMPUTES.write().unwrap();
        map.insert(COMPUTE_KEY.to_string(), context);
    }

    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        loop {
            if let Ok(request) = compute_request_rx.recv() {
                let response = match &request.command {
                    ComputeCommand::Compute(points, rect) => {
                        rt.block_on(_compute(&instance, &points, &rect))
                    }
                    ComputeCommand::Dispose => ComputeResponse { points: Vec::new() },
                };
                match compute_response_tx.send(response) {
                    Ok(result) => result,
                    Err(e) => panic!("Compute thread lost. {}", e),
                }

                if let ComputeCommand::Dispose = &request.command {
                    break;
                }
            }
        }
    });
}

async fn _compute(instance: &WgpuCompute, points: &[Vec2], rect: &ComputeRect) -> ComputeResponse {
    ComputeResponse {
        points: instance.execute(points, rect).await.unwrap(),
    }
}

struct ComputeRequest {
    context: Option<WgpuContext>,
    command: ComputeCommand,
}

struct ComputeResponse {
    pub points: Vec<Vec2>,
}

#[flutter_rust_bridge::frb(ignore)]
pub struct WgpuContext {
    request_tx: Sender<ComputeRequest>,
    response_rx: Receiver<ComputeResponse>,
}

unsafe impl Send for WgpuContext {}
unsafe impl Sync for WgpuContext {}

enum ComputeCommand {
    Compute(Vec<Vec2>, ComputeRect),
    // Reset,
    Dispose,
}

#[flutter_rust_bridge::frb(ignore)]
pub struct WgpuCompute {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cs_module: wgpu::ShaderModule,
    // pipeline: wgpu::ComputePipeline,
    // bind_group: wgpu::BindGroup,
    // points_buffer: wgpu::Buffer,
    // rect_buffer: wgpu::Buffer,
    // output_buffer: wgpu::Buffer,
    // emptys_buffer: wgpu::Buffer,
}

impl WgpuCompute {
    #[flutter_rust_bridge::frb(ignore)]
    pub async fn new() -> WgpuCompute {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
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

        // Our shader, kindly compiled with Naga.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader.wgsl"
            ))),
        });
        // Load the shader from WGSL
        let cs_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        WgpuCompute {
            device,
            queue,
            cs_module,
        }
    }

    #[flutter_rust_bridge::frb(ignore)]
    pub async fn execute(&self, points: &[Vec2], rect: &ComputeRect) -> Option<Vec<Vec2>> {
        let device = &self.device;
        let queue = &self.queue;
        let cs_module = &self.cs_module;

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

        // Poll the device in a blocking manner so that our future resolves.
        // In an actual application, `device.poll(...)` should
        // be called in an event loop or on another thread.
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
}

// ----------------------------------------------------------------------------------------------------------------------------------------------

pub async fn run_compute(points: Vec<Vec2>, rect: ComputeRect) -> Option<Vec<Vec2>> {
    let map = COMPUTES.read().unwrap();
    if !map.contains_key(COMPUTE_KEY) {
        panic!("Compute instance not found!");
    }

    let context = map.get(COMPUTE_KEY).unwrap();

    match context.request_tx.send(ComputeRequest {
        context: None,
        command: ComputeCommand::Compute(points, rect),
    }) {
        Ok(_) => {
            let response = context.response_rx.recv().unwrap();
            return Some(response.points);
        }
        Err(_) => {
            panic!("Failed to send command to GPU!");
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ComputeRect {
    pub min: Vec2,
    pub max: Vec2,
}

pub type Vec2 = [f32; 2];
