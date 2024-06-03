struct LedData {
    screen_resolution: vec2<f32>; // Screen resolution
    num_leds: u32;                // Number of LEDs (or segments)
    num_samples: u32;             // Number of samples per segment
    total_area_size: vec2<f32>;    // Size of the total area to sample
    area_position: vec2<f32>;     // Top-left position of the total area
};

@group(0) @binding(0) var textureSampler: sampler;
@group(0) @binding(1) var inputTexture: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> averageColorBuffer: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> ledData: LedData;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let ledIndex: u32 = global_id.x;

    if (ledIndex >= ledData.num_leds) {
        return;
    }

    // Calculate the size and position of each LED segment
    let segmentWidth = ledData.total_area_size.x / f32(ledData.num_leds);
    let halfSegmentWidth = segmentWidth / 2.0;
    let segmentHeight = ledData.total_area_size.y;

    let segmentCenterX = ledData.area_position.x + (f32(ledIndex) + 0.5) * segmentWidth;
    let halfSize = vec2<f32>(halfSegmentWidth, segmentHeight / 2.0);
    let startPos = vec2<f32>(segmentCenterX - halfSegmentWidth, ledData.area_position.y);
    let endPos = vec2<f32>(segmentCenterX + halfSegmentWidth, ledData.area_position.y + segmentHeight);

    var colorSum: vec4<f32> = vec4<f32>(0.0);
    var sampleCount: u32 = 0;

    // Adjust the step size based on the number of samples
    let stepX = ledData.total_area_size.x / f32(ledData.num_samples * ledData.num_leds);
    let stepY = ledData.total_area_size.y / f32(ledData.num_samples);

    for (var x = startPos.x; x < endPos.x; x += stepX) {
        for (var y = startPos.y; y < endPos.y; y += stepY) {
            let samplePos = vec2<f32>(x, y) / ledData.screen_resolution;
            colorSum += textureSample(inputTexture, textureSampler, samplePos);
            sampleCount += 1;
        }
    }

    let avgColor = colorSum / f32(sampleCount);

    // Write the average color to the storage buffer at the index corresponding to the LED segment
    averageColorBuffer[ledIndex] = avgColor;
}