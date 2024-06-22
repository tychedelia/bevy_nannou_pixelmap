#import bevy_render::{
    view::View,
    globals::Globals,
}

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<uniform> material: LedMaterial;
@group(1) @binding(1) var<storage, read> average_colors: array<vec4<f32>>;


struct LedMaterial {
    offset: u32,
    count: u32,
    position: vec2<f32>,
    size: vec2<f32>,
}


struct Vertex {
    @builtin(vertex_index) index: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    // Define the positions and UVs for the quad
    var positions = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );

    var uvs = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );

    // Extract the position and UV for the current vertex
    let p = positions[vertex.index];

    // Compute the final position in screen space
    var screen_position = material.position + p * material.size;

    // Convert screen position to normalized device coordinates (NDC)
    // Screen space coordinates need to be in range [-1, 1] for NDC
    var ndc_position = vec2<f32>(
        (screen_position.x / view.viewport.z) * 2.0 - 1.0,
        1.0 - (screen_position.y / view.viewport.w) * 2.0 // Flip the y-coordinate
    );

    // Convert NDC to clip space
    let clip_space_position = vec4<f32>(ndc_position, 0.0, 1.0);

    // Prepare the output
    var out: VertexOutput;
    out.position = clip_space_position;
    out.uv = uvs[vertex.index];
    return out;
}

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    let uv = mesh.uv;
    // Use the led count divided by the uv to determine the color
    let led_index = u32((uv.x * f32(material.count)));
    let color = average_colors[material.offset + led_index];
    return vec4(color.xyz, 1.0);
}
