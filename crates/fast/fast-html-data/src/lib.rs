//! Static knowledge about HTML, SVG and MathML: element, attribute and event
//! tables generated at build time into the binary (`generated.rs`, committed),
//! plus a runtime loader for VS Code custom-data JSON.
//!
//! The generated tables come from `@vscode/web-custom-data` (HTML) and a
//! curated SVG/MathML dataset in `generator/svg-data.json` — VS Code ships no
//! SVG data, and the corpus this analyzer is tested against is full of inline
//! SVG. See `generator/generate.mjs`.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
}

#[derive(Debug)]
pub struct AttributeData {
    pub name: &'static str,
    pub description: &'static str,
    /// A value-less attribute (`hidden`, `disabled`): the natural target of a
    /// `?` binding.
    pub boolean: bool,
    /// Enumerated values, when the attribute has a closed set.
    pub values: &'static [&'static str],
}

#[derive(Debug)]
pub struct ElementData {
    pub name: &'static str,
    pub description: &'static str,
    pub void: bool,
    pub namespace: Namespace,
    pub attributes: &'static [AttributeData],
}

mod generated;

pub use generated::{
    EVENTS, GLOBAL_ATTRIBUTES, HTML_ELEMENTS, MATHML_ELEMENTS, SVG_ELEMENTS,
    SVG_PRESENTATION_ATTRIBUTES,
};

/// Look up an element in its namespace. `title` exists in both HTML and SVG
/// with different meanings, so the caller says which world it is in.
pub fn element_in(name: &str, namespace: Namespace) -> Option<&'static ElementData> {
    match namespace {
        Namespace::Html => HTML_ELEMENTS.get(name),
        Namespace::Svg => SVG_ELEMENTS.get(name),
        Namespace::MathMl => MATHML_ELEMENTS.get(name),
    }
}

/// Namespace-agnostic lookup, HTML first — for callers with no context.
pub fn element(name: &str) -> Option<&'static ElementData> {
    HTML_ELEMENTS
        .get(name)
        .or_else(|| SVG_ELEMENTS.get(name))
        .or_else(|| MATHML_ELEMENTS.get(name))
}

pub fn is_void(name: &str) -> bool {
    HTML_ELEMENTS.get(name).map(|e| e.void).unwrap_or(false)
}

/// Attributes valid on every HTML element.
pub fn global_attribute(name: &str) -> Option<&'static AttributeData> {
    GLOBAL_ATTRIBUTES.iter().find(|a| a.name == name)
}

/// Attributes valid on every SVG-namespace element, on top of the HTML
/// globals that also apply there (`id`, `class`, `style`, `tabindex`, …).
pub fn svg_presentation_attribute(name: &str) -> Option<&'static AttributeData> {
    SVG_PRESENTATION_ATTRIBUTES.iter().find(|a| a.name == name)
}

/// DOM events, by name without the `on` prefix.
pub fn event(name: &str) -> Option<&'static str> {
    EVENTS
        .binary_search_by(|(n, _)| n.cmp(&name))
        .ok()
        .map(|i| EVENTS[i].1)
}

pub fn events() -> impl Iterator<Item = (&'static str, &'static str)> {
    EVENTS.iter().copied()
}

pub fn elements() -> impl Iterator<Item = &'static ElementData> {
    HTML_ELEMENTS
        .values()
        .chain(SVG_ELEMENTS.values())
        .chain(MATHML_ELEMENTS.values())
}

// --------------------------------------------------------------- custom data

/// VS Code custom-data JSON (`html.customData` format), as users supply it
/// through `fastElementUltra.customHtmlData` / `html.experimental.customData`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomData {
    pub tags: Vec<CustomTag>,
    pub global_attributes: Vec<CustomAttribute>,
    pub value_sets: Vec<CustomValueSet>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomTag {
    pub name: String,
    #[serde(deserialize_with = "string_or_marked", default)]
    pub description: String,
    pub attributes: Vec<CustomAttribute>,
    /// Not part of the VS Code format; accepted as an extension so a data file
    /// can declare events too.
    pub events: Vec<CustomEvent>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomAttribute {
    pub name: String,
    #[serde(deserialize_with = "string_or_marked", default)]
    pub description: String,
    pub value_set: Option<String>,
    pub values: Vec<CustomValue>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CustomEvent {
    pub name: String,
    #[serde(deserialize_with = "string_or_marked", default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CustomValue {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomValueSet {
    pub name: String,
    pub values: Vec<CustomValue>,
}

/// Descriptions in custom data are either a plain string or a
/// `{ kind, value }` markup object.
fn string_or_marked<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Description {
        Plain(String),
        Marked { value: String },
    }
    Ok(match Description::deserialize(deserializer)? {
        Description::Plain(s) => s,
        Description::Marked { value } => value,
    })
}

pub fn parse_custom_data(json: &str) -> Result<CustomData, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_elements_are_present() {
        let div = element("div").unwrap();
        assert_eq!(div.namespace, Namespace::Html);
        assert!(!div.void);
        assert!(element("input").unwrap().void);
        assert!(is_void("br"));
        assert!(!is_void("span"));
    }

    #[test]
    fn svg_elements_are_present() {
        let path = element("path").unwrap();
        assert_eq!(path.namespace, Namespace::Svg);
        assert!(path.attributes.iter().any(|a| a.name == "d"));
        assert!(element("circle").is_some());
        assert!(element("foreignObject").is_some());
    }

    #[test]
    fn global_and_presentation_attributes() {
        assert!(global_attribute("class").is_some());
        assert!(global_attribute("tabindex").is_some());
        assert!(svg_presentation_attribute("stroke-linecap").is_some());
        assert!(svg_presentation_attribute("focusable").is_some());
    }

    #[test]
    fn events_cover_the_common_set() {
        for name in ["click", "input", "keydown", "change", "focus", "blur", "wheel", "pointerdown"] {
            assert!(event(name).is_some(), "missing event {name}");
        }
        assert!(event("clack").is_none());
    }

    #[test]
    fn events_table_is_sorted_for_binary_search() {
        let mut sorted = EVENTS.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(EVENTS, sorted.as_slice());
    }

    #[test]
    fn custom_data_round_trip() {
        let data = parse_custom_data(
            r#"{
                "version": 1.1,
                "tags": [{
                    "name": "x-widget",
                    "description": { "kind": "markdown", "value": "A widget." },
                    "attributes": [
                        { "name": "size", "values": [{ "name": "s" }, { "name": "m" }] }
                    ],
                    "events": [{ "name": "resize", "description": "Fired on resize." }]
                }],
                "globalAttributes": [{ "name": "x-anywhere" }]
            }"#,
        )
        .unwrap();
        assert_eq!(data.tags.len(), 1);
        assert_eq!(data.tags[0].description, "A widget.");
        assert_eq!(data.tags[0].attributes[0].values.len(), 2);
        assert_eq!(data.tags[0].events[0].name, "resize");
        assert_eq!(data.global_attributes[0].name, "x-anywhere");
    }
}
