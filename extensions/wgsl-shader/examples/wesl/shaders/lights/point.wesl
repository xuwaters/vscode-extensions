// A module in a subdirectory: `package::lights::point`. `super` is
// `package::lights`, and `super::super` is the package root.

import super::super::lighting::{attenuate, diffuse, Light};

/// The light a point source casts on a surface.
fn shade(light: Light, position: vec3f, normal: vec3f) -> vec3f {
    let to_light = light.position - position;
    return light.colour * diffuse(normal, to_light) * attenuate(length(to_light));
}
