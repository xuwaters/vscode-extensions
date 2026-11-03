// Shared lighting helpers, imported by main.wesl.

/// Inverse-square falloff, clamped so a light never divides by zero.
fn attenuate(distance: f32) -> f32 {
    return 1.0 / max(distance * distance, 1e-4);
}

/// Lambert's cosine law.
fn diffuse(normal: vec3f, to_light: vec3f) -> f32 {
    return max(dot(normalize(normal), normalize(to_light)), 0.0);
}

struct Light {
    position: vec3f,
    colour: vec3f,
}
