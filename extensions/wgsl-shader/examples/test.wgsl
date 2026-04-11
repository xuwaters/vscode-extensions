// Example WGSL shader

struct VertexInput {
    @location(0) position: vec3f,
    @location(1) color: vec3f,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4f,
    @location(0) color: vec3f,
};

@group(0) @binding(0)
var<uniform> model_matrix: mat4x4f;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = model_matrix * vec4f(input.position, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    return vec4f(input.color, 1.0);
}
