//! The `#[wasm_bindgen]` surface.

use js_sys::{Function, Reflect};
use ron_lsp_core::{FileLoader, Server, Settings};
use serde_json::Value;
use wasm_bindgen::prelude::*;

/// `host.readFile(uri)`: the file's text, or `null` when there is none.
struct JsLoader {
    read_file: Function,
}

impl FileLoader for JsLoader {
    fn read(&self, uri: &str) -> Option<String> {
        self.read_file.call1(&JsValue::NULL, &JsValue::from_str(uri)).ok()?.as_string()
    }
}

/// The language server, as JS sees it.
#[wasm_bindgen]
pub struct RonServer {
    inner: Server,
}

#[wasm_bindgen]
impl RonServer {
    /// `host` carries `readFile(uri: string): string | null`; `init` is
    /// `{ settings }` with the `ronUltra.*` settings.
    #[wasm_bindgen(constructor)]
    pub fn new(host: JsValue, init: JsValue) -> Result<RonServer, JsValue> {
        console_error_panic_hook::set_once();
        let read_file: Function = Reflect::get(&host, &JsValue::from_str("readFile"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("host services are missing `readFile`"))?;
        let init = from_js(init)?;
        // Unreadable settings are not worth refusing to start over: the
        // defaults are documented and a `didChangeConfiguration` fixes them.
        let settings: Settings = serde_json::from_value(init["settings"].clone()).unwrap_or_default();
        Ok(RonServer { inner: Server::new(settings, Box::new(JsLoader { read_file })) })
    }

    /// The capabilities to answer `initialize` with.
    pub fn capabilities(&self) -> Result<JsValue, JsValue> {
        to_js(&ron_lsp_core::capabilities())
    }

    /// The request methods the server answers, for the host to register.
    pub fn requests() -> Result<JsValue, JsValue> {
        to_js(&ron_lsp_core::REQUESTS)
    }

    /// The notification methods the server handles.
    pub fn notifications() -> Result<JsValue, JsValue> {
        to_js(&ron_lsp_core::NOTIFICATIONS)
    }

    #[wasm_bindgen(js_name = onRequest)]
    pub fn on_request(&mut self, method: &str, params: JsValue) -> Result<JsValue, JsValue> {
        match self.inner.on_request(method, from_js(params)?) {
            Ok(value) => to_js(&value),
            Err(error) => Err(to_js(&error)?),
        }
    }

    #[wasm_bindgen(js_name = onNotification)]
    pub fn on_notification(&mut self, method: &str, params: JsValue) -> Result<(), JsValue> {
        self.inner.on_notification(method, from_js(params)?);
        Ok(())
    }

    /// Notifications produced by the calls above, drained after each
    /// response is written so Rust never re-enters JS mid-handler.
    #[wasm_bindgen(js_name = drainEvents)]
    pub fn drain_events(&mut self) -> Result<JsValue, JsValue> {
        to_js(&self.inner.drain())
    }
}

/// Through JSON, so a payload that works over the wire works here.
fn from_js(value: JsValue) -> Result<Value, JsValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(Value::Null);
    }
    let text: String = js_sys::JSON::stringify(&value)
        .map_err(|_| JsValue::from_str("params are not JSON-serializable"))?
        .into();
    serde_json::from_str(&text).map_err(|err| JsValue::from_str(&format!("bad params: {err}")))
}

fn to_js<T: serde::Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    let text = serde_json::to_string(value).map_err(|err| JsValue::from_str(&format!("unserializable result: {err}")))?;
    js_sys::JSON::parse(&text)
}
