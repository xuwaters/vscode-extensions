//! WASM entry points. JSON in, JSON out, matching the repo's other WASM
//! surfaces: the TypeScript host treats the module as a black box.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

/// A per-document render session; retains previous block hashes for diffing.
#[wasm_bindgen]
pub struct Session {
    inner: crate::Session,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session {
        Session {
            inner: crate::Session::new(),
        }
    }

    /// `options_json`: a `RenderOptions` object (see lib.rs). Returns a
    /// `RenderResult` as JSON. Changing options forces `reset: true`.
    pub fn render(&mut self, markdown: &str, options_json: &str) -> String {
        self.inner.render_json(markdown, options_json)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
