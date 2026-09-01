//! A native harness for the language server.
//!
//! Wraps exactly the three methods `wgsl-lsp-wasm` wraps — `on_request`,
//! `on_notification`, `drain` — so the whole feature set is exercised with no
//! WASM toolchain present, and so a test failing here means the feature is
//! broken rather than the binding.
//!
//! Fixtures mark the cursor with `|`, which is stripped before the document is
//! opened.

#![allow(dead_code)]

use lsp_types::{Position, Range, Uri};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wgsl_lsp_core::{Outbound, Server, ServerConfig, Settings};
use wgsl_syntax::Language;

/// The marker fixtures use to say where the cursor is.
pub const CURSOR: &str = "|";

pub struct Harness {
    server: Server,
    version: i32,
    /// Notifications the server has produced and nothing has read yet.
    events: Vec<Outbound>,
}

impl Harness {
    pub fn new() -> Harness {
        Harness::with_settings(Settings::default())
    }

    pub fn with_settings(settings: Settings) -> Harness {
        Harness {
            server: Server::new(ServerConfig { settings }),
            version: 1,
            events: Vec::new(),
        }
    }

    /// Open a document. The language comes from the file extension, as it does
    /// in the editor.
    pub fn open(&mut self, name: &str, text: &str) -> Uri {
        let uri = uri_for(name);
        let language = language_for(name);
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri.as_str(),
                    "languageId": language.id(),
                    "version": self.version,
                    "text": text,
                }
            }),
        );
        uri
    }

    /// Open a fixture carrying a `|` cursor marker, which is removed first.
    pub fn open_at(&mut self, name: &str, text: &str) -> (Uri, Position) {
        let offset = text.find(CURSOR).expect("fixture has no `|` cursor marker");
        let stripped = text.replacen(CURSOR, "", 1);
        let position = position_of(&stripped, offset);
        (self.open(name, &stripped), position)
    }

    /// Replace a document's whole contents, as a `didChange` would.
    pub fn change(&mut self, uri: &Uri, text: &str) {
        self.version += 1;
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri.as_str(), "version": self.version },
                "contentChanges": [{ "text": text }],
            }),
        );
    }

    /// Apply one incremental change.
    pub fn edit(&mut self, uri: &Uri, range: Range, text: &str) {
        self.version += 1;
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri.as_str(), "version": self.version },
                "contentChanges": [{ "range": range, "text": text }],
            }),
        );
    }

    pub fn save(&mut self, uri: &Uri) {
        self.notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        );
    }

    pub fn close(&mut self, uri: &Uri) {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        );
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.server.on_notification(method, params);
        self.events.extend(self.server.drain());
    }

    /// Send a request and deserialize its result.
    pub fn request<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> Option<T> {
        let value = self
            .server
            .on_request(method, params)
            .unwrap_or_else(|error| panic!("{method}: {}", error.message));
        self.events.extend(self.server.drain());
        if value.is_null() {
            return None;
        }
        Some(serde_json::from_value(value).expect("result deserializes"))
    }

    /// A request at a cursor position, for the many that take one.
    pub fn at<T: DeserializeOwned>(
        &mut self,
        method: &str,
        uri: &Uri,
        position: Position,
    ) -> Option<T> {
        self.request(
            method,
            json!({
                "textDocument": { "uri": uri.as_str() },
                "position": position,
            }),
        )
    }

    /// The raw error a request produced, for the cases that should fail.
    pub fn error(&mut self, method: &str, params: Value) -> Option<String> {
        self.server.on_request(method, params).err().map(|error| error.message)
    }

    /// Every notification the server has produced so far.
    pub fn events(&self) -> &[Outbound] {
        &self.events
    }

    /// The most recent `publishDiagnostics` for a URI, if there was one.
    pub fn diagnostics(&self, uri: &Uri) -> Option<Vec<lsp_types::Diagnostic>> {
        self.events
            .iter()
            .rev()
            .find(|event| {
                event.method == "textDocument/publishDiagnostics"
                    && event.params["uri"] == json!(uri.as_str())
            })
            .map(|event| {
                serde_json::from_value(event.params["diagnostics"].clone())
                    .expect("diagnostics deserialize")
            })
    }

    /// Tell the server about files the editor has not opened.
    pub fn workspace_files(&mut self, files: &[(&str, &str)]) {
        let files: Vec<Value> = files
            .iter()
            .map(|(name, text)| {
                json!({
                    "uri": uri_for(name).as_str(),
                    "languageId": language_for(name).id(),
                    "text": text,
                })
            })
            .collect();
        self.notify("wgsl/workspaceFiles", json!({ "files": files, "replace": true }));
    }

    pub fn configure(&mut self, settings: Value) {
        self.notify("workspace/didChangeConfiguration", json!({ "settings": settings }));
    }

    pub fn server(&self) -> &Server {
        &self.server
    }
}

impl Default for Harness {
    fn default() -> Self {
        Harness::new()
    }
}

pub fn uri_for(name: &str) -> Uri {
    format!("file:///shaders/{name}").parse().expect("a valid test URI")
}

fn language_for(name: &str) -> Language {
    let extension = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    Language::from_extension(extension)
        .unwrap_or_else(|| panic!("no language for {name}"))
}

/// The LSP position of a byte offset in `text`.
pub fn position_of(text: &str, offset: usize) -> Position {
    let before = &text[..offset];
    let line = before.matches('\n').count() as u32;
    let column = before.rsplit('\n').next().unwrap_or("");
    Position {
        line,
        character: column.chars().map(|c| c.len_utf16() as u32).sum(),
    }
}

/// The position of the first occurrence of `needle`, offset by `within`
/// characters into it.
pub fn find(text: &str, needle: &str, within: usize) -> Position {
    let offset = text.find(needle).unwrap_or_else(|| panic!("no {needle:?} in fixture"));
    position_of(text, offset + within)
}
