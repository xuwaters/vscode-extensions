#version 450

layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec2 v_uv;

// Vulkan GLSL keeps textures and samplers apart as separate objects, joined
// at the call site with sampler2D(texture, sampler); see test-opengl.frag.
layout(set = 0, binding = 1) uniform texture2D albedo;
layout(set = 0, binding = 2) uniform sampler albedo_sampler;

layout(location = 0) out vec4 out_color;

const vec3 LIGHT_DIRECTION = vec3(0.0, 1.0, 0.0);

float lambert(vec3 normal, vec3 light) {
    return max(dot(normalize(normal), normalize(light)), 0.0);
}

void main() {
    vec4 base = texture(sampler2D(albedo, albedo_sampler), v_uv);
    float diffuse = lambert(v_normal, LIGHT_DIRECTION);
    out_color = vec4(base.rgb * (0.2 + 0.8 * diffuse), base.a);
}
