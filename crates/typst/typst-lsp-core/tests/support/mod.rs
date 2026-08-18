//! A native harness for the language server.
//!
//! Wraps exactly the three methods `typst-lsp-wasm` wraps — `on_request`,
//! `on_notification`, `drain` — over `std::fs` ports, which is what makes the
//! whole feature set testable with no WASM toolchain present.
//!
//! Fixtures carry a `/* CURSOR */` marker, following the shape `typst-ide`'s own
//! tests use.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use lsp_types::{Position, Uri};
use serde_json::{Value, json};
use typst::foundations::Bytes;
use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
use typst::text::FontInfo;
use typst_lsp_core::{Outbound, Ports, ServerConfig, Server as CoreServer};
use typst_session::fs::{FsFiles, FsPackages, SystemClock};
use typst_session::ports::{FaceDescriptor, FontProvider};
use typst_session::{Session, SessionWorld};

/// The marker fixtures use to say where the cursor is.
pub const CURSOR: &str = "/* CURSOR */";

/// typst's bundled fonts, in memory.
pub struct BundledFonts {
    faces: Vec<FaceDescriptor>,
    data: Vec<Bytes>,
}

impl BundledFonts {
    pub fn new() -> Self {
        let mut faces = Vec::new();
        let mut data = Vec::new();
        for file in typst_assets::fonts() {
            let bytes = Bytes::new(file);
            for (index, info) in FontInfo::iter(file).enumerate() {
                faces.push(FaceDescriptor { info, index: index as u32 });
                data.push(bytes.clone());
            }
        }
        Self { faces, data }
    }
}

impl Default for BundledFonts {
    fn default() -> Self {
        Self::new()
    }
}

impl FontProvider for BundledFonts {
    fn faces(&self) -> &[FaceDescriptor] {
        &self.faces
    }

    fn data(&self, face: usize) -> Option<Bytes> {
        self.data.get(face).cloned()
    }
}

/// The port bundle the tests run on.
pub struct FsPorts;

impl Ports for FsPorts {
    type Files = FsFiles;
    type Fonts = BundledFonts;
    type Packages = FsPackages;
    type Clock = SystemClock;
}

/// A server plus the bookkeeping a client would do.
pub struct Harness {
    server: CoreServer<FsPorts>,
    root: PathBuf,
    version: i32,
}

/// The fixture corpus root.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

impl Harness {
    /// A server rooted at the fixture directory.
    pub fn new() -> Self {
        Self::rooted(fixtures())
    }

    /// A server rooted anywhere.
    pub fn rooted(root: PathBuf) -> Self {
        let main = file_id("main.typ");
        let world = SessionWorld::new(
            FsFiles::new(&root),
            BundledFonts::new(),
            FsPackages::new(None),
            SystemClock,
            main,
        );

        let server = CoreServer::new(
            Session::new(world, 1),
            ServerConfig {
                root_uri: uri_of(&root),
                package_cache_uri: None,
                settings: Default::default(),
            },
        );

        Self { server, root, version: 0 }
    }

    /// The underlying server, for direct calls.
    pub fn server(&mut self) -> &mut CoreServer<FsPorts> {
        &mut self.server
    }

    /// The URI of a workspace-relative path.
    pub fn uri(&self, relative: &str) -> Uri {
        format!("{}/{}", uri_of(&self.root), relative).parse().expect("valid uri")
    }

    /// Open a document with the given text, returning its URI.
    pub fn open(&mut self, relative: &str, text: &str) -> Uri {
        let uri = self.uri(relative);
        self.version += 1;
        self.server.on_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri.as_str(),
                    "languageId": "typst",
                    "version": self.version,
                    "text": text,
                }
            }),
        );
        uri
    }

    /// Open a fixture that contains a `/* CURSOR */` marker, returning the URI
    /// and the position the marker sat at (with the marker removed).
    pub fn open_with_cursor(&mut self, relative: &str, text: &str) -> (Uri, Position) {
        let offset = text.find(CURSOR).expect("fixture needs a /* CURSOR */ marker");
        let stripped = text.replace(CURSOR, "");
        let uri = self.open(relative, &stripped);

        let before = &stripped[..offset];
        let line = before.matches('\n').count() as u32;
        let column = before.rsplit('\n').next().unwrap_or("").chars().map(char::len_utf16).sum::<usize>();

        (uri, Position { line, character: column as u32 })
    }

    /// Replace an open document's text.
    pub fn change(&mut self, uri: &Uri, text: &str) {
        self.version += 1;
        self.server.on_notification(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri.as_str(), "version": self.version },
                "contentChanges": [{ "text": text }],
            }),
        );
    }

    /// Run a compile and return the notifications it produced.
    pub fn compile(&mut self, uri: &Uri) -> Vec<Outbound> {
        self.server
            .on_notification("typst/compile", json!({ "uri": uri.as_str() }));
        self.server.drain()
    }

    /// Send a request and unwrap the result.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.server
            .on_request(method, params)
            .unwrap_or_else(|error| panic!("{method} failed: {}", error.message))
    }

    /// Send a request expecting it to fail, returning the message.
    pub fn request_err(&mut self, method: &str, params: Value) -> String {
        match self.server.on_request(method, params) {
            Ok(value) => panic!("{method} unexpectedly succeeded: {value}"),
            Err(error) => error.message,
        }
    }

    /// The diagnostics published for a URI by the most recent compile.
    pub fn diagnostics(&mut self, uri: &Uri) -> Vec<Value> {
        let events = self.server.drain();
        diagnostics_in(&events, uri)
    }
}

/// The diagnostics for a URI within a batch of notifications.
pub fn diagnostics_in(events: &[Outbound], uri: &Uri) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event.method == "textDocument/publishDiagnostics")
        .filter(|event| event.params["uri"] == json!(uri.as_str()))
        .flat_map(|event| {
            event.params["diagnostics"].as_array().cloned().unwrap_or_default()
        })
        .collect()
}

/// Every notification of a given method in a batch.
pub fn events_named<'a>(events: &'a [Outbound], method: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event.method == method)
        .map(|event| &event.params)
        .collect()
}

/// A project-rooted file id.
pub fn file_id(path: &str) -> FileId {
    FileId::new(RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("valid virtual path"),
    ))
}

/// A `file:` URI for a real directory, with the pieces that need escaping
/// escaped.
pub fn uri_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace(' ', "%20");
    format!("file://{text}")
}

/// Shorthand for a `textDocument/position` params object.
pub fn at(uri: &Uri, position: Position) -> Value {
    json!({
        "textDocument": { "uri": uri.as_str() },
        "position": { "line": position.line, "character": position.character },
    })
}
