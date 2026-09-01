//! Which GLSL naga is being asked to validate, and whether it can.
//!
//! naga's GLSL front end exists to feed `wgpu`, so the language it implements
//! is *Vulkan* GLSL. Desktop OpenGL GLSL differs in two places that matter, and
//! naga reports both as errors:
//!
//! - **Bindings.** Vulkan GLSL writes `layout(set = 0, binding = 1)` on every
//!   resource; OpenGL lets the driver assign block bindings, and naga has
//!   nothing to fall back on — "uniform/buffer blocks require layout(binding=X)".
//! - **Samplers.** Vulkan GLSL keeps textures and samplers apart, `texture2D`
//!   plus `sampler`, and fuses them at the call site. OpenGL's combined
//!   `sampler2D` is not a type naga knows, so the declaration fails to parse at
//!   all — "Not implemented: variable qualifier", which does not name the real
//!   cause.
//!
//! Those errors are correct for a shader aimed at Vulkan or `wgpu` and pure
//! noise for one aimed at OpenGL or WebGL, and nothing in a `.frag` says which
//! it is. So the dialect is a setting, defaulting to a look at the source: a
//! shader using either OpenGL-only construct is highlighted and analysed as
//! usual but not validated, on the same grounds as a `#version 300 es` source.

use serde::{Deserialize, Serialize};

/// The `#version` declarations naga's GLSL front end accepts.
const SUPPORTED_VERSIONS: [u32; 3] = [440, 450, 460];

/// The half of every skip message that is always true.
const VULKAN_ONLY: &str = "naga's GLSL front end implements Vulkan GLSL only";

/// The GLSL dialect to validate against — `glsl.validate.dialect`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    /// Validate, unless the source uses a construct only OpenGL GLSL has.
    #[default]
    Auto,
    /// Always validate. Vulkan GLSL is the dialect naga implements.
    Vulkan,
    /// Never validate. naga has no OpenGL GLSL front end to do it with.
    OpenGl,
}

/// Why this source will not be validated, in words a user can act on.
///
/// `None` means go ahead and run naga.
pub fn skip_reason(source: &str, dialect: Dialect) -> Option<String> {
    // The version check comes first whatever the dialect: a `#version 300 es`
    // source is one naga cannot parse even when it is written Vulkan-style.
    if let Some(reason) = version_skip_reason(source) {
        return Some(reason);
    }
    match dialect {
        Dialect::Vulkan => None,
        Dialect::OpenGl => {
            Some(format!("{VULKAN_ONLY}, and `glsl.validate.dialect` is set to `opengl`"))
        }
        Dialect::Auto => opengl_marker(source).map(|marker| format!("{VULKAN_ONLY}, and {marker}")),
    }
}

/// Why this `#version` cannot be validated, in words a user can act on.
fn version_skip_reason(source: &str) -> Option<String> {
    let (version, profile) = declared_version(source)?;
    if profile != Some("es") && SUPPORTED_VERSIONS.contains(&version) {
        return None;
    }
    let suffix = profile.map(|p| format!(" {p}")).unwrap_or_default();
    Some(format!(
        "naga's GLSL front end accepts #version 440, 450 and 460 core; \
         this file declares {version}{suffix}"
    ))
}

/// The `#version` line's number and profile, if the source declares one.
fn declared_version(source: &str) -> Option<(u32, Option<&str>)> {
    for line in source.lines() {
        // `#version` need not be the first line — comments and other
        // directives may precede it — so keep looking rather than giving up.
        let Some(rest) = line.trim_start().strip_prefix('#') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("version") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let number = words.next()?.parse().ok()?;
        return Some((number, words.next()));
    }
    None
}

/// The keywords that introduce a resource declaration naga has to bind.
const RESOURCE_KEYWORDS: [&str; 2] = ["uniform", "buffer"];

/// The first construct in `source` that only OpenGL GLSL has, described as a
/// clause that follows "…implements Vulkan GLSL only, and ".
///
/// Deliberately conservative: everything it looks at is a resource declaration,
/// because a false positive here silently switches validation off for a file
/// that wanted it. Missing a marker only leaves the status quo — naga runs and
/// the user sees its errors.
fn opengl_marker(source: &str) -> Option<String> {
    let source = strip_comments(source);

    // Both keywords, in the order they appear, so the marker reported is the
    // first one a reader would hit scrolling down the file.
    let mut declarations: Vec<(usize, &'static str)> = Vec::new();
    for keyword in RESOURCE_KEYWORDS {
        let mut from = 0;
        while let Some(offset) = source[from..].find(keyword) {
            let at = from + offset;
            if is_whole_word(&source, at, keyword.len()) {
                declarations.push((at, keyword));
            }
            from = at + keyword.len();
        }
    }
    declarations.sort_unstable();

    for (at, keyword) in declarations {
        // Whatever sits between the end of the previous statement and the
        // keyword is this declaration's own qualifier list.
        let layout_start = source[..at].rfind([';', '}']).map(|end| end + 1).unwrap_or(0);
        let layout = &source[layout_start..at];

        let rest = &source[at + keyword.len()..];
        let (head, is_block) = match rest.find(['{', ';']) {
            Some(end) => (&rest[..end], rest.as_bytes()[end] == b'{'),
            None => (rest, false),
        };

        if let Some(sampler) = words(head).find(|word| is_combined_sampler(word)) {
            return Some(format!(
                "this file declares `{sampler}`, a combined image sampler that Vulkan GLSL \
                 splits into a separate `texture…` and `sampler`"
            ));
        }

        // A push constant block is the one resource naga binds without a
        // `binding`, so it must not count as an implicit OpenGL binding.
        if is_block && !layout.contains("binding") && !layout.contains("push_constant") {
            let name = words(head).next().unwrap_or(keyword);
            return Some(format!(
                "the `{name}` {keyword} block has no `layout(binding = …)`, which only OpenGL \
                 assigns implicitly"
            ));
        }
    }
    None
}

/// The identifier-shaped words in a declaration head.
fn words(head: &str) -> impl Iterator<Item = &str> {
    head.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|word| !word.is_empty())
}

/// Whether a type name is one of OpenGL's combined image samplers.
///
/// naga implements `sampler` and `samplerShadow` — a sampler on its own. Every
/// other `sampler…` name fuses a texture to it, which is what it cannot parse.
fn is_combined_sampler(word: &str) -> bool {
    let suffix = word
        .strip_prefix("sampler")
        .or_else(|| word.strip_prefix("isampler"))
        .or_else(|| word.strip_prefix("usampler"));
    suffix.is_some_and(|suffix| !suffix.is_empty() && suffix != "Shadow")
}

/// Whether the `len` bytes at `at` stand alone rather than sitting inside a
/// longer identifier — `buffer` the keyword, not the tail of `samplerBuffer`.
fn is_whole_word(source: &str, at: usize, len: usize) -> bool {
    let part_of_identifier = |c: char| c.is_alphanumeric() || c == '_';
    source[..at].chars().next_back().is_none_or(|c| !part_of_identifier(c))
        && source[at + len..].chars().next().is_none_or(|c| !part_of_identifier(c))
}

/// The source with every comment blanked out, byte offsets preserved.
///
/// The scan above reads whole declarations, so a `sampler2D` written in a
/// comment — as this extension's own example fragment shader does, explaining
/// the very split described here — must not count as one.
fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            while index < bytes.len() && bytes[index] != b'\n' {
                out.push(' ');
                index += 1;
            }
        } else if bytes[index..].starts_with(b"/*") {
            let end = source[index + 2..]
                .find("*/")
                .map(|offset| index + 2 + offset + 2)
                .unwrap_or(bytes.len());
            while index < end {
                // Newlines survive so `#version` line scanning is unaffected;
                // everything else becomes a space.
                out.push(if bytes[index] == b'\n' { '\n' } else { ' ' });
                index += 1;
            }
        } else {
            // Copy whole characters: a multi-byte one in an identifier or a
            // comment must not be split into invalid UTF-8.
            let character = source[index..].chars().next().expect("index is on a boundary");
            out.push(character);
            index += character.len_utf8();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the user hits first: OpenGL's implicit block binding.
    #[test]
    fn an_unbound_uniform_block_is_an_opengl_marker() {
        let source = "#version 450\n\
            uniform VertexInfo {\n  mat4 mvp;\n  mat4 model;\n}\nvertex_info;\n";
        let marker = opengl_marker(source).expect("flagged");
        assert!(marker.contains("VertexInfo"), "{marker}");
        assert!(marker.contains("uniform block"), "{marker}");
    }

    #[test]
    fn a_combined_image_sampler_is_an_opengl_marker() {
        let source = "#version 450\nuniform sampler2D albedoTexture;\n";
        let marker = opengl_marker(source).expect("flagged");
        assert!(marker.contains("sampler2D"), "{marker}");
    }

    /// The separated form naga does implement, which must never be flagged.
    #[test]
    fn vulkan_style_declarations_are_not_flagged() {
        let source = "#version 450\n\
            layout(set = 0, binding = 0) uniform Camera { mat4 view; } camera;\n\
            layout(set = 0, binding = 1) uniform texture2D albedo;\n\
            layout(set = 0, binding = 2) uniform sampler albedo_sampler;\n\
            layout(set = 0, binding = 3) uniform samplerShadow shadow_sampler;\n\
            layout(std430, binding = 4) buffer Values { float values[]; };\n";
        assert_eq!(opengl_marker(source), None);
    }

    /// `sampler2D(texture, sampler)` is a legal *constructor* in Vulkan GLSL
    /// even though it is not a legal type, so a call must not be mistaken for
    /// a declaration.
    #[test]
    fn the_sampler2d_constructor_is_not_a_declaration() {
        let source = "#version 450\n\
            layout(binding = 0) uniform texture2D albedo;\n\
            layout(binding = 1) uniform sampler albedo_sampler;\n\
            void main() { vec4 c = texture(sampler2D(albedo, albedo_sampler), vec2(0.0)); }\n";
        assert_eq!(opengl_marker(source), None);
    }

    #[test]
    fn a_push_constant_block_needs_no_binding() {
        let source = "#version 450\nlayout(push_constant) uniform Push { mat4 mvp; } push;\n";
        assert_eq!(opengl_marker(source), None);
    }

    #[test]
    fn a_sampler2d_in_a_comment_does_not_count() {
        let source = "#version 450\n\
            // Textures and samplers are separate: sampler2D(tex, samp) at the call site.\n\
            /* uniform sampler2D legacy; */\n\
            layout(binding = 0) uniform texture2D albedo;\n";
        assert_eq!(opengl_marker(source), None);
    }

    /// `buffer` is only a keyword on its own; `samplerBuffer` is a type name.
    #[test]
    fn a_keyword_inside_an_identifier_is_not_a_declaration() {
        assert!(is_whole_word("uniform x;", 0, "uniform".len()));
        assert!(!is_whole_word("samplerBuffer b;", 7, "Buffer".len()));
    }

    #[test]
    fn combined_samplers_are_the_ones_naga_lacks() {
        for name in ["sampler2D", "samplerCube", "isampler2DArray", "usampler3D"] {
            assert!(is_combined_sampler(name), "{name}");
        }
        for name in ["sampler", "samplerShadow", "texture2D", "vec4"] {
            assert!(!is_combined_sampler(name), "{name}");
        }
    }

    #[test]
    fn the_dialect_setting_overrides_the_source() {
        let opengl = "#version 450\nuniform sampler2D albedo;\n";
        // Auto looks; vulkan insists; opengl refuses.
        assert!(skip_reason(opengl, Dialect::Auto).is_some());
        assert_eq!(skip_reason(opengl, Dialect::Vulkan), None);
        assert!(skip_reason(opengl, Dialect::OpenGl).is_some());

        // A version naga cannot parse is skipped whatever the dialect says.
        let es = "#version 300 es\nvoid main() {}\n";
        assert!(skip_reason(es, Dialect::Vulkan).is_some_and(|r| r.contains("300 es")));
    }

    #[test]
    fn supported_versions_are_not_skipped() {
        for version in ["440", "450", "460"] {
            let source = format!("#version {version}\nvoid main() {{}}\n");
            assert!(version_skip_reason(&source).is_none(), "{version}");
        }
        // No `#version` at all is fine too.
        assert!(version_skip_reason("void main() {}\n").is_none());
    }

    #[test]
    fn the_dialect_names_round_trip_through_the_wire_spelling() {
        let parse = |text: &str| serde_json::from_str::<Dialect>(text).unwrap();
        assert_eq!(parse("\"auto\""), Dialect::Auto);
        assert_eq!(parse("\"vulkan\""), Dialect::Vulkan);
        assert_eq!(parse("\"opengl\""), Dialect::OpenGl);
        assert_eq!(Dialect::default(), Dialect::Auto);
    }

    #[test]
    fn stripping_comments_preserves_offsets_and_lines() {
        let source = "a // b\nc /* d\ne */ f\n";
        let stripped = strip_comments(source);
        assert_eq!(stripped.len(), source.len());
        assert_eq!(stripped.lines().count(), source.lines().count());
        assert!(!stripped.contains('b'), "{stripped:?}");
        assert!(stripped.contains('a') && stripped.contains('c') && stripped.contains('f'));
    }
}
