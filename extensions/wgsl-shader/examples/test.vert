#version 450

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;

layout(binding = 0) uniform Camera {
    mat4 view;
    mat4 projection;
} camera;

layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec2 v_uv;

void main() {
    v_normal = normalize(in_normal);
    v_uv = in_uv;
    gl_Position = camera.projection * camera.view * vec4(in_position, 1.0);
}
