//! The `#[wasm_bindgen]` surface.
//!
//! Three entry points and a callback bag, deliberately. Everything interesting
//! happens in `typst-lsp-core`, which knows nothing about WASM and is tested
//! natively.

use ecow::EcoString;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
use typst::text::FontInfo;
use typst_lsp_core::{Ports, Server, ServerConfig, capabilities};
use typst_session::ports::FaceDescriptor;
use typst_session::{Session, SessionWorld};
use wasm_bindgen::prelude::*;

use crate::host::{HostServices, JsClock, JsFiles, JsFonts, JsPackages};

/// The port bundle backed by JS callbacks.
pub struct JsPorts;

impl Ports for JsPorts {
    type Files = JsFiles;
    type Fonts = JsFonts;
    type Packages = JsPackages;
    type Clock = JsClock;
}

/// One font face as the host's on-disk index stores it.
///
/// `FontInfo` is `Serialize`/`Deserialize` upstream, so the index round-trips
/// exactly and the expensive scan happens once per machine rather than once per
/// session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceEntry {
    /// Family, variant, and coverage.
    pub info: FontInfo,
    /// The face's index within its container file.
    #[serde(default)]
    pub index: u32,
}

/// What the host passes at construction.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InitOptions {
    /// The compile root, as a `file:` URI.
    pub root_uri: String,
    /// The package cache directory, as a `file:` URI.
    pub package_cache_uri: Option<String>,
    /// The initial entry file, project-relative.
    pub main_path: String,
    /// The `typstUltra.*` settings.
    pub settings: Value,
    /// Every font face the host knows about.
    pub font_faces: Vec<FaceEntry>,
    /// The Universe package index, for package-name completions.
    pub packages: Vec<PackageIndexEntry>,
}

/// One entry of the Universe index.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageIndexEntry {
    /// `@preview/cetz:0.4.2`.
    pub spec: String,
    /// A one-line description, if the index has one.
    #[serde(default)]
    pub description: Option<String>,
}

/// The language server, as JS sees it.
#[wasm_bindgen]
pub struct TypstServer {
    inner: Server<JsPorts>,
    /// Kept so the font book can be rebuilt in place when the host finishes
    /// indexing system fonts, without tearing down the session.
    services: HostServices,
}

#[wasm_bindgen]
impl TypstServer {
    /// Build a server.
    ///
    /// `host` is a plain object carrying the synchronous callbacks; `init` is
    /// an [`InitOptions`] object.
    #[wasm_bindgen(constructor)]
    pub fn new(host: &JsValue, init: JsValue) -> Result<TypstServer, JsValue> {
        console_error_panic_hook::set_once();

        let options: InitOptions = serde_wasm_bindgen_compat(init)?;
        let services = HostServices::from_js(host)?;

        let faces = descriptors_of(&options.font_faces);

        let index: Vec<(PackageSpec, Option<EcoString>)> = options
            .packages
            .iter()
            .filter_map(|entry| {
                let spec = entry.spec.parse::<PackageSpec>().ok()?;
                Some((spec, entry.description.as_deref().map(EcoString::from)))
            })
            .collect();

        let main_path = if options.main_path.is_empty() {
            "main.typ".to_string()
        } else {
            options.main_path.clone()
        };
        let main = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new(&main_path)
                .map_err(|err| JsValue::from_str(&format!("bad main path: {err}")))?,
        ));

        let world = SessionWorld::new(
            JsFiles::new(&services),
            JsFonts::new(&services, faces),
            JsPackages::new(&services, index),
            JsClock::new(&services),
            main,
        );

        let settings = serde_json::from_value(options.settings.clone()).unwrap_or_default();
        let session = Session::new(world, 1);

        Ok(TypstServer {
            inner: Server::new(
                session,
                ServerConfig {
                    root_uri: options.root_uri,
                    package_cache_uri: options.package_cache_uri,
                    settings,
                },
            ),
            services,
        })
    }

    /// Replace the font book after the host finishes indexing.
    ///
    /// Rebuilt in place rather than by restarting the server: the session holds
    /// open documents the editor would otherwise have to resend, and the only
    /// thing that actually changed is which faces exist.
    #[wasm_bindgen(js_name = setFontFaces)]
    pub fn set_font_faces(&mut self, faces: JsValue) -> Result<(), JsValue> {
        let entries: Vec<FaceEntry> = serde_wasm_bindgen_compat(faces)?;
        let fonts = JsFonts::new(&self.services, descriptors_of(&entries));
        self.inner.session_mut().world_mut().set_fonts(fonts);
        Ok(())
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

    /// Parse font metadata without retaining the bytes.
    ///
    /// The host calls this once per font file, caches the result on disk keyed
    /// by path + mtime + size, and passes it back as `fontFaces` next time.
    #[wasm_bindgen(js_name = indexFont)]
    pub fn index_font(data: &[u8]) -> Result<JsValue, JsValue> {
        let faces: Vec<FaceEntry> = FontInfo::iter(data)
            .enumerate()
            .map(|(index, info)| FaceEntry { info, index: index as u32 })
            .collect();
        to_js(&faces)
    }

    /// The WASM linear memory currently allocated, in bytes.
    ///
    /// The host watches this against `typstUltra.memory.restartThresholdMb`.
    /// Linear memory is never returned to the OS, so restarting the child
    /// process is the only real reclamation mechanism.
    #[wasm_bindgen(js_name = heapBytes)]
    pub fn heap_bytes() -> f64 {
        (core::arch::wasm32::memory_size(0) * 65536) as f64
    }

    /// The upstream typst version this build compiles with.
    #[wasm_bindgen(js_name = typstVersion)]
    pub fn typst_version() -> String {
        typst_lsp_core::TYPST_VERSION.to_string()
    }
}

fn descriptors_of(entries: &[FaceEntry]) -> Vec<FaceDescriptor> {
    entries
        .iter()
        .map(|face| FaceDescriptor { info: face.info.clone(), index: face.index })
        .collect()
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

fn serde_wasm_bindgen_compat<T: for<'de> Deserialize<'de>>(
    value: JsValue,
) -> Result<T, JsValue> {
    let json = from_js(value)?;
    serde_json::from_value(json)
        .map_err(|err| JsValue::from_str(&format!("bad initialization options: {err}")))
}
