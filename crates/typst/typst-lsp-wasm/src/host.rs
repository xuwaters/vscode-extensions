//! The ports, implemented over synchronous JS callbacks.
//!
//! Synchronous is the load-bearing word: these are called from inside
//! `World::file` in the middle of a compile. The feasibility spike verified
//! that a `js_sys::Function` call from that position works under
//! `wasm-bindgen --target nodejs`, including error propagation through
//! `FileResult`.
//!
//! Anything that cannot be synchronous — package downloads, system font
//! indexing — is deferred: the provider reports `Pending`, the compile finishes
//! with a diagnostic naming the package, and the host recompiles once it lands.

use ecow::EcoString;
use js_sys::{Function, Uint8Array};
use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst::syntax::package::PackageSpec;
use typst::syntax::{VirtualPath, VirtualRoot};
use typst_session::ports::{
    ClockProvider, FaceDescriptor, FileProvider, FontProvider, PackageProvider,
    PackageResolution,
};
use wasm_bindgen::JsValue;

use crate::keys::encode_root;
use crate::single_threaded::SingleThreaded;

/// The JS side of the boundary, as this crate needs it.
pub struct HostServices {
    read_file: SingleThreaded<Function>,
    list_dir: SingleThreaded<Function>,
    font_data: SingleThreaded<Function>,
    resolve_package: SingleThreaded<Function>,
    now: SingleThreaded<Function>,
    timezone_offset: SingleThreaded<Function>,
}

impl HostServices {
    /// Pull the callbacks off a plain JS object.
    ///
    /// A missing one is a programming error in `server/main.ts`, and it is
    /// better to fail loudly at construction than on the first compile.
    pub fn from_js(host: &JsValue) -> Result<Self, JsValue> {
        Ok(Self {
            read_file: SingleThreaded::new(method(host, "readFile")?),
            list_dir: SingleThreaded::new(method(host, "listDir")?),
            font_data: SingleThreaded::new(method(host, "fontData")?),
            resolve_package: SingleThreaded::new(method(host, "resolvePackage")?),
            now: SingleThreaded::new(method(host, "now")?),
            timezone_offset: SingleThreaded::new(method(host, "timezoneOffsetMinutes")?),
        })
    }
}

fn method(host: &JsValue, name: &str) -> Result<Function, JsValue> {
    let value = js_sys::Reflect::get(host, &JsValue::from_str(name))?;
    value.dyn_into_function().ok_or_else(|| {
        JsValue::from_str(&format!("host services are missing `{name}`"))
    })
}

/// `JsValue::dyn_into` for functions, without pulling in the cast machinery.
trait DynIntoFunction {
    fn dyn_into_function(self) -> Option<Function>;
}

impl DynIntoFunction for JsValue {
    fn dyn_into_function(self) -> Option<Function> {
        self.is_function().then(|| Function::from(self))
    }
}

/// Project and package file reads.
pub struct JsFiles {
    read: SingleThreaded<Function>,
    list: SingleThreaded<Function>,
}

impl JsFiles {
    /// Build from the host services bag.
    pub fn new(host: &HostServices) -> Self {
        Self {
            read: SingleThreaded::new(host.read_file.get().clone()),
            list: SingleThreaded::new(host.list_dir.get().clone()),
        }
    }
}

impl FileProvider for JsFiles {
    fn read(&self, root: &VirtualRoot, vpath: &VirtualPath) -> FileResult<Bytes> {
        let out = self
            .read
            .get()
            .call2(
                &JsValue::NULL,
                &JsValue::from_str(&encode_root(root)),
                &JsValue::from_str(vpath.get_with_slash()),
            )
            .map_err(|err| FileError::Other(Some(describe(&err))))?;

        if out.is_null() || out.is_undefined() {
            return Err(FileError::NotFound(vpath.get_without_slash().into()));
        }
        // A string result is the host reporting a read error it wants attributed
        // to this file, rather than to the whole compile.
        if let Some(message) = out.as_string() {
            return Err(FileError::Other(Some(message.into())));
        }

        Ok(Bytes::new(Uint8Array::new(&out).to_vec()))
    }

    fn list(&self, root: &VirtualRoot, vpath: &VirtualPath) -> Vec<String> {
        let Ok(out) = self.list.get().call2(
            &JsValue::NULL,
            &JsValue::from_str(&encode_root(root)),
            &JsValue::from_str(vpath.get_with_slash()),
        ) else {
            return Vec::new();
        };

        js_sys::Array::from(&out).iter().filter_map(|entry| entry.as_string()).collect()
    }
}

/// Font metadata supplied at startup; bytes fetched lazily.
pub struct JsFonts {
    faces: Vec<FaceDescriptor>,
    data: SingleThreaded<Function>,
}

impl JsFonts {
    /// Build from the host's font index.
    pub fn new(host: &HostServices, faces: Vec<FaceDescriptor>) -> Self {
        Self { faces, data: SingleThreaded::new(host.font_data.get().clone()) }
    }
}

impl FontProvider for JsFonts {
    fn faces(&self) -> &[FaceDescriptor] {
        &self.faces
    }

    fn data(&self, face: usize) -> Option<Bytes> {
        let out = self
            .data
            .get()
            .call1(&JsValue::NULL, &JsValue::from_f64(face as f64))
            .ok()?;

        if out.is_null() || out.is_undefined() {
            return None;
        }
        Some(Bytes::new(Uint8Array::new(&out).to_vec()))
    }
}

/// Package availability, asked once per compile per package.
pub struct JsPackages {
    resolve: SingleThreaded<Function>,
    index: Vec<(PackageSpec, Option<EcoString>)>,
}

impl JsPackages {
    /// Build from the host services bag, with an optional Universe index for
    /// package-name completions.
    pub fn new(host: &HostServices, index: Vec<(PackageSpec, Option<EcoString>)>) -> Self {
        Self {
            resolve: SingleThreaded::new(host.resolve_package.get().clone()),
            index,
        }
    }
}

impl PackageProvider for JsPackages {
    fn resolve(&self, spec: &PackageSpec) -> PackageResolution {
        let out = self
            .resolve
            .get()
            .call1(&JsValue::NULL, &JsValue::from_str(&spec.to_string()));

        let Ok(out) = out else {
            return PackageResolution::Failed("the host could not be reached".into());
        };
        let Some(text) = out.as_string() else {
            return PackageResolution::Failed("the host gave no answer".into());
        };

        match text.as_str() {
            "ready" => PackageResolution::Ready,
            "pending" => PackageResolution::Pending,
            other => PackageResolution::Failed(
                other.strip_prefix("failed:").unwrap_or(other).into(),
            ),
        }
    }

    fn index(&self) -> &[(PackageSpec, Option<EcoString>)] {
        &self.index
    }
}

/// The host's clock.
pub struct JsClock {
    now: SingleThreaded<Function>,
    offset: SingleThreaded<Function>,
}

impl JsClock {
    /// Build from the host services bag.
    pub fn new(host: &HostServices) -> Self {
        Self {
            now: SingleThreaded::new(host.now.get().clone()),
            offset: SingleThreaded::new(host.timezone_offset.get().clone()),
        }
    }
}

impl ClockProvider for JsClock {
    fn now_ms(&self) -> Option<i64> {
        let out = self.now.get().call0(&JsValue::NULL).ok()?;
        Some(out.as_f64()? as i64)
    }

    fn local_offset_minutes(&self) -> i64 {
        self.offset
            .get()
            .call0(&JsValue::NULL)
            .ok()
            .and_then(|out| out.as_f64())
            .map(|minutes| minutes as i64)
            .unwrap_or(0)
    }
}

fn describe(error: &JsValue) -> EcoString {
    error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(error, &JsValue::from_str("message"))
                .ok()
                .and_then(|message| message.as_string())
        })
        .unwrap_or_else(|| "the host read failed".to_string())
        .into()
}
