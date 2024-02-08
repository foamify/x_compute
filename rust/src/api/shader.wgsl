struct Rect {
    min: vec2<f32>,
    max: vec2<f32>,
};

@group(0) @binding(0) var<storage, read> inputPoints: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> inputRect: Rect;
@group(0) @binding(2) var<storage, read_write> outputPoints: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read_write> emptyPoints: array<i32>;


fn isPointInsideRect(point: vec2<f32>, rect: Rect) -> bool {
    return point.x >= rect[0].x && point.y >= rect[0].y &&
           point.x <= rect[1].x && point.y <= rect[1].y;
}

@compute
@workgroup_size(1)
fn main(@builtin(global_invocation_id) idx: vec3<u32>) {
    if (idx.x >= arrayLength(&inputPoints)) {
        return;
    }
    let point = inputPoints[idx.x];
    if (isPointInsideRect(point, inputRect)) {
        outputPoints[idx.x] = point;
        emptyPoints[idx.x] = -1;
    } else {
        emptyPoints[idx.x] = 1;
    }
}
