//! The `#[wasm_bindgen]` surface.
//!
//! Four entry points and nothing else, deliberately. Everything interesting
//! happens in `wgsl-lsp-core`, which knows nothing about WASM and is tested
//! natively.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::prelude::*;
use wgsl_lsp_core::{Server, ServerConfig, capabilities};

/// What the host passes at construction.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InitOptions {
    /// The `wgsl.*` and `glsl.*` settings, as one object.
    pub settings: Value,
}

/// The language server, as JS sees it.
#[wasm_bindgen]
pub struct ShaderServer {
    inner: Server,
}

#[wasm_bindgen]
impl ShaderServer {
    /// Build a server. `init` is an [`InitOptions`] object.
    #[wasm_bindgen(constructor)]
    pub fn new(init: JsValue) -> Result<ShaderServer, JsValue> {
        console_error_panic_hook::set_once();

        let options: InitOptions = serde_json::from_value(from_js(init)?)
            .map_err(|err| JsValue::from_str(&format!("bad initialization options: {err}")))?;
        // Unreadable settings are not worth refusing to start over: the
        // defaults are documented and every one of them is recoverable with a
        // `didChangeConfiguration`.
        let settings = serde_json::from_value(options.settings).unwrap_or_default();

        Ok(ShaderServer { inner: Server::new(ServerConfig { settings }) })
    }

    /// The capabilities to answer `initialize` with, as JSON.
    #[wasm_bindgen(js_name = capabilities)]
    pub fn capabilities(&self) -> Result<JsValue, JsValue> {
        to_js(&capabilities(self.inner.settings()))
    }

    /// Handle a request. Returns the LSP result value.
    #[wasm_bindgen(js_name = onRequest)]
    pub fn on_request(&mut self, method: &str, params: JsValue) -> Result<JsValue, JsValue> {
        let params = from_js(params)?;
        match self.inner.on_request(method, params) {
            Ok(value) => to_js(&value),
            Err(error) => Err(to_js(&error)?),
        }
    }

    /// Handle a notification.
    #[wasm_bindgen(js_name = onNotification)]
    pub fn on_notification(&mut self, method: &str, params: JsValue) -> Result<(), JsValue> {
        let params = from_js(params)?;
        self.inner.on_notification(method, params);
        Ok(())
    }

    /// Notifications produced while handling the above.
    ///
    /// Drained by the JS loop *after* the response is written, so Rust never
    /// re-enters JS mid-handler.
    #[wasm_bindgen(js_name = drainEvents)]
    pub fn drain_events(&mut self) -> Result<JsValue, JsValue> {
        to_js(&self.inner.drain())
    }

    /// The WASM linear memory currently allocated, in bytes.
    ///
    /// Linear memory is never returned to the OS, so restarting the child
    /// process is the only real reclamation mechanism — the host watches this
    /// to decide when that is worth doing.
    #[wasm_bindgen(js_name = heapBytes)]
    pub fn heap_bytes() -> f64 {
        (core::arch::wasm32::memory_size(0) * 65536) as f64
    }

    /// The naga version this build validates with.
    #[wasm_bindgen(js_name = nagaVersion)]
    pub fn naga_version() -> String {
        crate::NAGA_VERSION.to_string()
    }
}

/// JSON in, `serde_json::Value` out.
///
/// Going through JSON rather than `serde-wasm-bindgen` keeps the boundary
/// identical to what the JSON-RPC layer already produces, so a payload that
/// works over the wire works here.
fn from_js(value: JsValue) -> Result<Value, JsValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(Value::Null);
    }
    let text = js_sys::JSON::stringify(&value)
        .map_err(|_| JsValue::from_str("params are not JSON-serializable"))?;
    let text: String = text.into();
    serde_json::from_str(&text)
        .map_err(|err| JsValue::from_str(&format!("bad params: {err}")))
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let text = serde_json::to_string(value)
        .map_err(|err| JsValue::from_str(&format!("unserializable result: {err}")))?;
    js_sys::JSON::parse(&text)
}
