//! Prototypes: `<table class="funcprototype-table">` → structured overloads.
//!
//! The first cell is `RET name(`, every later cell is one parameter, and the
//! last carries the closing `);`. Across all 1,168 prototypes in the corpus the
//! flattened text of those cells matched that shape exactly, so this parser is
//! strict on purpose: anything it cannot read stops the run rather than being
//! silently dropped (`research/docs-gl.md` §3).

use crate::page::{Source, by_class, flat_text};
use crate::spec::{is_family, normalize_type};

/// One prototype, still in the reference page's own vocabulary — types are
/// spellings, families are not yet expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOverload {
    /// The name the prototype declares, which is not always the page's name:
    /// `packUnorm.xhtml` declares four different functions.
    pub function: String,
    pub ret: String,
    pub params: Vec<RawParam>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawParam {
    pub name: String,
    pub ty: String,
    pub flow: Flow,
    /// Written `[float bias]` — omissible at the call site.
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    In,
    Out,
    InOut,
}

impl Flow {
    pub const fn variant(self) -> &'static str {
        match self {
            Flow::In => "In",
            Flow::Out => "Out",
            Flow::InOut => "InOut",
        }
    }
}

impl RawOverload {
    /// Every type spelling this prototype mentions, return type included —
    /// what a version-table row's qualifiers are matched against.
    pub fn types(&self) -> Vec<String> {
        let mut types = Vec::with_capacity(self.params.len() + 1);
        types.push(self.ret.clone());
        types.extend(self.params.iter().map(|p| p.ty.clone()));
        types
    }

    /// A stable key for merging the same signature across profiles.
    pub fn signature(&self) -> String {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                format!(
                    "{}{}{} {}{}",
                    if p.optional { "[" } else { "" },
                    match p.flow {
                        Flow::In => "",
                        Flow::Out => "out ",
                        Flow::InOut => "inout ",
                    },
                    p.ty,
                    p.name,
                    if p.optional { "]" } else { "" }
                )
            })
            .collect();
        format!("{} {}({})", self.ret, self.function, params.join(", "))
    }
}

/// Every prototype on a page, in document order.
pub fn parse(source: &Source, root: roxmltree::Node<'_, '_>) -> Result<Vec<RawOverload>, String> {
    let mut overloads = Vec::new();
    for table in by_class(root, "table", "funcprototype-table") {
        let cells: Vec<String> = table
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "td")
            .map(flat_text)
            .filter(|text| !text.is_empty())
            .collect();
        let Some((head, rest)) = cells.split_first() else {
            return Err(format!("{}: empty prototype table", source.where_()));
        };
        let (ret, function) = split_head(head)
            .ok_or_else(|| format!("{}: cannot read prototype head {head:?}", source.where_()))?;

        let mut params = Vec::with_capacity(rest.len());
        for cell in rest {
            let cell = cell.trim_end_matches(';').trim_end().trim_end_matches(')').trim();
            let cell = cell.trim_end_matches(',').trim();
            let optional = cell.starts_with('[');
            let text = cell.trim_matches(['[', ']']).trim();
            // `void EmitVertex(void)` — a sole `void` is an empty list.
            if text == "void" {
                continue;
            }
            params.push(parse_param(source, text, optional)?);
        }
        overloads.push(RawOverload {
            function: function.to_string(),
            ret: normalize_type(&ret).to_string(),
            params,
        });
    }
    Ok(overloads)
}

/// `genType mix(` → `("genType", "mix")`.
fn split_head(head: &str) -> Option<(String, String)> {
    let head = head.strip_suffix('(')?.trim_end();
    let (ret, name) = head.rsplit_once(' ')?;
    Some((ret.trim().to_string(), name.trim().to_string()))
}

fn parse_param(source: &Source, text: &str, optional: bool) -> Result<RawParam, String> {
    let mut words = text.split_whitespace().collect::<Vec<_>>();
    let flow = match words.first().copied() {
        Some("out") => {
            words.remove(0);
            Flow::Out
        }
        Some("inout") => {
            words.remove(0);
            Flow::InOut
        }
        Some("in") => {
            words.remove(0);
            Flow::In
        }
        _ => Flow::In,
    };
    let [ty, name] = words[..] else {
        return Err(format!("{}: cannot read parameter {text:?}", source.where_()));
    };
    // `sl4/texelFetch.xhtml` writes `sample sample`, dropping the type: the
    // spec's signature is `int sample`. research/docs-gl.md §3.1.
    let ty = if ty == "sample" && name == "sample" { "int" } else { ty };
    let ty = normalize_type(ty);
    Ok(RawParam {
        name: name.to_string(),
        ty: ty.to_string(),
        flow,
        optional,
    })
}

/// Every type spelling on a page that is a generic family — the input to the
/// generated family table.
pub fn families(overloads: &[RawOverload]) -> Vec<String> {
    let mut names: Vec<String> = overloads
        .iter()
        .flat_map(RawOverload::types)
        .filter(|t| is_family(t))
        .collect();
    names.sort();
    names.dedup();
    names
}
