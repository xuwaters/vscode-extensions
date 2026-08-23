//! wasm-bindgen surface for the FAST analyzer.
//!
//! This engine lives inside tsserver, and a Rust panic there takes every
//! TypeScript feature down with it — so containment is layered (decision
//! 0006, architecture.md §1.1), and the layering was corrected by testing
//! the real artifact:
//!
//! - **On `wasm32-unknown-unknown` a panic is a trap.** `panic = "abort"`
//!   compiles the panic path to `unreachable`; `catch_unwind` catches
//!   nothing, and the call surfaces in JS as a `RuntimeError` from the glue.
//!   The binding contract is therefore: **the plugin wraps every engine call
//!   in try/catch**, counts throws, and poisons the instance on the second —
//!   a trapped instance's memory is not trustworthy. `debugPanic()` exists
//!   so that path is tested against the real artifact, not a mock.
//! - The `catch_unwind` wrappers below still run — and actually catch — under
//!   native `cargo test`, and convert recoverable engine errors (bad JSON,
//!   unknown document) into `None` + `lastError()` on every target, so an
//!   ordinary error never becomes an exception in tsserver.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};

use wasm_bindgen::prelude::*;

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn record_error(context: &str, detail: String) {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = Some(format!("{context}: {detail}"));
    });
}

/// Run `body`, converting a panic or an engine error into `None`.
fn contained<T>(context: &str, body: impl FnOnce() -> Result<T, String>) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            record_error(context, error);
            None
        }
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("panic with a non-string payload");
            record_error(context, format!("panic: {message}"));
            None
        }
    }
}

#[wasm_bindgen(js_name = engineVersion)]
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[wasm_bindgen]
pub struct Engine {
    inner: fast_analyzer_core::Engine,
}

impl Default for Engine {
    fn default() -> Engine {
        Engine::new()
    }
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        // Panic messages still reach the console when a hook is installed —
        // the containment below is what keeps them out of tsserver's stack.
        console_error_panic_hook::set_once();
        Engine {
            inner: fast_analyzer_core::Engine::new(),
        }
    }

    /// The message behind the most recent `None`/`false`, then clears it.
    #[wasm_bindgen(js_name = lastError)]
    pub fn last_error(&self) -> Option<String> {
        LAST_ERROR.with(|slot| slot.borrow_mut().take())
    }

    #[wasm_bindgen(js_name = setConfig)]
    pub fn set_config(&mut self, json: &str) -> bool {
        contained("setConfig", || self.inner.set_config_json(json)).is_some()
    }

    #[wasm_bindgen(js_name = upsertFile)]
    pub fn upsert_file(&mut self, json: &str) -> bool {
        contained("upsertFile", || self.inner.upsert_file_json(json)).is_some()
    }

    #[wasm_bindgen(js_name = removeFile)]
    pub fn remove_file(&mut self, file_name: &str) -> bool {
        contained("removeFile", || {
            self.inner.remove_file(file_name);
            Ok(())
        })
        .is_some()
    }

    /// `AnalyzeResult` as JSON, or `None` on panic/unknown document.
    pub fn analyze(&self, document_id: &str) -> Option<String> {
        contained("analyze", || self.inner.analyze_json(document_id))
    }

    /// One entry point for every position query; the payload's `type` field
    /// selects the feature. Returns the result as JSON (`null` when the
    /// feature has nothing to say), or `None` on panic.
    pub fn query(&self, json: &str) -> Option<String> {
        contained("query", || self.inner.query_json(json))
    }

    /// Deliberately panic inside the engine — exists so the plugin's
    /// containment path can be tested against the real artifact (task P1-07).
    #[wasm_bindgen(js_name = debugPanic)]
    pub fn debug_panic(&self) -> Option<String> {
        contained("debugPanic", || -> Result<String, String> {
            panic!("deliberate panic for the containment test")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_is_contained_and_recorded() {
        let engine = Engine::new();
        assert_eq!(engine.debug_panic(), None);
        let error = engine.last_error().expect("error recorded");
        assert!(error.contains("deliberate panic"), "{error}");
        // Reading it cleared it.
        assert_eq!(engine.last_error(), None);
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        let mut engine = Engine::new();
        assert!(!engine.set_config("not json"));
        assert!(engine.last_error().unwrap().contains("setConfig"));
        assert!(!engine.upsert_file("{"));
        assert!(engine.last_error().is_some());
    }

    #[test]
    fn happy_path_round_trip() {
        let mut engine = Engine::new();
        assert!(engine.set_config(r#"{"strict": true}"#));
        assert!(engine.upsert_file(
            r#"{
                "fileName": "/t.ts",
                "components": [],
                "dependencies": [],
                "documents": [{
                    "id": "d1", "fileName": "/t.ts", "templateStart": 5,
                    "kind": "html", "text": "<div><butto>x</div>",
                    "placeholders": []
                }]
            }"#
        ));
        let analysis = engine.analyze("d1").expect("analysis");
        assert!(analysis.contains("no-unclosed-tag"), "{analysis}");
        let closing = engine
            .query(r#"{"type": "severities"}"#)
            .expect("severities");
        assert!(closing.contains("no-unclosed-tag"));
    }
}
