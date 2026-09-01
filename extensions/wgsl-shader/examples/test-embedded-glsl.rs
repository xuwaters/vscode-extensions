// Example of GLSL embedded in Rust using the /* glsl */ comment tag.
//
// Requires `"rust-analyzer.semanticHighlighting.strings.enable": false`, for the
// same reason as the WGSL example next door: rust-analyzer's `string` semantic
// token covers the whole literal and overrides the injected TextMate scopes.

// Raw string with hashes — the usual form for a shader.
const VERTEX: &str = /* glsl */ r#"
    #version 450

    layout(location = 0) in vec3 in_position;
    layout(location = 0) out vec3 v_color;

    layout(binding = 0) uniform Camera {
        mat4 view_projection;
    } camera;

    void main() {
        v_color = abs(normalize(in_position));
        gl_Position = camera.view_projection * vec4(in_position, 1.0);
    }
"#;

// Raw string without hashes.
const CLEAR: &str = /* glsl */ r"
    #version 450

    layout(location = 0) out vec4 out_color;

    void main() {
        out_color = vec4(0.0, 0.0, 0.0, 1.0);
    }
";

// Plain string literal, escapes still highlighted as Rust escapes.
const COMPUTE: &str = /* glsl */ "
    #version 450\n
    layout(local_size_x = 64) in;

    layout(std430, binding = 0) buffer Data { float values[]; };

    void main() {
        uint i = gl_GlobalInvocationID.x;
        values[i] = values[i] * 2.0;
    }
";

fn main() {
    println!("{}", VERTEX.len() + CLEAR.len() + COMPUTE.len());
}
