//! A RON language server, minus a transport.
//!
//! [`Server`] takes a method name and a `serde_json::Value` and returns a
//! `Value` plus a queue of outbound notifications. `ron-lsp-wasm` wraps
//! exactly those three methods; the tests in `tests/` wrap the same three,
//! which is how the whole feature set is tested without a WASM toolchain.
//!
//! Unlike most language servers in this repository, this one reads files it
//! was not sent: a RON file annotated with `@[crate::config::GameConfig]` is
//! checked against that Rust struct, so the server reads `Cargo.toml` and
//! `.rs` files through a host-provided [`FileLoader`] — synchronously, on
//! demand, and cached until `workspace/didChangeWatchedFiles` says a file
//! changed.

pub mod convert;
pub mod dispatch;
pub mod handlers;

use lsp_types::{
    CompletionOptions, FoldingRangeProviderCapability, HoverProviderCapability, OneOf,
    SelectionRangeProviderCapability, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
};
use ron_analyzer::rust::world::World;
use ron_analyzer::schema::Schema;
use ron_analyzer::workspace::ParsedFile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use dispatch::{Outbound, ResponseError, NOTIFICATIONS, REQUESTS};
pub use ron_analyzer::rust::world::{FileLoader, NoFiles};

/// The `ronUltra.*` settings, as the client sends them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub diagnostics: Toggle,
    pub format: FormatSettings,
    /// Checking against the `@[...]`-annotated Rust type.
    pub schema: Toggle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Toggle {
    pub enabled: bool,
}

impl Default for Toggle {
    fn default() -> Self {
        Toggle { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormatSettings {
    pub enabled: bool,
    pub print_width: u32,
}

impl Default for FormatSettings {
    fn default() -> Self {
        FormatSettings { enabled: true, print_width: 80 }
    }
}

/// An open RON document and what the analyzer made of it.
pub struct Document {
    pub version: i32,
    pub parsed: ParsedFile,
    pub schema: Schema,
}

/// The language server.
pub struct Server {
    settings: Settings,
    /// Open documents, keyed by URI string.
    documents: HashMap<String, Document>,
    /// The Rust sources and manifests read so far.
    world: World,
    /// Notifications produced while handling a call, drained afterwards so
    /// the server never re-enters the host mid-handler.
    outbox: Vec<Outbound>,
}

impl Server {
    pub fn new(settings: Settings, loader: Box<dyn FileLoader>) -> Server {
        Server { settings, documents: HashMap::new(), world: World::new(loader), outbox: Vec::new() }
    }

    pub fn on_request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        dispatch::request(self, method, params)
    }

    pub fn on_notification(&mut self, method: &str, params: serde_json::Value) {
        dispatch::notification(self, method, params);
    }

    /// Take the notifications produced since the last drain.
    pub fn drain(&mut self) -> Vec<Outbound> {
        std::mem::take(&mut self.outbox)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn document(&self, uri: &str) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub(crate) fn notify(&mut self, method: impl Into<String>, params: serde_json::Value) {
        self.outbox.push(Outbound { method: method.into(), params });
    }
}

/// What the server tells the client it can do. Formatting is always
/// advertised and declined per request when switched off, so the setting
/// needs no restart.
pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            // `(`, `[`, `{` and `,` open an entry; `:` starts a value, and
            // the second `:` of `crate::` a path segment.
            trigger_characters: Some(["(", "[", "{", ",", ":"].iter().map(|c| c.to_string()).collect()),
            resolve_provider: Some(false),
            ..CompletionOptions::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    }
}
