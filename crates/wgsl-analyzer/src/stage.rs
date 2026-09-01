//! Working out which shader stage a GLSL source belongs to.
//!
//! naga's GLSL front end needs the stage up front — GLSL has no in-language way
//! to say "this is a fragment shader", so the stage has to come from somewhere
//! else. In order of authority: the `#pragma shader_stage(…)` directive that
//! `glslc` defines, the file extension, and finally a look at what the source
//! uses.

use naga::ShaderStage;

/// What a GLSL file's extension says about its stage.
///
/// `Unknown` covers the extension-less `.glsl` case and anything we don't
/// recognise; `Unsupported` covers stages GLSL has but naga's front end does
/// not (geometry and tessellation), which we highlight but cannot validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageHint {
    Known(ShaderStage),
    Unknown,
    Unsupported,
}

/// Map a lower-case file extension (no dot) to the stage it conventionally means.
///
/// The names follow the `glslang` / `glslc` conventions plus the short forms
/// (`.vs`, `.fs`) that engines and sample code tend to use.
pub fn hint_from_extension(extension: &str) -> StageHint {
    match extension {
        "vert" | "vs" | "vsh" | "vshader" | "vertexshader" | "glslv" | "vertex" => {
            StageHint::Known(ShaderStage::Vertex)
        }
        "frag" | "fs" | "fsh" | "fshader" | "fragmentshader" | "glslf" | "fragment" | "pixel" => {
            StageHint::Known(ShaderStage::Fragment)
        }
        "comp" | "cs" | "csh" | "compute" => StageHint::Known(ShaderStage::Compute),
        // Real GLSL stages that naga's front end does not implement.
        "geom" | "geometry" | "gsh" | "gshader" | "tesc" | "tese" | "mesh" | "task" | "rgen"
        | "rchit" | "rmiss" | "rahit" | "rint" | "rcall" => StageHint::Unsupported,
        _ => StageHint::Unknown,
    }
}

/// The stage named by a `#pragma shader_stage(…)` directive, if the source has one.
///
/// `glslc` gives this directive precedence over the file extension, and so do we.
pub fn stage_from_pragma(source: &str) -> Option<ShaderStage> {
    for line in source.lines() {
        let line = line.trim_start();
        let rest = match line.strip_prefix('#') {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let rest = match rest.strip_prefix("pragma") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let rest = match rest.strip_prefix("shader_stage") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let rest = match rest.strip_prefix('(') {
            Some(rest) => rest,
            None => continue,
        };
        let name = match rest.split_once(')') {
            Some((name, _)) => name.trim(),
            None => continue,
        };
        match name {
            "vertex" => return Some(ShaderStage::Vertex),
            "fragment" => return Some(ShaderStage::Fragment),
            "compute" => return Some(ShaderStage::Compute),
            _ => return None,
        }
    }
    None
}

/// Guess the stage from what the source actually uses.
///
/// Only reached for sources with nothing better to go on — a `.glsl` file with
/// no `#pragma`. The markers are stage-exclusive built-ins, checked from the
/// most specific stage outwards; a source with none of them is treated as a
/// fragment shader, which is the common case for a bare `.glsl` file.
pub fn sniff_stage(source: &str) -> ShaderStage {
    const COMPUTE: [&str; 5] = [
        "local_size_x",
        "gl_GlobalInvocationID",
        "gl_LocalInvocationID",
        "gl_WorkGroupID",
        "gl_NumWorkGroups",
    ];
    const FRAGMENT: [&str; 5] = [
        "gl_FragColor",
        "gl_FragCoord",
        "gl_FragDepth",
        "gl_FrontFacing",
        "gl_PointCoord",
    ];
    const VERTEX: [&str; 5] = [
        "gl_Position",
        "gl_VertexID",
        "gl_VertexIndex",
        "gl_InstanceID",
        "gl_InstanceIndex",
    ];

    if COMPUTE.iter().any(|m| source.contains(m)) {
        ShaderStage::Compute
    } else if FRAGMENT.iter().any(|m| source.contains(m)) {
        ShaderStage::Fragment
    } else if VERTEX.iter().any(|m| source.contains(m)) {
        ShaderStage::Vertex
    } else {
        ShaderStage::Fragment
    }
}

/// The stage to parse `source` as, given the file extension it was read from.
///
/// Returns `None` when the extension names a stage naga cannot parse.
pub fn resolve_stage(source: &str, extension: &str) -> Option<ShaderStage> {
    if let Some(stage) = stage_from_pragma(source) {
        return Some(stage);
    }
    match hint_from_extension(extension) {
        StageHint::Known(stage) => Some(stage),
        StageHint::Unsupported => None,
        StageHint::Unknown => Some(sniff_stage(source)),
    }
}

/// The name this crate reports a stage under, matching the `#pragma` spelling.
pub fn stage_name(stage: ShaderStage) -> &'static str {
    match stage {
        ShaderStage::Vertex => "vertex",
        ShaderStage::Fragment => "fragment",
        ShaderStage::Compute => "compute",
        // Stages the resolver never produces, but the enum has.
        ShaderStage::Task => "task",
        ShaderStage::Mesh => "mesh",
        ShaderStage::RayGeneration => "raygen",
        ShaderStage::Miss => "miss",
        ShaderStage::AnyHit => "anyhit",
        ShaderStage::ClosestHit => "closesthit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_to_their_stage() {
        assert_eq!(
            hint_from_extension("vert"),
            StageHint::Known(ShaderStage::Vertex)
        );
        assert_eq!(
            hint_from_extension("fs"),
            StageHint::Known(ShaderStage::Fragment)
        );
        assert_eq!(
            hint_from_extension("comp"),
            StageHint::Known(ShaderStage::Compute)
        );
        assert_eq!(hint_from_extension("geom"), StageHint::Unsupported);
        assert_eq!(hint_from_extension("glsl"), StageHint::Unknown);
    }

    #[test]
    fn pragma_beats_the_extension() {
        let source = "#version 450\n#pragma shader_stage(compute)\nvoid main() {}\n";
        assert_eq!(resolve_stage(source, "vert"), Some(ShaderStage::Compute));
        // Even for an extension we would otherwise refuse.
        assert_eq!(resolve_stage(source, "geom"), Some(ShaderStage::Compute));
    }

    #[test]
    fn pragma_spellings() {
        assert_eq!(
            stage_from_pragma("  #  pragma  shader_stage( fragment ) // why not"),
            Some(ShaderStage::Fragment)
        );
        assert_eq!(stage_from_pragma("#pragma shader_stage(geometry)"), None);
        assert_eq!(stage_from_pragma("#version 450\nvoid main() {}"), None);
    }

    #[test]
    fn sniffing_picks_the_stage_from_built_ins() {
        assert_eq!(
            sniff_stage("layout(local_size_x = 64) in;\nvoid main() {}"),
            ShaderStage::Compute
        );
        assert_eq!(
            sniff_stage("void main() { gl_FragColor = vec4(1.0); }"),
            ShaderStage::Fragment
        );
        assert_eq!(
            sniff_stage("void main() { gl_Position = vec4(0.0); }"),
            ShaderStage::Vertex
        );
        // A shader that writes gl_Position from a compute-looking body is still
        // compute: the more specific stage wins.
        assert_eq!(
            sniff_stage("layout(local_size_x = 1) in;\nvoid main() { gl_Position; }"),
            ShaderStage::Compute
        );
        // Nothing to go on.
        assert_eq!(sniff_stage("float f(float x) { return x; }"), ShaderStage::Fragment);
    }

    #[test]
    fn unsupported_extensions_resolve_to_nothing() {
        assert_eq!(resolve_stage("void main() {}", "tesc"), None);
    }
}
