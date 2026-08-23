//! The component registry: per-file contributions, the five-level merge
//! order from design/component-model.md §4, and the shared answer to "what do
//! we know about this tag".
//!
//! Confidence order, highest first — a lower level fills gaps and never
//! overrides a higher one:
//!
//! 1. components declared in the program (`origin: decorator | define`)
//! 2. JSDoc-declared members on those components (folded into the facts)
//! 3. VS Code custom data
//! 4. `globalTags` / `globalAttributes` / `globalEvents`
//! 5. built-in HTML/SVG data from `fast-html-data`

use std::collections::HashMap;

use fast_html_data::{CustomData, CustomTag, ElementData, Namespace};
use fast_template_syntax::ElementKind;

use crate::protocol::{ComponentFact, Config, EventFact, MemberFact};

#[derive(Default)]
pub struct Registry {
    /// File → the components that file declared.
    by_file: HashMap<String, Vec<ComponentFact>>,
    /// File → its resolved imports.
    dependencies: HashMap<String, Vec<String>>,
    /// Parsed custom data from the config.
    custom: Vec<CustomData>,
}

/// Why the registry believes a tag exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagOrigin {
    Declaration,
    CustomData,
    GlobalTag,
    Builtin,
}

impl TagOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            TagOrigin::Declaration => "declaration",
            TagOrigin::CustomData => "customData",
            TagOrigin::GlobalTag => "globalTags",
            TagOrigin::Builtin => "builtin",
        }
    }
}

pub enum TagKnowledge<'a> {
    Components(Vec<&'a ComponentFact>),
    Custom(&'a CustomTag),
    GlobalTag,
    Builtin(&'static ElementData),
    Unknown,
}

impl TagKnowledge<'_> {
    pub fn origin(&self) -> Option<TagOrigin> {
        match self {
            TagKnowledge::Components(_) => Some(TagOrigin::Declaration),
            TagKnowledge::Custom(_) => Some(TagOrigin::CustomData),
            TagKnowledge::GlobalTag => Some(TagOrigin::GlobalTag),
            TagKnowledge::Builtin(_) => Some(TagOrigin::Builtin),
            TagKnowledge::Unknown => None,
        }
    }
}

impl Registry {
    pub fn set_custom_data(&mut self, config: &Config) {
        self.custom = config
            .custom_html_data
            .iter()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect();
    }

    pub fn upsert_file(
        &mut self,
        file_name: &str,
        components: Vec<ComponentFact>,
        dependencies: Vec<String>,
    ) {
        self.by_file.insert(file_name.to_string(), components);
        self.dependencies.insert(file_name.to_string(), dependencies);
    }

    pub fn remove_file(&mut self, file_name: &str) {
        self.by_file.remove(file_name);
        self.dependencies.remove(file_name);
    }

    pub fn components(&self) -> impl Iterator<Item = (&str, &ComponentFact)> {
        self.by_file
            .iter()
            .flat_map(|(file, comps)| comps.iter().map(move |c| (file.as_str(), c)))
    }

    pub fn components_in_file(&self, file_name: &str) -> &[ComponentFact] {
        self.by_file
            .get(file_name)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn components_for_tag(&self, tag: &str) -> Vec<&ComponentFact> {
        self.components()
            .filter(|(_, c)| c.tag_name.as_deref() == Some(tag))
            .map(|(_, c)| c)
            .collect()
    }

    pub fn component_by_source_type(&self, source_type_id: u32) -> Option<&ComponentFact> {
        self.components()
            .map(|(_, c)| c)
            .find(|c| c.source_type_id == Some(source_type_id))
    }

    /// The component whose `template:` or `styles:` names this document — the
    /// cross-file case (element.ts registering template.ts's template), which
    /// the plugin cannot mark on the document itself.
    pub fn component_for_document(&self, document_id: &str) -> Option<&ComponentFact> {
        self.components().map(|(_, c)| c).find(|c| {
            c.template_document_id.as_deref() == Some(document_id)
                || c.style_document_ids.iter().any(|id| id == document_id)
        })
    }

    /// The file that declares a tag, for `no-missing-import` and definitions.
    pub fn declaring_files(&self, tag: &str) -> Vec<&str> {
        self.by_file
            .iter()
            .filter(|(_, comps)| comps.iter().any(|c| c.tag_name.as_deref() == Some(tag)))
            .map(|(file, _)| file.as_str())
            .collect()
    }

    pub fn custom_tag(&self, tag: &str) -> Option<&CustomTag> {
        self.custom
            .iter()
            .flat_map(|d| d.tags.iter())
            .find(|t| t.name == tag)
    }

    pub fn custom_global_attribute(&self, name: &str) -> bool {
        self.custom
            .iter()
            .flat_map(|d| d.global_attributes.iter())
            .any(|a| a.name == name)
    }

    /// The merged answer for one tag, in an element-kind context (built-in
    /// lookups are namespace-aware: `<title>` inside `<svg>` is SVG's).
    pub fn lookup<'a>(
        &'a self,
        tag: &str,
        kind: ElementKind,
        config: &Config,
    ) -> TagKnowledge<'a> {
        let components = self.components_for_tag(tag);
        if !components.is_empty() {
            return TagKnowledge::Components(components);
        }
        if let Some(custom) = self.custom_tag(tag) {
            return TagKnowledge::Custom(custom);
        }
        if config.global_tags.iter().any(|t| t == tag) {
            return TagKnowledge::GlobalTag;
        }
        let namespace = match kind {
            ElementKind::Svg => Namespace::Svg,
            ElementKind::MathMl => Namespace::MathMl,
            _ => Namespace::Html,
        };
        if let Some(data) = fast_html_data::element_in(tag, namespace) {
            return TagKnowledge::Builtin(data);
        }
        TagKnowledge::Unknown
    }

    /// Every tag name completion could offer: declared components, custom
    /// data, global tags. Built-ins are appended by the caller.
    pub fn known_custom_tags(&self, config: &Config) -> Vec<String> {
        let mut tags: Vec<String> = self
            .components()
            .filter_map(|(_, c)| c.tag_name.clone())
            .collect();
        tags.extend(self.custom.iter().flat_map(|d| d.tags.iter()).map(|t| t.name.clone()));
        tags.extend(config.global_tags.iter().cloned());
        tags.sort();
        tags.dedup();
        tags
    }

    // -------------------------------------------------------- member lookup

    /// The declared attribute for `name` on a component, searching every
    /// declaration of the tag.
    pub fn find_attribute<'a>(
        &self,
        components: &[&'a ComponentFact],
        name: &str,
    ) -> Option<&'a MemberFact> {
        components
            .iter()
            .flat_map(|c| c.attributes.iter())
            .find(|a| a.name.eq_ignore_ascii_case(name))
    }

    pub fn find_property<'a>(
        &self,
        components: &[&'a ComponentFact],
        name: &str,
    ) -> Option<&'a MemberFact> {
        components
            .iter()
            .flat_map(|c| c.properties.iter())
            .find(|p| p.name == name)
            .or_else(|| {
                // An `@attr` also creates a property, under its property name.
                components
                    .iter()
                    .flat_map(|c| c.attributes.iter())
                    .find(|a| a.property_name.as_deref() == Some(name))
            })
    }

    pub fn find_event<'a>(
        &self,
        components: &[&'a ComponentFact],
        name: &str,
    ) -> Option<&'a EventFact> {
        components
            .iter()
            .flat_map(|c| c.events.iter())
            .find(|e| e.name == name)
    }

    // ---------------------------------------------------------- reachability

    /// Is any declaring file of `tag` reachable from `from` through imports,
    /// within `max_depth` (-1 = unlimited)?
    pub fn is_reachable(&self, from: &str, tag: &str, max_depth: i32) -> bool {
        let targets = self.declaring_files(tag);
        if targets.is_empty() {
            return true;
        }
        if targets.contains(&from) {
            return true;
        }
        let mut visited = std::collections::HashSet::new();
        let mut frontier = vec![from.to_string()];
        visited.insert(from.to_string());
        let mut depth = 0;
        while !frontier.is_empty() && (max_depth < 0 || depth < max_depth) {
            depth += 1;
            let mut next = Vec::new();
            for file in frontier {
                for dep in self.dependencies.get(&file).into_iter().flatten() {
                    if targets.iter().any(|t| *t == dep) {
                        return true;
                    }
                    if visited.insert(dep.clone()) {
                        next.push(dep.clone());
                    }
                }
            }
            frontier = next;
        }
        false
    }
}
