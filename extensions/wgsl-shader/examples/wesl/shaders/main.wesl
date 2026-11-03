// Ctrl-click an imported name to land on its declaration in another file;
// hover it for that declaration's signature and doc comment.

import package::lighting::Light;
import package::lights::point;

@if(high_quality)
const SAMPLES: u32 = 16u;
@else
const SAMPLES: u32 = 4u;

@group(0) @binding(0) var<uniform> light: Light;

struct VertexOutput {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    let lit = point::shade(light, input.world, input.normal);
    return vec4f(lit, f32(SAMPLES) / 16.0);
}
