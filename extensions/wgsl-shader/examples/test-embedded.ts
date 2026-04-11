// Example of WGSL embedded in TypeScript using /* wgsl */ comment tag

const shaderCode = /* wgsl */ `
    struct VertexOutput {
        @builtin(position) position: vec4f,
        @location(0) color: vec3f,
    };

    @vertex
    fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
        var positions = array<vec2f, 3>(
            vec2f(0.0, 0.5),
            vec2f(-0.5, -0.5),
            vec2f(0.5, -0.5),
        );

        var colors = array<vec3f, 3>(
            vec3f(1.0, 0.0, 0.0),
            vec3f(0.0, 1.0, 0.0),
            vec3f(0.0, 0.0, 1.0),
        );

        var output: VertexOutput;
        output.position = vec4f(positions[vertex_index], 0.0, 1.0);
        output.color = colors[vertex_index];
        return output;
    }

    @fragment
    fn fs_main(input: VertexOutput) -> @location(0) vec4f {
        return vec4f(input.color, 1.0);
    }
`;

export { shaderCode };
