//! The version-support table at the foot of every page.
//!
//! Uniform to a fault: one column set per profile, body cells that are either
//! `✔` or `-`, and nothing else (`research/docs-gl.md` §5). The *rows*, on the
//! other hand, are labelled in prose and do not line up with prototypes, which
//! is what most of this module is about (§5.1).

use crate::page::{Profile, Source, collapse, flat_text, section};
use crate::spec::comparison_key;

/// docs.gl's column labels, in the bit order `glsl-spec`'s version enums use.
const DESKTOP_COLUMNS: &[&str] = &[
    "1.10", "1.20", "1.30", "1.40", "1.50", "3.30", "4.00", "4.10", "4.20", "4.30", "4.40",
    "4.50",
];
const ES_COLUMNS: &[&str] = &["1.00", "3.00", "3.10"];

/// The bit that docs.gl has no column for, copied from the last one it does.
///
/// Desktop 4.60 and ES 3.20 postdate these tables and removed nothing, so the
/// last column carries forward. `research/docs-gl.md` §5.
const DESKTOP_EXTRAPOLATED: u32 = 12;
const ES_EXTRAPOLATED: u32 = 3;

/// One row of the table: what it is about, and where that thing exists.
#[derive(Debug, Clone)]
pub struct VersionRow {
    /// The row label as written: `mix(genDType)`, `texture (gsampler2DMS,
    /// gsampler2DMSArray)`, `gl_Layer (geometry stage)`.
    pub label: String,
    /// The label parsed into the names it is about and the type or stage
    /// tokens that qualify each.
    pub entries: Vec<RowEntry>,
    /// The versions the row ticks, as a bitmask over the profile's enum.
    pub bits: u32,
}

/// One `name (qualifiers)` clause of a row label.
///
/// The qualifiers are *alternatives*, not a conjunction: `textureSize
/// (samplerBuffer, samplerRect{Shadow})` is one row covering three unrelated
/// signature groups, not one signature taking all three.
#[derive(Debug, Clone)]
pub struct RowEntry {
    pub name: String,
    /// Every spelling this clause covers, as comparison keys.
    pub qualifiers: Vec<String>,
}

/// Read the `versions` section. `None` only for pages that have none, which is
/// no page in this corpus — the caller treats it as an error.
pub fn parse(source: &Source, root: roxmltree::Node<'_, '_>) -> Result<Vec<VersionRow>, String> {
    let columns = match source.profile {
        Profile::Desktop => DESKTOP_COLUMNS,
        Profile::Es => ES_COLUMNS,
    };
    let extrapolated = match source.profile {
        Profile::Desktop => DESKTOP_EXTRAPOLATED,
        Profile::Es => ES_EXTRAPOLATED,
    };

    let section = section(root, "versions")
        .ok_or_else(|| format!("{}: no versions section", source.where_()))?;
    let table = section
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "table")
        .ok_or_else(|| format!("{}: versions section has no table", source.where_()))?;

    let head = child_element(table, "thead")
        .ok_or_else(|| format!("{}: versions table has no thead", source.where_()))?;
    let header = element_children(head, "tr")
        .last()
        .copied()
        .ok_or_else(|| format!("{}: versions thead has no rows", source.where_()))?;
    let labels: Vec<String> =
        element_children(header, "th").iter().map(|&th| flat_text(th)).collect();
    // The first cell names what the rows are ("Function Name"/"Variable Name").
    let found: Vec<&str> = labels.iter().skip(1).map(|s| s.as_str()).collect();
    if found != columns {
        return Err(format!(
            "{}: unexpected version columns {found:?}, expected {columns:?}",
            source.where_()
        ));
    }

    let body = child_element(table, "tbody")
        .ok_or_else(|| format!("{}: versions table has no tbody", source.where_()))?;
    let mut rows = Vec::new();
    for tr in element_children(body, "tr") {
        let cells = element_children(tr, "td");
        if cells.len() != columns.len() + 1 {
            return Err(format!(
                "{}: versions row has {} cells, expected {}",
                source.where_(),
                cells.len(),
                columns.len() + 1
            ));
        }
        let label = flat_text(cells[0]);
        let mut bits = 0u32;
        for (index, &cell) in cells[1..].iter().enumerate() {
            match flat_text(cell).as_str() {
                "✔" => bits |= 1 << index,
                "-" => {}
                other => {
                    return Err(format!(
                        "{}: versions row {label:?} has cell {other:?}, expected ✔ or -",
                        source.where_()
                    ));
                }
            }
        }
        if bits & (1 << (columns.len() - 1)) != 0 {
            bits |= 1 << extrapolated;
        }
        let entries = parse_label(&label);
        rows.push(VersionRow { label, entries, bits });
    }
    Ok(rows)
}

/// Split a row label into its `name (qualifiers)` clauses.
///
/// Top-level commas separate clauses; commas inside the parentheses separate
/// qualifiers. `X{Shadow}` becomes the alternatives `X` and `XShadow`.
fn parse_label(label: &str) -> Vec<RowEntry> {
    split_top_level(label)
        .into_iter()
        .filter_map(|clause| {
            let clause = clause.trim();
            let (name, inside) = match clause.split_once('(') {
                Some((name, rest)) => (name.trim(), rest.trim_end_matches(')')),
                None => (clause, ""),
            };
            if name.is_empty() {
                return None;
            }
            let qualifiers = inside
                .split(',')
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .flat_map(alternatives)
                .collect();
            Some(RowEntry { name: name.to_string(), qualifiers })
        })
        .collect()
}

/// `gsampler2DRect{Shadow}` → `[gsampler2DRect, gsampler2DRectShadow]`; any
/// other token stands for itself. Both come back as comparison keys, so a row
/// that says `samplerBuffer` can be matched against a prototype that says
/// `gsamplerBuffer`.
fn alternatives(token: &str) -> Vec<String> {
    match token.split_once('{') {
        Some((base, tail)) => {
            let suffix = tail.trim_end_matches('}');
            vec![comparison_key(base), comparison_key(&format!("{base}{suffix}"))]
        }
        None => vec![comparison_key(token)],
    }
}

/// Split on commas that are not inside parentheses.
fn split_top_level(label: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in label.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts.into_iter().map(|p| collapse(&p)).filter(|p| !p.is_empty()).collect()
}

impl VersionRow {
    /// Whether this row says anything about `name`.
    ///
    /// Case-insensitive: `floatBitsToInt.xhtml`'s table writes
    /// `floatBitsToUInt` while its prototype writes `floatBitsToUint`, and that
    /// is the only disagreement of its kind in the corpus.
    pub fn mentions(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// The clauses of this row that are about `name`.
    pub fn entries_for<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a RowEntry> {
        self.entries.iter().filter(move |e| e.name.eq_ignore_ascii_case(name))
    }
}

impl RowEntry {
    /// Whether one of this clause's alternatives is that type.
    pub fn covers(&self, ty: &str) -> bool {
        let key = comparison_key(ty);
        self.qualifiers.contains(&key)
    }

    /// A clause with no parenthesised qualifiers — the catch-all row, used only
    /// when no qualified row claims the signature.
    pub fn is_catch_all(&self) -> bool {
        self.qualifiers.is_empty()
    }
}

/// The versions one prototype is available in, and whether a row claimed it.
///
/// Rows are prose (§5.1), so this is a best effort with a permissive fallback:
/// a qualified row that names the signature's first parameter type wins, then
/// one that names its return type, then the row with no qualifiers at all, and
/// failing everything the function's own union — which can never be narrower
/// than the truth and so can never produce a spurious "not available here".
pub fn mask_for(
    rows: &[VersionRow],
    function: &str,
    ret: &str,
    first_param: Option<&str>,
    fallback: u32,
) -> (u32, bool) {
    if let Some(first_param) = first_param {
        for row in rows {
            if row.entries_for(function).any(|e| !e.is_catch_all() && e.covers(first_param)) {
                return (row.bits, true);
            }
        }
    }
    for row in rows {
        if row.entries_for(function).any(|e| !e.is_catch_all() && e.covers(ret)) {
            return (row.bits, true);
        }
    }
    for row in rows {
        if row.entries_for(function).any(RowEntry::is_catch_all) {
            return (row.bits, true);
        }
    }
    (fallback, false)
}

/// The union of every row that names `function`, or of every row on the page
/// when none does — the authoritative, function-level mask.
pub fn union_for(rows: &[VersionRow], function: &str) -> u32 {
    let named = rows.iter().filter(|r| r.mentions(function)).fold(0, |bits, r| bits | r.bits);
    if named != 0 {
        return named;
    }
    rows.iter().fold(0, |bits, r| bits | r.bits)
}

fn child_element<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|n| n.is_element() && n.tag_name().name() == tag)
}

fn element_children<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Vec<roxmltree::Node<'a, 'input>> {
    node.children().filter(|n| n.is_element() && n.tag_name().name() == tag).collect()
}
