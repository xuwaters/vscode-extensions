//! The FAST template analysis engine.
//!
//! A pure function of what it is told: the plugin feeds it component facts,
//! dependency lists and virtual documents (`upsert_file`), and asks questions
//! (`analyze`, `query`). It does no I/O, never calls back into the host, and
//! what it cannot decide — type assignability — it declares as binding facts
//! for the plugin to answer (decision 0002).
//!
//! Everything here is testable with `cargo test` against recorded JSON
//! payloads; `fast-analyzer-wasm` is the only crate that knows WASM exists.

use std::collections::HashMap;

pub mod config;
pub mod documents;
pub mod ide;
pub mod protocol;
pub mod registry;
pub mod rules;
pub mod suggest;

use documents::DocumentState;
use protocol::{AnalyzeResult, Config, Query, UpsertFile};
use registry::Registry;

/// DOM properties offered for `:` completion on built-in elements — shared
/// between the rule pass and completion.
pub fn rules_dom_extra_properties() -> &'static [&'static str] {
    rules::DOM_EXTRA_PROPERTIES
}

#[derive(Default)]
pub struct Engine {
    config: Config,
    registry: Registry,
    documents: HashMap<String, DocumentState>,
    docs_by_file: HashMap<String, Vec<String>>,
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    pub fn set_config(&mut self, config: Config) {
        self.registry.set_custom_data(&config);
        self.config = config;
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn document(&self, id: &str) -> Option<&DocumentState> {
        self.documents.get(id)
    }

    pub fn documents(&self) -> impl Iterator<Item = &DocumentState> {
        self.documents.values()
    }

    /// Replace everything `file_name` previously contributed.
    pub fn upsert_file(&mut self, upsert: UpsertFile) {
        let UpsertFile {
            file_name,
            mut dependencies,
            node_module_dependencies,
            components,
            documents,
            global_events,
        } = upsert;
        // Reachability treats project and node_modules imports alike; the
        // depth limits differ only in configuration today.
        dependencies.extend(node_module_dependencies);
        self.remove_file_documents(&file_name);
        self.registry
            .upsert_file(&file_name, components, global_events, dependencies);
        let mut ids = Vec::with_capacity(documents.len());
        for fact in documents {
            ids.push(fact.id.clone());
            self.documents.insert(fact.id.clone(), DocumentState::new(fact));
        }
        self.docs_by_file.insert(file_name, ids);
    }

    pub fn remove_file(&mut self, file_name: &str) {
        self.remove_file_documents(file_name);
        self.registry.remove_file(file_name);
    }

    fn remove_file_documents(&mut self, file_name: &str) {
        if let Some(ids) = self.docs_by_file.remove(file_name) {
            for id in ids {
                self.documents.remove(&id);
            }
        }
    }

    pub fn analyze(&self, document_id: &str) -> Option<AnalyzeResult> {
        let doc = self.documents.get(document_id)?;
        Some(rules::analyze_document(doc, &self.registry, &self.config))
    }

    // ------------------------------------------------------- JSON boundary

    pub fn set_config_json(&mut self, json: &str) -> Result<(), String> {
        let config: Config = serde_json::from_str(json).map_err(|e| e.to_string())?;
        self.set_config(config);
        Ok(())
    }

    pub fn upsert_file_json(&mut self, json: &str) -> Result<(), String> {
        let upsert: UpsertFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        self.upsert_file(upsert);
        Ok(())
    }

    pub fn analyze_json(&self, document_id: &str) -> Result<String, String> {
        let result = self
            .analyze(document_id)
            .ok_or_else(|| format!("unknown document {document_id}"))?;
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    pub fn query_json(&self, json: &str) -> Result<String, String> {
        let query: Query = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let value = self.query(query);
        serde_json::to_string(&value).map_err(|e| e.to_string())
    }
}
