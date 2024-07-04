#import bevy_render::{
    view::View,
}

@group(0) @binding(0) var inputTexture: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> average_colors: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> leds: array<LedData>;
@group(0) @binding(3) var<uniform> view: View;

struct LedData {
    start_index: u32,
    rotation: f32,
    num_leds: u32,
    num_samples: u32,
    total_area_size: vec2<f32>,
    area_position: vec2<f32>,
};

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let bar_index: u32 = global_id.x;

    if (bar_index >= arrayLength(&leds)) {
        return;
    }

    let led_data = leds[bar_index];
    let segment_width = led_data.total_area_size.x / f32(led_data.num_leds);
    let half_segment_width = segment_width / 2.0;
    let segment_height = led_data.total_area_size.y;

    let cos_theta = cos(led_data.rotation);
    let sin_theta = -sin(led_data.rotation);

    for (var led_index: u32 = 0; led_index < led_data.num_leds; led_index++) {
        let segment_center_x = led_data.area_position.x + (f32(led_index) + 0.5) * segment_width;
        let start_pos = vec2<f32>(segment_center_x - half_segment_width, led_data.area_position.y);
        let end_pos = vec2<f32>(segment_center_x + half_segment_width, led_data.area_position.y + segment_height);

        var color_sum: vec4<f32> = vec4<f32>(0.0);
        var sample_count: u32 = 0u;

        let step_x = segment_width / f32(led_data.num_samples);
        let step_y = segment_height / f32(led_data.num_samples);

        for (var x = start_pos.x; x < end_pos.x; x += step_x) {
            for (var y = start_pos.y; y < end_pos.y; y += step_y) {
                let local_x = x - led_data.area_position.x;
                let local_y = y - led_data.area_position.y;

                let rotated_x = cos_theta * local_x - sin_theta * local_y;
                let rotated_y = sin_theta * local_x + cos_theta * local_y;

                let sample_pos = vec2<f32>(rotated_x + led_data.area_position.x, rotated_y + led_data.area_position.y) / view.viewport.zw;
                let texel = textureLoad(inputTexture, vec2<i32>(sample_pos * view.viewport.zw), 0);
                color_sum += texel;
                sample_count += 1u;
            }
        }

        let avg_color = color_sum / f32(sample_count);
        average_colors[led_data.start_index + led_index] = avg_color;
    }
}