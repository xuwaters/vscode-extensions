#version 300 es

// GLSL ES 3.00 — what WebGL 2 and mobile OpenGL ES speak, and what the old
// naga-backed validation could not read at all. Combined samplers, precision
// statements and no bindings: all of it is analysed now.
//
// Hover `texture` here and you get the ES 3.00 overloads, not the desktop ones:
// there is no `sampler1D` in this language, and `texture2D` is the GLSL ES 1.00
// spelling of what this file calls `texture`.

precision highp float;
precision highp int;

// A macro the host prepends before compiling is ordinary practice in WebGL.
// It is expanded for real, and a mistake inside one is reported on the line
// that wrote it rather than on the definition.
#define SATURATE(x) clamp(x, 0.0, 1.0)

uniform sampler2D u_albedo;

uniform Material {
    vec3 tint;
    float exposure;
} material;

in vec2 v_uv;
in vec3 v_normal;

layout(location = 0) out vec4 fragColour;

const vec3 LIGHT_DIRECTION = vec3(0.0, 1.0, 0.0);

float lambert(vec3 normal, vec3 light) {
    return max(dot(normalize(normal), normalize(light)), 0.0);
}

// Only the live side of a conditional is analysed; the other side still shows
// up in the outline and is dimmed in the editor.
#ifdef USE_TONEMAP
vec3 tonemap(vec3 colour) {
    return colour / (colour + vec3(1.0));
}
#else
vec3 tonemap(vec3 colour) {
    return pow(colour, vec3(1.0 / 2.2));
}
#endif

void main() {
    vec4 base = texture(u_albedo, v_uv);
    float diffuse = lambert(v_normal, LIGHT_DIRECTION);
    vec3 lit = base.rgb * material.tint * (0.2 + 0.8 * diffuse);
    fragColour = vec4(tonemap(SATURATE(lit * material.exposure)), base.a);
}
