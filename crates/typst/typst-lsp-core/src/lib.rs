//! A Typst language server, minus a transport.
//!
//! [`Server`] takes a method name and a `serde_json::Value` and returns a
//! `Value` plus a queue of outbound notifications. `typst-lsp-wasm` wraps
//! exactly those three methods and nothing else; the test harness wraps the
//! same three. That is how the entire feature set is tested without WASM.
//!
//! Two clocks run here, and keeping them apart is the design's central trick
//! (see `docs/rfc/010-typst-ultra/design/architecture.md` §5):
//!
//! | State | Updated by | Feeds |
//! | --- | --- | --- |
//! | `Source` trees | every `didChange`, incrementally | completion, hover, definition, tokens, folding, symbols, formatting |
//! | `PagedDocument` | the debounced compile | diagnostics, preview, label completions, jump mapping, export |
//!
//! **An LSP request never triggers or waits for a compile.** `typst-ide`'s
//! heavyweight entry points all take the document as an `Option`, so a
//! completion arriving mid-typing answers from a fresh syntax tree plus a
//! slightly stale document, and stays under 30 ms regardless of document size.

pub mod capabilities;
pub mod convert;
pub mod dispatch;
pub mod features;
pub mod settings;
pub mod state;

use lsp_types::Uri;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use typst::syntax::FileId;
use typst_preview_core::PreviewSession;
use typst_session::ports::{ClockProvider, FileProvider, FontProvider, PackageProvider};
use typst_session::{Session, SessionWorld};

pub use capabilities::capabilities;
pub use convert::UriMap;
pub use dispatch::{Outbound, ResponseError};
pub use settings::Settings;
pub use state::{DocumentEntry, TokenCache};

/// The four ports a server instance is built on, bundled into one type
/// parameter so every signature in the crate stays readable.
pub trait Ports: 'static {
    /// Project and package file reads.
    type Files: FileProvider + Send + Sync;
    /// Font metadata and bytes.
    type Fonts: FontProvider + Send + Sync;
    /// Package availability.
    type Packages: PackageProvider + Send + Sync;
    /// The clock.
    type Clock: ClockProvider + Send + Sync;
}

/// The session type for a port bundle.
pub type PortSession<Q> = Session<
    <Q as Ports>::Files,
    <Q as Ports>::Fonts,
    <Q as Ports>::Packages,
    <Q as Ports>::Clock,
>;

/// The world type for a port bundle.
pub type PortWorld<Q> = SessionWorld<
    <Q as Ports>::Files,
    <Q as Ports>::Fonts,
    <Q as Ports>::Packages,
    <Q as Ports>::Clock,
>;

/// What the host tells the server at startup, beyond standard LSP.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerConfig {
    /// The compile root, as a `file:` URI.
    pub root_uri: String,
    /// Where typst's package cache lives, as a `file:` URI. `None` disables
    /// mapping package files to editor URIs.
    pub package_cache_uri: Option<String>,
    /// The `typstUltra.*` settings.
    pub settings: Settings,
}

/// The language server.
pub struct Server<Q: Ports> {
    session: PortSession<Q>,
    preview: PreviewSession,
    uris: UriMap,
    settings: Settings,
    /// Documents the editor has open, by file id.
    documents: FxHashMap<FileId, DocumentEntry>,
    /// URIs that currently carry diagnostics. A file that had them and now has
    /// none must be published as an empty array or the squiggles persist, so
    /// the difference between compiles is what gets cleared.
    published: FxHashSet<String>,
    /// Per-document semantic token cache, for `full/delta`.
    tokens: FxHashMap<FileId, TokenCache>,
    /// Files the host has told us about, for `workspace/symbol`.
    workspace_files: Vec<FileId>,
    /// The pinned compile root, if the user set one. When `None` the server
    /// follows the focused editor — the two modes decision 0008 describes.
    pinned_main: Option<FileId>,
    /// Notifications produced while handling a request, drained afterwards so
    /// Rust never re-enters the host mid-handler.
    outbox: Vec<Outbound>,
    /// The version of the most recent compile that produced diagnostics.
    last_published_version: Option<i32>,
}

impl<Q: Ports> Server<Q> {
    /// Build a server around an already-constructed session.
    ///
    /// The caller builds the session because only it knows how to make the
    /// ports — from JS callbacks in the WASM crate, from `std::fs` in tests.
    pub fn new(session: PortSession<Q>, config: ServerConfig) -> Self {
        let uris = UriMap::new(&config.root_uri, config.package_cache_uri.as_deref());
        let mut session = session;
        session.set_evict_age(config.settings.memory.evict_age);

        Self {
            session,
            preview: PreviewSession::new(),
            uris,
            settings: config.settings,
            documents: FxHashMap::default(),
            published: FxHashSet::default(),
            tokens: FxHashMap::default(),
            workspace_files: Vec::new(),
            pinned_main: None,
            outbox: Vec::new(),
            last_published_version: None,
        }
    }

    /// Handle a request and produce its result.
    pub fn on_request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        dispatch::request(self, method, params)
    }

    /// Handle a notification.
    pub fn on_notification(&mut self, method: &str, params: Value) {
        dispatch::notification(self, method, params);
    }

    /// Take the notifications produced since the last drain.
    pub fn drain(&mut self) -> Vec<Outbound> {
        std::mem::take(&mut self.outbox)
    }

    /// The compile session.
    pub fn session(&self) -> &PortSession<Q> {
        &self.session
    }

    /// Mutable access to the compile session.
    pub fn session_mut(&mut self) -> &mut PortSession<Q> {
        &mut self.session
    }

    /// The URI ⇄ file id map.
    pub fn uris(&self) -> &UriMap {
        &self.uris
    }

    /// The settings in force.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Apply new settings. Returns whether a recompile is warranted.
    pub fn set_settings(&mut self, settings: Settings) -> bool {
        let recompile = settings.memory.evict_age != self.settings.memory.evict_age
            || settings.diagnostics.enabled != self.settings.diagnostics.enabled;
        self.session.set_evict_age(settings.memory.evict_age);
        self.settings = settings;
        recompile
    }

    /// Tell the server which files the workspace contains, for
    /// `workspace/symbol`. The host walks the file system; we do not.
    pub fn set_workspace_files(&mut self, uris: &[Uri]) {
        self.workspace_files =
            uris.iter().filter_map(|uri| self.uris.to_file_id(uri)).collect();
    }

    /// Queue a notification for the host.
    pub fn notify(&mut self, method: impl Into<String>, params: Value) {
        self.outbox.push(Outbound { method: method.into(), params });
    }

    /// The open document for a file id, if any.
    pub fn document(&self, id: FileId) -> Option<&DocumentEntry> {
        self.documents.get(&id)
    }
}

/// The upstream typst version the server is built against.
pub const TYPST_VERSION: &str = typst_session::TYPST_VERSION;
