// Example of WGSL embedded in Rust using the /* wgsl */ comment tag.

// Raw string with hashes — the usual form, since WGSL uses no escapes.
const SHADER: &str = /* wgsl */ r#"
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
"#;

// Raw string without hashes.
const CLEAR: &str = /* wgsl */ r"
    @fragment
    fn fs_clear() -> @location(0) vec4f {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
";

// Plain string literal, escapes still highlighted as Rust escapes.
const COMPUTE: &str = /* wgsl */ "
    @group(0) @binding(0) var<storage, read_write> data: array<f32>;

    @compute @workgroup_size(64)
    fn cs_main(@builtin(global_invocation_id) id: vec3u) {
        data[id.x] = data[id.x] * 2.0;
    }
";

fn main() {
    println!("{}", SHADER.len() + CLEAR.len() + COMPUTE.len());
}
