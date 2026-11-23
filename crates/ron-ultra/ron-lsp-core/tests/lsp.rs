//! The server through the same three entry points `ron-lsp-wasm` wraps:
//! `on_request`, `on_notification`, `drain`.

use pretty_assertions::assert_eq;
use ron_lsp_core::{FileLoader, Server, Settings};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Files on "disk", shared with the test so it can edit them mid-run.
#[derive(Clone, Default)]
struct Disk(Rc<RefCell<HashMap<String, String>>>);

impl Disk {
    fn write(&self, uri: &str, text: &str) {
        self.0.borrow_mut().insert(uri.to_string(), text.to_string());
    }
}

impl FileLoader for Disk {
    fn read(&self, uri: &str) -> Option<String> {
        self.0.borrow().get(uri).cloned()
    }
}

const RON: &str = "file:///game/assets/config.ron";

fn game_disk() -> Disk {
    let disk = Disk::default();
    disk.write("file:///game/Cargo.toml", "[package]\nname = \"game\"\n");
    disk.write("file:///game/src/lib.rs", "pub mod config;\n");
    disk.write(
        "file:///game/src/config.rs",
        "/// Settings.\npub struct GameConfig {\n    /// Frames per second.\n    pub max_fps: u8,\n    pub mode: Mode,\n}\npub enum Mode { Windowed, Fullscreen }\n",
    );
    disk
}

struct Harness {
    server: Server,
    version: i32,
}

impl Harness {
    fn new(disk: Disk) -> Harness {
        Harness { server: Server::new(Settings::default(), Box::new(disk)), version: 0 }
    }

    fn open(&mut self, text: &str) -> Vec<Value> {
        self.version += 1;
        self.server.on_notification(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": RON, "languageId": "ron", "version": self.version, "text": text } }),
        );
        self.diagnostics()
    }

    fn change(&mut self, text: &str) -> Vec<Value> {
        self.version += 1;
        self.server.on_notification(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": RON, "version": self.version }, "contentChanges": [{ "text": text }] }),
        );
        self.diagnostics()
    }

    /// The codes of the last diagnostics published for the RON file.
    fn diagnostics(&mut self) -> Vec<Value> {
        let events = self.server.drain();
        let last = events.iter().rev().find(|e| e.method == "textDocument/publishDiagnostics").expect("a publish");
        last.params["diagnostics"].as_array().unwrap().clone()
    }

    fn codes(diags: &[Value]) -> Vec<String> {
        diags.iter().map(|d| d["code"].as_str().unwrap().to_string()).collect()
    }

    fn request(&mut self, method: &str, line: u32, character: u32) -> Value {
        self.server
            .on_request(
                method,
                json!({ "textDocument": { "uri": RON }, "position": { "line": line, "character": character } }),
            )
            .expect("a response")
    }
}

const SRC: &str = "/* @[crate::config::GameConfig] */\nGameConfig(max_fps: 300, mode: Borderless)\n";

#[test]
fn schema_diagnostics_are_published_with_syntax_ones() {
    let mut h = Harness::new(game_disk());
    let diags = h.open(SRC);
    assert_eq!(Harness::codes(&diags), ["RON019", "RON016"]);
    assert_eq!(diags[0]["range"]["start"], json!({ "line": 1, "character": 20 }));
    assert_eq!(diags[0]["source"], "ron");
    assert_eq!(Harness::codes(&h.change("/* @[crate::config::GameConfig] */\nGameConfig(max_fps: 60, mode: Windowed")), ["RON001"]);
}

#[test]
fn rust_changes_reach_open_files_through_watched_file_events() {
    let disk = game_disk();
    let mut h = Harness::new(disk.clone());
    assert_eq!(Harness::codes(&h.open(SRC)), ["RON019", "RON016"]);
    disk.write(
        "file:///game/src/config.rs",
        "pub struct GameConfig { pub max_fps: u16, pub mode: Mode }\npub enum Mode { Windowed, Borderless }\n",
    );
    h.server.on_notification(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": "file:///game/src/config.rs", "type": 2 }] }),
    );
    assert_eq!(Harness::codes(&h.diagnostics()), Vec::<String>::new());
    // A file the server never read changes nothing and publishes nothing.
    h.server.on_notification(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": "file:///elsewhere/src/lib.rs", "type": 2 }] }),
    );
    assert!(h.server.drain().is_empty());
}

#[test]
fn a_missing_manifest_that_appears_is_picked_up() {
    let disk = Disk::default();
    let mut h = Harness::new(disk.clone());
    assert_eq!(Harness::codes(&h.open(SRC)), ["RON012"]);
    for (uri, text) in [
        ("file:///game/Cargo.toml", "[package]\nname = \"game\"\n"),
        ("file:///game/src/lib.rs", "pub mod config;\n"),
        ("file:///game/src/config.rs", "pub struct GameConfig { pub max_fps: u16, pub mode: u8 }\n"),
    ] {
        disk.write(uri, text);
    }
    h.server.on_notification(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": "file:///game/Cargo.toml", "type": 1 }] }),
    );
    assert_eq!(Harness::codes(&h.diagnostics()), ["RON013"]);
}

#[test]
fn hover_completion_and_definition() {
    let mut h = Harness::new(game_disk());
    h.open("/* @[crate::config::GameConfig] */\nGameConfig(max_fps: 60, mode: )\n");

    let hover = h.request("textDocument/hover", 1, 12);
    assert_eq!(hover["contents"]["kind"], "markdown");
    let text = hover["contents"]["value"].as_str().unwrap();
    assert!(text.starts_with("```rust\nGameConfig.max_fps: u8\n```\n\nFrames per second."), "{text}");

    let items = h.request("textDocument/completion", 1, 30);
    let labels: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Windowed", "Fullscreen"]);

    let def = h.request("textDocument/definition", 1, 12);
    assert_eq!(def["uri"], "file:///game/src/config.rs");
    assert_eq!(def["range"]["start"], json!({ "line": 3, "character": 8 }));
}

#[test]
fn field_completion_inserts_the_key_and_asks_again() {
    let mut h = Harness::new(game_disk());
    h.open("/* @[crate::config::GameConfig] */\nGameConfig(\n    \n)\n");
    let items = h.request("textDocument/completion", 2, 4);
    let first = &items[0];
    assert_eq!(first["label"], "max_fps");
    assert_eq!(first["insertText"], "max_fps: ");
    assert_eq!(first["detail"], "u8");
    assert_eq!(first["command"]["command"], "editor.action.triggerSuggest");
}

#[test]
fn structure_and_formatting() {
    let mut h = Harness::new(Disk::default());
    h.open("Config(size:(800,600),\n  name:\"x\")");
    let symbols = h.server.on_request("textDocument/documentSymbol", json!({ "textDocument": { "uri": RON } })).unwrap();
    assert_eq!(symbols[0]["name"], "Config");
    assert_eq!(symbols[0]["children"][0]["name"], "size");

    let edits = h
        .server
        .on_request(
            "textDocument/formatting",
            json!({ "textDocument": { "uri": RON }, "options": { "tabSize": 4, "insertSpaces": true, "insertFinalNewline": true } }),
        )
        .unwrap();
    assert_eq!(edits[0]["newText"], "Config(size: (800, 600), name: \"x\")\n");

    let ranges = h
        .server
        .on_request(
            "textDocument/selectionRange",
            json!({ "textDocument": { "uri": RON }, "positions": [{ "line": 0, "character": 14 }] }),
        )
        .unwrap();
    assert_eq!(ranges[0]["range"]["start"], json!({ "line": 0, "character": 13 }));
    assert!(ranges[0]["parent"].is_object());
}

#[test]
fn settings_switch_features_off() {
    let mut h = Harness::new(game_disk());
    h.open(SRC);
    h.server.on_notification(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "schema": { "enabled": false } } }),
    );
    assert_eq!(Harness::codes(&h.diagnostics()), Vec::<String>::new());
    h.server.on_notification(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "diagnostics": { "enabled": false } } }),
    );
    assert!(h.diagnostics().is_empty());
}

#[test]
fn unknown_methods_and_documents() {
    let mut h = Harness::new(Disk::default());
    assert_eq!(h.server.on_request("nope", Value::Null).unwrap_err().code, -32601);
    assert_eq!(h.request("textDocument/hover", 0, 0), Value::Null);
    let caps = serde_json::to_value(ron_lsp_core::capabilities()).unwrap();
    assert_eq!(caps["textDocumentSync"], 1);
}

/// The real filesystem, for `file://` URIs without percent-escapes.
struct Fs;

impl FileLoader for Fs {
    fn read(&self, uri: &str) -> Option<String> {
        std::fs::read_to_string(uri.strip_prefix("file://")?).ok()
    }
}

#[test]
fn the_shipped_example_is_clean() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../extensions/ron-ultra/examples/game");
    let file = dir.canonicalize().expect("the example exists").join("assets/game-config.ron");
    let uri = format!("file://{}", file.display());
    let mut server = Server::new(Settings::default(), Box::new(Fs));
    server.on_notification(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "ron", "version": 1, "text": std::fs::read_to_string(&file).unwrap() } }),
    );
    let events = server.drain();
    assert_eq!(events[0].params["diagnostics"], json!([]));
    // And it really is checked: the annotation resolved.
    assert!(server.document(&uri).unwrap().schema.target.is_some());
    // And already formatted.
    let edits = server
        .on_request(
            "textDocument/formatting",
            json!({ "textDocument": { "uri": uri }, "options": { "tabSize": 4, "insertSpaces": true, "insertFinalNewline": true } }),
        )
        .unwrap();
    assert_eq!(edits, Value::Null);
}
