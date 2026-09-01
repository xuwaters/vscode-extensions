//! A WGSL and GLSL language server, minus a transport.
//!
//! [`Server`] takes a method name and a `serde_json::Value` and returns a
//! `Value` plus a queue of outbound notifications. `wgsl-lsp-wasm` wraps
//! exactly those three methods and nothing else; the test harness in
//! `tests/support` wraps the same three. That is how the whole feature set is
//! tested without a WASM toolchain.
//!
//! One analyzer per language, and which one a feature reaches for is the
//! central design decision (see [`state::Document`]):
//!
//! - **WGSL.** [`wgsl_syntax`] parses anything, always, and knows about names
//!   and scopes; [`analysis`] runs naga, which knows about *types* but only for
//!   sources it can parse in full. Features prefer naga and fall back to
//!   syntax, which is what keeps hover, completion and go-to-definition working
//!   mid-keystroke.
//! - **GLSL.** [`glsl`] preprocesses, parses and analyses with this
//!   workspace's own `glsl-*` crates — every dialect, not just the Vulkan one
//!   naga implements (RFC 012). It answers types and diagnostics *and* survives
//!   a half-typed line, so there is no fallback to arrange.

pub mod analysis;
pub mod capabilities;
pub mod convert;
pub mod dispatch;
pub mod features;
pub mod glsl;
pub mod index;
pub mod settings;
pub mod state;

use std::collections::{HashMap, HashSet};

use lsp_types::Uri;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wgsl_syntax::Language;

pub use capabilities::capabilities;
pub use dispatch::{Outbound, ResponseError};
pub use index::WorkspaceIndex;
pub use settings::Settings;
pub use state::{Document, TokenCache};

/// What the host tells the server at startup, beyond standard LSP.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerConfig {
    /// The `wgsl.*` and `glsl.*` settings.
    pub settings: Settings,
}

/// The language server.
pub struct Server {
    settings: Settings,
    /// Open documents, keyed by URI string. A `Uri` is not `Hash` in
    /// `lsp-types` 0.97, and the string form is what the wire carries anyway.
    documents: HashMap<String, Document>,
    /// Symbols from files the host has told us about but the editor has not
    /// opened, for `workspace/symbol`.
    index: WorkspaceIndex,
    /// URIs that currently carry diagnostics. A file that had them and now has
    /// none must be published as an empty array or the squiggles persist, so
    /// the difference between publishes is what gets cleared.
    published: HashSet<String>,
    /// Notifications produced while handling a request, drained afterwards so
    /// the server never re-enters the host mid-handler.
    outbox: Vec<Outbound>,
}

impl Server {
    pub fn new(config: ServerConfig) -> Server {
        Server {
            settings: config.settings,
            documents: HashMap::new(),
            index: WorkspaceIndex::default(),
            published: HashSet::new(),
            outbox: Vec::new(),
        }
    }

    /// Handle a request and produce its result.
    pub fn on_request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        dispatch::request(self, method, params)
    }

    /// Handle a notification.
    pub fn on_notification(&mut self, method: &str, params: Value) {
        dispatch::notification(self, method, params);
    }

    /// Take the notifications produced since the last drain.
    pub fn drain(&mut self) -> Vec<Outbound> {
        std::mem::take(&mut self.outbox)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Apply new settings, returning whether diagnostics need republishing.
    pub fn set_settings(&mut self, settings: Settings) -> bool {
        let mut changed = settings.wgsl.validate != self.settings.wgsl.validate
            || settings.glsl.validate != self.settings.glsl.validate;
        self.settings = settings;

        // Unlike the triggers beside them, the assumed `#version` is read
        // where the analysis is *made*, so each open document holds its own
        // copy and has to be told.
        let default_version =
            glsl::parse_version(&self.settings.glsl.default_version);
        for document in self.documents.values_mut() {
            if document.language == Language::Glsl
                && document.set_default_version(default_version)
            {
                changed = true;
            }
        }
        changed
    }

    /// Queue a notification for the host.
    pub fn notify(&mut self, method: impl Into<String>, params: Value) {
        self.outbox.push(Outbound { method: method.into(), params });
    }

    pub fn document(&self, uri: &Uri) -> Option<&Document> {
        self.documents.get(uri.as_str())
    }

    pub fn document_mut(&mut self, uri: &Uri) -> Option<&mut Document> {
        self.documents.get_mut(uri.as_str())
    }

    pub fn documents(&self) -> impl Iterator<Item = &Document> {
        self.documents.values()
    }

    pub fn index(&self) -> &WorkspaceIndex {
        &self.index
    }

    pub fn index_mut(&mut self) -> &mut WorkspaceIndex {
        &mut self.index
    }

    pub(crate) fn insert_document(&mut self, document: Document) {
        self.documents.insert(document.uri.as_str().to_string(), document);
    }

    pub(crate) fn remove_document(&mut self, uri: &Uri) -> Option<Document> {
        self.documents.remove(uri.as_str())
    }

    pub(crate) fn published(&mut self) -> &mut HashSet<String> {
        &mut self.published
    }

    /// The document at `uri` plus the byte offset of `position` within it.
    pub(crate) fn locate(
        &self,
        uri: &Uri,
        position: lsp_types::Position,
    ) -> Option<(&Document, u32)> {
        let document = self.document(uri)?;
        Some((document, document.offset(position)))
    }
}

/// The language a VS Code language id names, for the document sync handlers.
pub fn language_of(language_id: &str) -> Option<Language> {
    Language::from_id(language_id)
}

/// The naga version the server validates WGSL against, for the client's
/// status bar.
pub const NAGA_VERSION: &str = "30.0.1";

/// The GLSL analyzer's version — this workspace's own, since it is this
/// workspace that answers for GLSL (RFC 012).
pub const GLSL_ANALYZER_VERSION: &str = env!("CARGO_PKG_VERSION");
