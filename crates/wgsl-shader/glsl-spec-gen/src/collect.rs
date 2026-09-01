//! Reading both profiles and merging them into one table each.
//!
//! `el3/`'s page names are a strict subset of `sl4/`'s (`research/docs-gl.md`
//! §1), so the merge is a name-keyed join: one entry per builtin carrying a
//! desktop mask and an ES mask. Signatures that appear in both profiles merge
//! into one overload with both masks set; a signature only one profile has
//! keeps the other's mask empty, which is how ES's genuinely different
//! `texture(samplerCubeShadow, vec4)` survives next to the desktop `vec3` form.
//!
//! Prose comes from `sl4/` whenever the page exists there, because the desktop
//! page is the superset and having one description per builtin is the point of
//! merging. ES-only pages — of which this corpus has none, but the code does
//! not assume it — fall back to `el3/`'s.

use std::collections::BTreeMap;

use crate::markdown;
use crate::page::{Profile, Source, by_class, flat_text, load_profile, section};
use crate::prototypes::{self, Flow, RawOverload};
use crate::spec;
use crate::variables::{self, RawVariable};
use crate::versions::{self, VersionRow};
use std::path::Path;

/// A builtin function, merged across profiles.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub overloads: Vec<Overload>,
    pub doc: String,
    /// Sorted by parameter name.
    pub param_docs: Vec<(String, String)>,
    pub desktop: u32,
    pub es: u32,
}

/// One signature, with the availability of that signature alone.
#[derive(Debug, Clone)]
pub struct Overload {
    pub raw: RawOverload,
    pub desktop: u32,
    pub es: u32,
}

/// A `gl_*` variable, merged across profiles.
#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub ty: String,
    pub flow: Flow,
    pub stages: u8,
    pub doc: String,
    pub desktop: u32,
    pub es: u32,
}

/// Everything the generator produces, plus what it learned on the way.
#[derive(Debug, Default)]
pub struct Spec {
    /// Sorted by name.
    pub functions: Vec<Function>,
    /// Sorted by name.
    pub variables: Vec<Variable>,
    /// Sorted family names, with their members.
    pub families: Vec<(String, Vec<String>)>,
    pub stats: Stats,
}

/// What a run is worth reporting: enough to notice a docs.gl change without
/// reading the diff.
#[derive(Debug, Default)]
pub struct Stats {
    pub pages: usize,
    pub redirects: usize,
    pub prototypes: usize,
    /// Overloads whose availability came from a matched version-table row
    /// rather than the function-level fallback (§5.1).
    pub matched_overloads: usize,
    pub total_overloads: usize,
}

/// Read a docs.gl checkout and merge both profiles.
pub fn collect(root: &Path) -> Result<Spec, String> {
    let mut spec = Spec::default();
    // Desktop first: it decides the prose, and `Profile::ALL` is in that order.
    let mut functions: BTreeMap<String, Function> = BTreeMap::new();
    let mut vars: BTreeMap<String, Variable> = BTreeMap::new();
    let mut families: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for profile in Profile::ALL {
        for source in load_profile(root, profile)? {
            if source.skipped().is_some() {
                spec.stats.redirects += 1;
                continue;
            }
            spec.stats.pages += 1;
            let document = roxmltree::Document::parse(&source.text)
                .map_err(|e| format!("{}: not well-formed XML: {e}", source.where_()))?;
            let root_node = document.root_element();
            let rows = versions::parse(&source, root_node)?;
            let page = Page { source: &source, rows, root: root_node };
            page.collect_into(&mut functions, &mut vars, &mut families, &mut spec.stats)?;
        }
    }

    spec.functions = functions.into_values().collect();
    spec.variables = vars.into_values().collect();
    spec.families = families.into_iter().collect();
    for function in &mut spec.functions {
        function.overloads.sort_by_key(|o| o.raw.signature());
    }
    Ok(spec)
}

/// One page, parsed far enough to contribute to the tables.
struct Page<'a, 'input> {
    source: &'a Source,
    rows: Vec<VersionRow>,
    root: roxmltree::Node<'a, 'input>,
}

impl Page<'_, '_> {
    fn collect_into(
        &self,
        functions: &mut BTreeMap<String, Function>,
        variables: &mut BTreeMap<String, Variable>,
        families: &mut BTreeMap<String, Vec<String>>,
        stats: &mut Stats,
    ) -> Result<(), String> {
        let row_labels: Vec<String> = self.rows.iter().map(|r| r.label.clone()).collect();
        if let Some(raw) = variables::parse(self.source, self.root, &row_labels)? {
            self.merge_variable(raw, variables);
            return Ok(());
        }

        let overloads = prototypes::parse(self.source, self.root)?;
        if overloads.is_empty() {
            return Err(format!("{}: no prototypes and no variable", self.source.where_()));
        }
        stats.prototypes += overloads.len();

        for name in prototypes::families(&overloads) {
            let members = spec::family_members(&name)
                .ok_or_else(|| format!("{}: unknown family {name:?}", self.source.where_()))?;
            families.entry(name).or_insert(members);
        }

        let doc = self.description();
        let param_docs = self.param_docs();

        for overload in overloads {
            let function_bits = versions::union_for(&self.rows, &overload.function);
            if function_bits == 0 {
                return Err(format!(
                    "{}: version table ticks nothing for {:?}",
                    self.source.where_(),
                    overload.function
                ));
            }
            let (bits, matched) = versions::mask_for(
                &self.rows,
                &overload.function,
                &overload.ret,
                overload.params.first().map(|p| p.ty.as_str()),
                function_bits,
            );
            stats.total_overloads += 1;
            stats.matched_overloads += usize::from(matched);

            let entry = functions.entry(overload.function.clone()).or_insert_with(|| Function {
                name: overload.function.clone(),
                overloads: Vec::new(),
                doc: doc.clone(),
                param_docs: param_docs.clone(),
                desktop: 0,
                es: 0,
            });
            match self.source.profile {
                Profile::Desktop => entry.desktop |= function_bits,
                Profile::Es => entry.es |= function_bits,
            }
            match entry.overloads.iter_mut().find(|o| o.raw == overload) {
                Some(existing) => match self.source.profile {
                    Profile::Desktop => existing.desktop |= bits,
                    Profile::Es => existing.es |= bits,
                },
                None => {
                    let (desktop, es) = match self.source.profile {
                        Profile::Desktop => (bits, 0),
                        Profile::Es => (0, bits),
                    };
                    entry.overloads.push(Overload { raw: overload, desktop, es });
                }
            }
        }
        Ok(())
    }

    fn merge_variable(&self, raw: RawVariable, variables: &mut BTreeMap<String, Variable>) {
        let bits = self.rows.iter().fold(0, |bits, row| bits | row.bits);
        let entry = variables.entry(raw.name.clone()).or_insert_with(|| Variable {
            name: raw.name.clone(),
            ty: raw.ty.clone(),
            flow: raw.flow,
            stages: raw.stages,
            doc: self.description(),
            desktop: 0,
            es: 0,
        });
        entry.stages |= raw.stages;
        match self.source.profile {
            Profile::Desktop => entry.desktop |= bits,
            Profile::Es => entry.es |= bits,
        }
    }

    fn description(&self) -> String {
        section(self.root, "description").map(markdown::render).unwrap_or_default()
    }

    /// The `parameters` variable list, `term` → prose, sorted by term.
    fn param_docs(&self) -> Vec<(String, String)> {
        let Some(section) = section(self.root, "parameters") else { return Vec::new() };
        let terms = by_class(section, "span", "term");
        let mut docs: Vec<(String, String)> = terms
            .iter()
            .filter_map(|&term| {
                let dt = term.ancestors().find(|n| n.tag_name().name() == "dt")?;
                let dd = dt
                    .next_siblings()
                    .find(|n| n.is_element() && n.tag_name().name() == "dd")?;
                let name = flat_text(term);
                let doc = markdown::render_paragraphs(dd);
                (!name.is_empty() && !doc.is_empty()).then_some((name, doc))
            })
            .collect();
        docs.sort();
        docs.dedup_by(|a, b| a.0 == b.0);
        docs
    }
}
