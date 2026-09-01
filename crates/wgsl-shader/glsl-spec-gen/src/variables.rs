//! `gl_*` pages: declaration, flow and stage.
//!
//! Two declaration shapes and one special case, all measured in
//! `research/docs-gl.md` §4:
//!
//! - `<code class="fieldsynopsis">` with `modifier`/`type`/`varname` spans —
//!   43 of the 45 variable pages, five of which carry two of them (one per
//!   stage) that merge into a single entry.
//! - `sl4/gl_Position.xhtml` and `sl4/gl_PointSize.xhtml`, which document a
//!   member of the `gl_PerVertex` block as a `<pre class="programlisting">`
//!   listing and have no `fieldsynopsis` at all.
//!
//! Stage is nowhere machine-readable, so it is read from the prose — the
//! version-table row labels and the description's opening clause — and defaults
//! to *every* stage when neither says. Rejecting a legal name is the worse
//! error.

use crate::page::{Source, by_class, flat_text, raw_text, section};
use crate::prototypes::Flow;

/// Stage bits, in the order `glsl-spec`'s `Stage` enum declares them.
pub const VERTEX: u8 = 1 << 0;
pub const TESS_CONTROL: u8 = 1 << 1;
pub const TESS_EVALUATION: u8 = 1 << 2;
pub const GEOMETRY: u8 = 1 << 3;
pub const FRAGMENT: u8 = 1 << 4;
pub const COMPUTE: u8 = 1 << 5;
pub const ALL_STAGES: u8 = (1 << 6) - 1;

/// A `gl_*` variable as one page declares it.
#[derive(Debug, Clone)]
pub struct RawVariable {
    pub name: String,
    /// The declared type with the array suffix folded in: `vec4`, `float[4]`,
    /// `float[]`.
    pub ty: String,
    pub flow: Flow,
    pub stages: u8,
}

/// The variable a `gl_*` page declares, or `None` when the page declares
/// functions instead.
pub fn parse(
    source: &Source,
    root: roxmltree::Node<'_, '_>,
    row_labels: &[String],
) -> Result<Option<RawVariable>, String> {
    if !source.stem.starts_with("gl_") {
        return Ok(None);
    }

    let synopses = by_class(root, "code", "fieldsynopsis");
    let declarations: Vec<(String, String, Flow)> = if synopses.is_empty() {
        // The `gl_PerVertex` pages. Read the member line out of the listing.
        block_member(source, root)?.into_iter().collect()
    } else {
        synopses
            .iter()
            .map(|&node| field_synopsis(source, node))
            .collect::<Result<Vec<_>, _>>()?
    };

    let Some(((ty, name), flow)) = declarations
        .first()
        .map(|(ty, name, flow)| ((ty.clone(), name.clone()), *flow))
    else {
        return Err(format!("{}: no declaration found", source.where_()));
    };
    if name != source.stem {
        return Err(format!(
            "{}: declares {name:?}, expected {:?}",
            source.where_(),
            source.stem
        ));
    }
    // A variable that is `in` in one stage and `out` in another — `gl_Layer`,
    // `gl_PrimitiveID`, `gl_ViewportIndex`.
    let flow = if declarations.iter().any(|(_, _, f)| *f != flow) { Flow::InOut } else { flow };

    let mut stages = row_labels.iter().fold(0u8, |mask, label| mask | stages_in(label));
    if stages == 0 {
        stages = stages_from_description(root);
    }
    if stages == 0 {
        stages = ALL_STAGES;
    }

    Ok(Some(RawVariable { name, ty, flow, stages }))
}

/// `<code class="fieldsynopsis"><span class="modifier">in </span>…` →
/// `("vec4", "gl_FragCoord", In)`.
fn field_synopsis(
    source: &Source,
    node: roxmltree::Node<'_, '_>,
) -> Result<(String, String, Flow), String> {
    let span = |class: &str| {
        node.descendants()
            .find(|n| n.is_element() && n.attribute("class") == Some(class))
            .map(flat_text)
    };
    let modifier = span("modifier").unwrap_or_default();
    let ty = span("type")
        .ok_or_else(|| format!("{}: fieldsynopsis has no type", source.where_()))?;
    let varname = span("varname")
        .ok_or_else(|| format!("{}: fieldsynopsis has no varname", source.where_()))?;
    let flow = match modifier.trim() {
        "out" => Flow::Out,
        "inout" => Flow::InOut,
        _ => Flow::In,
    };
    let (name, ty) = split_array_suffix(&varname, &ty);
    Ok((ty, name, flow))
}

/// The `gl_PerVertex` listing pages. The block's own `in`/`out` keyword gives
/// the flow; the member line gives the type.
fn block_member(
    source: &Source,
    root: roxmltree::Node<'_, '_>,
) -> Result<Option<(String, String, Flow)>, String> {
    let listing = by_class(root, "pre", "programlisting")
        .into_iter()
        .map(raw_text)
        .find(|text| text.contains(&source.stem))
        .ok_or_else(|| {
            format!("{}: no fieldsynopsis and no listing declaring it", source.where_())
        })?;

    let flow = if listing.trim_start().starts_with("out") { Flow::Out } else { Flow::In };
    for line in listing.lines() {
        let line = line.trim().trim_end_matches(';');
        let Some((ty, name)) = line.rsplit_once(' ') else { continue };
        let (name, ty) = split_array_suffix(name.trim(), ty.trim());
        if name == source.stem {
            return Ok(Some((ty, name, flow)));
        }
    }
    Err(format!("{}: listing declares no line for it", source.where_()))
}

/// `gl_TessLevelOuter[4]` + `float` → `("gl_TessLevelOuter", "float[4]")`. The
/// suffix rides on the *type* in the table so a lookup by bare name works.
fn split_array_suffix(varname: &str, ty: &str) -> (String, String) {
    let varname = varname.trim();
    let ty = ty.trim();
    match varname.split_once('[') {
        Some((name, suffix)) => (name.trim().to_string(), format!("{ty}[{suffix}")),
        None => (varname.to_string(), ty.to_string()),
    }
}

/// The stages the description names.
///
/// Most variable pages open with the boilerplate *"Available only in the
/// fragment language, …"*, which is an exact statement about this variable and
/// nothing else — so only that first sentence is read. A page that does not
/// open that way (`gl_Position`, which describes four stages across four
/// paragraphs) has its whole description read instead, erring towards more
/// stages rather than fewer.
fn stages_from_description(root: roxmltree::Node<'_, '_>) -> u8 {
    let Some(description) = section(root, "description") else { return 0 };
    let text: String = description
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .map(flat_text)
        .collect::<Vec<_>>()
        .join(" ");
    let lowercase = text.to_lowercase();
    if lowercase.starts_with("available only in") {
        match lowercase.find(". ") {
            Some(end) => stages_in(&lowercase[..end]),
            None => stages_in(&lowercase),
        }
    } else {
        stages_in(&lowercase)
    }
}

/// The stages a piece of prose names. Matched on lowercased text so
/// "Tessellation Control and Evaluation Languages" and "the tessellation
/// control language" both land.
fn stages_in(text: &str) -> u8 {
    let text = text.to_lowercase();
    let text = text.as_str();
    let mut stages = 0;
    if text.contains("vertex") {
        stages |= VERTEX;
    }
    if text.contains("tessellation control") || text.contains("control language") {
        stages |= TESS_CONTROL;
    }
    if text.contains("evaluation") {
        stages |= TESS_EVALUATION;
    }
    if text.contains("geometry") {
        stages |= GEOMETRY;
    }
    if text.contains("fragment") {
        stages |= FRAGMENT;
    }
    if text.contains("compute") {
        stages |= COMPUTE;
    }
    stages
}
