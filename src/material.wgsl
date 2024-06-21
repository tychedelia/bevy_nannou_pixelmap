#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings

@group(2) @binding(0) var<uniform> material: LedMaterial;
@group(2) @binding(1) var<storage, read> average_colors: array<vec4<f32>>;

struct LedMaterial {
    offset: u32,
    count: u32,
}

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    return out;
}

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
//    return average_colors[0];
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
