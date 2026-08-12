//! WASM entry points. JSON in, JSON out, matching the repo's other WASM
//! surfaces: the TypeScript host treats the module as a black box.

use serde::Deserialize;
use wasm_bindgen::prelude::*;

use crate::buffer::Pos;
use crate::keys::Key;

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

#[derive(Deserialize)]
struct ChangeIn {
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startCol")]
    start_col: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    #[serde(rename = "endCol")]
    end_col: usize,
    text: String,
}

/// A per-document editing session; holds modal state and the text mirror.
#[wasm_bindgen]
pub struct Session {
    inner: crate::Session,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str, line: usize, col: usize) -> Session {
        let mut inner = crate::Session::new(text);
        inner.reset(text, line, col);
        Session { inner }
    }

    /// Replace the mirror wholesale (document reopened or desync recovery).
    pub fn reset(&mut self, text: &str, line: usize, col: usize) {
        self.inner.reset(text, line, col);
    }

    /// Feed one key; returns `Effects` as JSON.
    pub fn key(&mut self, key: &str) -> String {
        match Key::parse(key) {
            Some(k) => to_json(&self.inner.key(k)),
            None => "null".to_string(),
        }
    }

    /// Mirror external document changes: a JSON array of
    /// `{startLine, startCol, endLine, endCol, text}` in pre-state
    /// coordinates, ordered as VSCode reports them (last-to-first).
    pub fn apply_changes(&mut self, changes_json: &str) {
        let Ok(changes) = serde_json::from_str::<Vec<ChangeIn>>(changes_json) else {
            return;
        };
        for ch in changes {
            self.inner.apply_change(
                Pos::new(ch.start_line, ch.start_col),
                Pos::new(ch.end_line, ch.end_col),
                &ch.text,
            );
        }
    }

    /// The editor cursor moved outside the engine; returns `Effects` JSON
    /// (possibly with a clamped-position correction).
    pub fn set_position(&mut self, line: usize, col: usize) -> String {
        to_json(&self.inner.set_position(line, col))
    }

    /// A non-empty selection was made outside the engine (mouse drag).
    pub fn set_selection(
        &mut self,
        anchor_line: usize,
        anchor_col: usize,
        active_line: usize,
        active_col: usize,
    ) -> String {
        to_json(&self.inner.set_selection(
            Pos::new(anchor_line, anchor_col),
            Pos::new(active_line, active_col),
        ))
    }

    /// Current mode label ("normal" | "insert" | "visual" | "visualLine").
    pub fn mode(&self) -> String {
        self.inner.mode_label().to_string()
    }

    /// Mirrored text, for host-side desync checks.
    pub fn text(&self) -> String {
        self.inner.text()
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}
