// Example of GLSL embedded in TypeScript using the /* glsl */ comment tag.

const size = 64;

const vertexShader = /* glsl */ `
    #version 450

    layout(location = 0) in vec3 in_position;
    layout(location = 1) in vec2 in_uv;

    layout(location = 0) out vec2 v_uv;

    void main() {
        v_uv = in_uv;
        gl_Position = vec4(in_position, 1.0);
    }
`;

const computeShader = /* glsl */ `
    #version 450

    layout(local_size_x = ${size}) in;

    layout(std430, binding = 0) buffer Data {
        float values[];
    };

    void main() {
        uint i = gl_GlobalInvocationID.x;
        values[i] = clamp(values[i], 0.0, 1.0);
    }
`;

export { vertexShader, computeShader };
