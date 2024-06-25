#import bevy_render::{
    view::View,
    globals::Globals,
}

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<uniform> material: LedMaterial;
@group(1) @binding(1) var<storage, read> average_colors: array<vec4<f32>>;


struct LedMaterial {
    offset: u32,
    rotation: f32,
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
    // Define the positions and UVs for the quad relative to the top-left corner
    var positions = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0), // top-left corner (anchor point)
        vec2<f32>(0.0, 1.0), // bottom-left corner
        vec2<f32>(1.0, 0.0), // top-right corner
        vec2<f32>(1.0, 1.0)  // bottom-right corner
    );

    var uvs = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );

    let p = positions[vertex.index];

    let cos_theta = cos(material.rotation);
    let sin_theta = -sin(material.rotation);

    // Calculate the local position based on size and maintain original proportions
    var local_position = p * material.size;

    // Rotate the position around the top-left corner (vertex 0)
    let rotated_position = vec2<f32>(
        cos_theta * local_position.x - sin_theta * local_position.y,
        sin_theta * local_position.x + cos_theta * local_position.y
    );

    // Translate to the correct position
    var world_position = rotated_position + material.position;

    // Convert to clip space
    var ndc_position = vec2<f32>(
        (world_position.x - view.viewport.x) / view.viewport.z * 2.0 - 1.0,
        -((world_position.y - view.viewport.y) / view.viewport.w * 2.0 - 1.0) // Flip y-coordinate
    );

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
    let led_range = (uv.x * f32(material.count));
    let led_index = u32(led_range);
    // If we're exactly on the border between indices, draw a white line
    if (led_range - f32(led_index) < 0.03) {
        return vec4(1.0, 1.0, 1.0, 1.0);
    }

    let color = average_colors[material.offset + led_index];
    return vec4(color.xyz, 1.0);
}
