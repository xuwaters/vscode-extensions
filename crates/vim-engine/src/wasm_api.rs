//! WASM entry points. JSON in, JSON out — except edits, which cross as one
//! binary block: a `:%s` over a big file produces ~1 MB of them, and JSON
//! costs several milliseconds escaping on this side and parsing on the other.
//! The effects JSON carries an `editCount` instead; when it is non-zero the
//! host calls `take_edits` for the block (see its layout there).

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::buffer::{Pos, utf16_len};
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

/// One editor selection, as the host sends the multi-cursor set.
#[derive(Deserialize)]
struct SelectionIn {
    #[serde(rename = "anchorLine")]
    anchor_line: usize,
    #[serde(rename = "anchorCol")]
    anchor_col: usize,
    #[serde(rename = "activeLine")]
    active_line: usize,
    #[serde(rename = "activeCol")]
    active_col: usize,
}

/// `Effects` as the host receives it: the effects fields (minus edits, which
/// never serialize) plus how many edits `take_edits` is holding.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EffectsMsg<'a> {
    #[serde(flatten)]
    fx: &'a crate::Effects,
    edit_count: usize,
}

/// A per-document editing session; holds modal state and the text mirror.
#[wasm_bindgen]
pub struct Session {
    inner: crate::Session,
    /// Edits of the last effects, awaiting `take_edits`.
    edits: Vec<crate::Edit>,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str, line: usize, col: usize) -> Session {
        let mut inner = crate::Session::new(text);
        inner.reset(text, line, col);
        Session { inner, edits: Vec::new() }
    }

    /// Replace the mirror wholesale (document reopened or desync recovery).
    pub fn reset(&mut self, text: &str, line: usize, col: usize) {
        self.inner.reset(text, line, col);
        self.edits.clear();
    }

    /// Feed one key; returns `Effects` as JSON.
    pub fn key(&mut self, key: &str) -> String {
        match Key::parse(key) {
            Some(k) => {
                let fx = self.inner.key(k);
                self.effects_json(fx)
            }
            None => "null".to_string(),
        }
    }

    /// The edits of the last effects as one binary block, drained: a u32
    /// count, then per edit `[startLine, startCol, endLine, endCol, textLen]`
    /// as little-endian u32s, then every edit's text as one UTF-8 blob.
    /// Columns and `textLen` are UTF-16 units, so the host decodes the blob
    /// once and slices the string per edit.
    pub fn take_edits(&mut self) -> Vec<u8> {
        let edits = std::mem::take(&mut self.edits);
        let text_bytes: usize = edits.iter().map(|e| e.text.len()).sum();
        let mut out = Vec::with_capacity(4 + edits.len() * 20 + text_bytes);
        out.extend_from_slice(&(edits.len() as u32).to_le_bytes());
        for e in &edits {
            let header = [e.start.line, e.start.col, e.end.line, e.end.col, utf16_len(&e.text)];
            for v in header {
                out.extend_from_slice(&(v as u32).to_le_bytes());
            }
        }
        for e in &edits {
            out.extend_from_slice(e.text.as_bytes());
        }
        out
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
        let fx = self.inner.set_position(line, col);
        self.effects_json(fx)
    }

    /// A non-empty selection was made outside the engine. `by_hand`: the user
    /// drew it (pointer drag, shift+arrow) rather than a command leaving it
    /// behind (`cmd+f`, a completion's placeholder).
    pub fn set_selection(
        &mut self,
        anchor_line: usize,
        anchor_col: usize,
        active_line: usize,
        active_col: usize,
        by_hand: bool,
    ) -> String {
        let fx = self.inner.set_selection(
            Pos::new(anchor_line, anchor_col),
            Pos::new(active_line, active_col),
            by_hand,
        );
        self.effects_json(fx)
    }

    /// The editor's whole selection set, primary first: a JSON array of
    /// `{anchorLine, anchorCol, activeLine, activeCol}`. Used when the editor
    /// has more than one cursor; one selection behaves like `set_position` /
    /// `set_selection`, `by_hand` and all.
    pub fn set_cursors(&mut self, selections_json: &str, by_hand: bool) -> String {
        let Ok(sels) = serde_json::from_str::<Vec<SelectionIn>>(selections_json) else {
            return "null".to_string();
        };
        let sels: Vec<(Pos, Pos)> = sels
            .iter()
            .map(|s| {
                (
                    Pos::new(s.anchor_line, s.anchor_col),
                    Pos::new(s.active_line, s.active_col),
                )
            })
            .collect();
        let fx = self.inner.set_cursors(&sels, by_hand);
        self.effects_json(fx)
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

impl Session {
    /// Stash the edits for `take_edits` and serialize the rest.
    fn effects_json(&mut self, mut fx: crate::Effects) -> String {
        self.edits = std::mem::take(&mut fx.edits);
        let msg = EffectsMsg { fx: &fx, edit_count: self.edits.len() };
        serde_json::to_string(&msg).unwrap_or_else(|_| "null".to_string())
    }
}
