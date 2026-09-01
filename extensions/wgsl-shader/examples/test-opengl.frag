#version 330 core

// Desktop OpenGL, written the way desktop OpenGL is actually written: combined
// `sampler2D` uniforms, a uniform block the driver assigns a binding to, and
// no `set = …, binding = …` layout qualifiers anywhere.
//
// This is the shape that used to be highlighted and never checked, because the
// only validator was naga's Vulkan-only front end. It is checked now, by the
// same analyzer that checks the Vulkan and ES files next door.

uniform sampler2D albedo;
uniform sampler2D normalMap;

uniform Lighting {
    vec3 direction;
    vec3 colour;
    float ambient;
} lighting;

in vec2 v_uv;
in vec3 v_normal;
in vec3 v_tangent;

out vec4 fragColour;

const float GAMMA = 2.2;

float lambert(vec3 normal, vec3 light) {
    return max(dot(normalize(normal), normalize(light)), 0.0);
}

// The tangent frame, rebuilt per fragment. `mat3(a, b, c)` takes its columns
// from three vectors, which is a constructor rule rather than an overload.
vec3 perturb(vec3 normal, vec3 tangent, vec2 uv) {
    vec3 bitangent = cross(normal, tangent);
    vec3 sampled = texture(normalMap, uv).xyz * 2.0 - 1.0;
    return normalize(mat3(tangent, bitangent, normal) * sampled);
}

void main() {
    vec4 base = texture(albedo, v_uv);
    vec3 normal = perturb(normalize(v_normal), normalize(v_tangent), v_uv);
    float diffuse = lambert(normal, lighting.direction);
    vec3 lit = base.rgb * lighting.colour * (lighting.ambient + diffuse);
    fragColour = vec4(pow(lit, vec3(1.0 / GAMMA)), base.a);
}
