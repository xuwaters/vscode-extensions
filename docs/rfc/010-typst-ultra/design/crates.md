# RFC 010 — The Rust Crates

Four crates under `crates/typst/`. Every API shown here is written against the **real** typst 0.15.1
surface as verified by the spike; where a signature is quoted from upstream, the source file is linked.

```
crates/typst/
  typst-session/        World, VFS/font/package ports, compile session, export   ← no WASM
  typst-lsp-core/       LSP dispatch + IDE features                              ← no WASM
  typst-preview-core/   page SVG, page hashing/diff, jump mapping                ← no WASM
  typst-lsp-wasm/       #[wasm_bindgen] surface + host-callback glue             ← the only WASM crate
```

---

## 1. Why four crates

The repo has both precedents. RFC 009 kept `markdown-engine` as one crate because "there is no second
consumer on the horizon"; RFC 007/008 split `log-engine` from `log-parser` because there was. Typst has
three consumers of the compile session (the LSP features, the preview renderer, and the export commands)
and one hard constraint: **the WASM bindings must be isolated so everything else is testable with plain
`cargo test`**.

| Crate | Justification |
| --- | --- |
| `typst-session` | The compile session is shared state that both features and preview read. It is also the only place that touches host I/O ports, so it is where the fs-backed test doubles live |
| `typst-lsp-core` | Depends on `lsp-types`; nothing else should. Keeps LSP shapes out of the engine |
| `typst-preview-core` | Depends on `typst-svg`/`typst-render`; the LSP crate does not need either. Enables golden-image tests without an LSP harness |
| `typst-lsp-wasm` | `wasm-bindgen`, `js-sys`, and `unsafe impl Send`/`Sync` are quarantined to ~400 lines |

The ports pattern is what makes it work: `typst-session` defines `FileProvider`, `FontProvider`, and
`PackageProvider` as traits. `typst-lsp-wasm` implements them over JS callbacks; test modules implement
them over `std::fs`. Same code path, no `#[cfg(target_arch)]` scattered through the engine.

---

## 2. `typst-session`

The engine. Owns the `World`, the virtual file system, the font book, and the compile lifecycle.

```
crates/typst/typst-session/
  Cargo.toml
  src/
    lib.rs          # Session: the public entry point
    world.rs        # SessionWorld: impl World + impl typst_ide::IdeWorld
    vfs.rs          # Vfs: open-document overlay over the FileProvider
    fonts.rs        # FontSlots: lazy font loading over the FontProvider
    packages.rs     # package spec → root resolution, "needed" tracking
    compile.rs      # compile + evict, versioning, last-good document
    diagnostics.rs  # SourceDiagnostic → span → (file, byte range)
    export.rs       # PDF / SVG / PNG
  tests/
    fs_ports.rs     # std::fs implementations of the three ports
    compile.rs      # snapshot tests over a fixture corpus
```

### 2.1 The ports

```rust
/// Everything the engine needs from the outside world. Implementations are
/// synchronous — verified callable from inside a compile (see spike.md §6).
pub trait FileProvider {
    /// Read a file. `root` distinguishes project files from package files.
    fn read(&self, root: &VirtualRoot, vpath: &VirtualPath) -> FileResult<Bytes>;
    /// List a directory, for path completions. Optional; default is empty.
    fn list(&self, root: &VirtualRoot, vpath: &VirtualPath) -> Vec<String> { Vec::new() }
}

pub trait FontProvider {
    /// Metadata for every known face, built once by the host and cached on
    /// disk. `FontInfo` is `Serialize`/`Deserialize` upstream, so this
    /// round-trips exactly.
    fn faces(&self) -> &[FaceDescriptor];
    /// Bytes for one face. Called lazily — only for faces a document uses.
    fn data(&self, face: usize) -> Option<Bytes>;
}

pub trait PackageProvider {
    /// Resolve a package to a readable root, or report why not.
    /// `Pending` makes the engine emit a diagnostic and record the spec so
    /// the host can download it and trigger a recompile.
    fn resolve(&self, spec: &PackageSpec) -> PackageResolution;
}

pub enum PackageResolution { Ready, Pending, Failed(EcoString) }

pub struct FaceDescriptor { pub info: FontInfo, pub index: u32 }
```

### 2.2 `SessionWorld`

The upstream contract we implement ([`temp/typst/crates/typst-library/src/lib.rs:62`](../../../../temp/typst/crates/typst-library/src/lib.rs#L62)):

```rust
#[comemo::track]
pub trait World: Send + Sync {
    fn library(&self) -> &LazyHash<Library>;
    fn book(&self) -> &LazyHash<FontBook>;
    fn main(&self) -> FileId;
    fn source(&self, id: FileId) -> FileResult<Source>;
    fn file(&self, id: FileId) -> FileResult<Bytes>;
    fn font(&self, index: usize) -> Option<Font>;
    fn today(&self, offset: Option<Duration>) -> Option<Datetime>;
}
```

Our implementation:

```rust
pub struct SessionWorld<F, T, P> {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,          // from FontProvider::faces()
    slots: FontSlots<T>,               // face index → OnceCell<Option<Font>>
    vfs: Vfs<F>,                       // open-document overlay + FileProvider
    packages: Packages<P>,
    main: FileId,
    today: OnceCell<Option<Datetime>>, // one date per compile, per upstream guidance
}
```

Notes that matter:

- **`source()` goes through the VFS overlay first.** A file open in the editor is served from its in-memory
  `Source` (kept incrementally current via `Source::edit`), never from disk. Closed files fall through to
  `FileProvider::read`.
- **`font()` is lazy.** `FontSlots` holds a `OnceCell<Option<Font>>` per face; the first request calls
  `FontProvider::data(face)` and constructs `Font::new(bytes, index)`. A machine with 400 MB of installed
  fonts contributes ~2 KB of `FontInfo` each to the `FontBook` and *zero* bytes to the heap until a
  document actually selects a face.
- **`today()` is memoized per compile.** `Datetime` must not change mid-compile or comemo's constraint
  validation gets confused.
- **`IdeWorld` is implemented too** — `upcast()` returns `self`, `packages()` returns the Universe index if
  the host has fetched it, `files()` returns known `FileId`s so path completions work
  ([`temp/typst/crates/typst-ide/src/lib.rs:33`](../../../../temp/typst/crates/typst-ide/src/lib.rs#L33)).

### 2.3 The compile lifecycle

This is where the invariant from [architecture.md §6](architecture.md#6-memory-and-the-comemo-cache) is
enforced structurally, so no caller can get the order wrong:

```rust
impl Session {
    /// Compile, then evict — never the other way round. Reversing these costs
    /// ~50× in latency (research/spike.md §4.1), so the ordering is not
    /// exposed. `evict_age` defaults to 1 (decisions/0005).
    pub fn compile(&mut self, version: DocVersion) -> CompileOutcome {
        let warned = typst::compile::<PagedDocument>(&self.world);
        comemo::evict(self.evict_age);

        let diagnostics = self.collect(&warned);
        match warned.output {
            Ok(doc) => {
                self.last_good = Some(Arc::new(doc));
                self.last_good_version = version;
            }
            Err(_) => { /* keep the previous good document for IDE + preview */ }
        }
        CompileOutcome { version, diagnostics, document: self.last_good.clone() }
    }

    /// The document IDE features and the preview read. May lag one compile
    /// behind the source tree — deliberately. See architecture.md §5.
    pub fn last_good(&self) -> Option<&PagedDocument> { self.last_good.as_deref() }

    pub fn edit(&mut self, id: FileId, range: Range<usize>, text: &str) { … }
}
```

**Keeping the last good document on failure** is what makes the editing experience tolerable: a syntax
error mid-keystroke would otherwise blank the preview and kill label completions on every character typed.

### 2.4 Diagnostics

`typst::compile` returns `Warned<SourceResult<PagedDocument>>` — warnings always, errors on failure. Each
`SourceDiagnostic` carries a `Span`, which `WorldExt::range` turns into a byte range:

```rust
// temp/typst/crates/typst-library/src/lib.rs:141
pub trait WorldExt {
    fn range(&self, span: impl Into<DiagSpan>) -> Option<Range<usize>>;
}
```

Byte range → LSP `Range` needs a UTF-16 conversion, which `Source` provides via its line index. Detached
spans (no file) attach to the main file at offset 0 with a note. Diagnostics are grouped by `FileId` and
published per URI, including for files the editor does not have open — that is how an error in an imported
file shows up in the Problems panel.

`SourceDiagnostic` also carries `hints` and `trace` (the `#import` chain that led to the error); both map
onto `DiagnosticRelatedInformation`.

---

## 3. `typst-lsp-core`

Dispatch plus every IDE feature. Depends on `typst-session`, `typst-ide`, `typst-syntax`,
`typstyle-core`, and `lsp-types`. Feature-by-feature detail is in [lsp-features.md](lsp-features.md);
this is the shape.

```
crates/typst/typst-lsp-core/
  src/
    lib.rs            # Server: state + capabilities
    dispatch.rs       # method name → handler, params/result (de)serialization
    state.rs          # open documents, settings, compile scheduling policy
    convert.rs        # byte offsets ⇄ LSP positions (UTF-16), FileId ⇄ Uri
    features/
      diagnostics.rs  completion.rs  hover.rs  definition.rs  references.rs
      rename.rs       symbols.rs     semantic_tokens.rs       folding.rs
      selection.rs    links.rs       formatting.rs            inlay_hints.rs
  tests/
    fixtures/*.typ    # documents with a `/* CURSOR */` marker
```

The dispatch surface is transport-agnostic — it takes a method name and `serde_json::Value` and returns a
`Value` plus a queue of outbound messages:

```rust
pub struct Server<F, T, P> { session: Session<F, T, P>, state: State, outbox: Vec<Outbound> }

impl<F: FileProvider, T: FontProvider, P: PackageProvider> Server<F, T, P> {
    pub fn on_request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError>;
    pub fn on_notification(&mut self, method: &str, params: Value);
    pub fn drain(&mut self) -> Vec<Outbound>;   // notifications produced as a side effect
}
```

`typst-lsp-wasm` wraps exactly these three methods and nothing else. A native harness in `tests/` wraps
the same three, which is how the whole feature set is tested without WASM.

### Position conversion is not a footnote

Typst works in **byte offsets**; LSP works in **UTF-16 code units**. `convert.rs` is the single place that
translates, using `Source`'s line index, and it has its own property test (round-trip every offset in a
document containing CJK, emoji with ZWJ sequences, and combining marks). Getting this wrong produces
off-by-one squiggles that are maddening to debug, so it is isolated and tested rather than inlined at
twenty call sites.

---

## 4. `typst-preview-core`

Turns a `PagedDocument` into something a webview can display, incrementally.

```
crates/typst/typst-preview-core/
  src/
    lib.rs      # PreviewSession
    pages.rs    # per-page SVG render + content hashing
    patch.rs    # hash-sequence diff → page patch script
    jump.rs     # cursor ⇄ page position, wrapping typst_ide::jump_*
    export.rs   # whole-document SVG (svg_merged), PNG (typst-render)
```

```rust
pub struct PageMetrics { pub index: usize, pub width_pt: f64, pub height_pt: f64, pub hash: u64 }

pub struct PreviewSession { metrics: Vec<PageMetrics> }

impl PreviewSession {
    /// Cheap: hashes every page's frame without rendering SVG. Drives the
    /// webview's placeholder layout.
    pub fn measure(&mut self, doc: &PagedDocument) -> Vec<PageMetrics>;

    /// Render only the requested pages, and only those whose hash the client
    /// does not already hold.
    pub fn render(
        &self,
        doc: &PagedDocument,
        want: &[usize],
        known: &HashMap<usize, u64>,
    ) -> Vec<PagePatch>;
}

pub enum PagePatch {
    Unchanged { index: usize },
    Replace { index: usize, hash: u64, svg: String },
    Removed { index: usize },
}
```

Why this shape, from [spike.md §7](../research/spike.md#7-page-svg-anatomy):

- An A4 text page is **386 KB of SVG**; a 30-page document is **11.4 MB**. Rendering everything on every
  keystroke is not an option.
- Rendering *one* page is **~5 ms**. Rendering only what is visible plus a one-page prefetch margin keeps
  a keystroke at one or two page renders.
- Page hashing is what makes the "only what changed" part work. Hash the page's `Frame` content, not the
  rendered SVG string — hashing the frame is far cheaper than rendering, so `measure()` can run on every
  compile while `render()` runs only for visible pages.

Export is separate from preview because it wants the opposite trade-off — completeness over latency:

```rust
pub fn export_pdf(doc: &PagedDocument, opts: &PdfOptions) -> Result<Vec<u8>, EcoString>;
pub fn export_svg(doc: &PagedDocument, gap: Abs) -> String;   // typst_svg::svg_merged
pub fn export_png(doc: &PagedDocument, page: usize, ppi: f32) -> Vec<u8>;  // typst_render::render
```

`typst_svg::svg_merged(document, opts, gap)` is upstream's whole-document SVG writer with shared glyph
definitions — right for export, wrong for the live preview since it cannot be diffed per page.

### Jump mapping

Both directions come from upstream, unmodified:

```rust
// temp/typst/crates/typst-ide/src/jump.rs:34, :343
pub fn jump_from_click<D: JumpFromDocument>(world, document, position) -> Option<Jump>;
pub fn jump_from_cursor<D: JumpInDocument>(document, source, cursor) -> Vec<D::Position>;

pub enum Jump { File(FileId, usize), Url(Url), Position(PagedPosition) }
pub struct PagedPosition { pub page: NonZeroUsize, pub point: Point }
```

This is the SyncTeX-equivalent, and it is the reason we need no compiler patch for preview sync — tinymist's
`no-content-hint` fork exists to solve a problem this API does not have.

---

## 5. `typst-lsp-wasm`

The only crate that knows WASM exists. Target: under 400 lines.

```rust
#[wasm_bindgen]
pub struct TypstServer { inner: Server<JsFiles, JsFonts, JsPackages> }

#[wasm_bindgen]
impl TypstServer {
    #[wasm_bindgen(constructor)]
    pub fn new(host: JsHostServices, init: JsValue) -> Result<TypstServer, JsValue>;

    /// Synchronous: returns the LSP response value.
    pub fn on_request(&mut self, method: &str, params: JsValue) -> Result<JsValue, JsValue>;
    pub fn on_notification(&mut self, method: &str, params: JsValue);

    /// Notifications produced while handling the above. Drained by the JS
    /// loop *after* the response is written, so Rust never re-enters JS
    /// mid-handler.
    pub fn drain_events(&mut self) -> JsValue;

    /// Indexing helper: parse font metadata without retaining bytes.
    /// `FontInfo` is upstream-`Serialize`, so the host can cache the result.
    pub fn index_font(data: &[u8]) -> JsValue;

    pub fn heap_bytes() -> usize;
    pub fn version() -> String;
}
```

The port implementations are thin wrappers over `js_sys::Function`, calling synchronously — the pattern
[the spike verified](../research/spike.md#6-synchronous-host-vfs-callbacks):

```rust
struct JsFiles { read: js_sys::Function, list: js_sys::Function }

impl FileProvider for JsFiles {
    fn read(&self, root: &VirtualRoot, vpath: &VirtualPath) -> FileResult<Bytes> {
        let key = encode_root_and_path(root, vpath);
        let out = self.read.call1(&JsValue::NULL, &key.into())
            .map_err(|_| FileError::Other(Some("host read failed".into())))?;
        if out.is_null() || out.is_undefined() {
            return Err(FileError::NotFound(vpath.get_without_slash().into()));
        }
        Ok(Bytes::new(js_sys::Uint8Array::new(&out).to_vec()))
    }
}
```

### The `Send + Sync` question

`World: Send + Sync`, but `js_sys::Function` is neither. The spike used a blanket
`unsafe impl Send for … {}`. That is fine in the sense that `wasm32-unknown-unknown` here is genuinely
single-threaded, but "fine because of an assumption" is how unsound code gets written.

The production version wraps the JS handles in a marker type that makes the assumption checkable:

```rust
/// A value that is only safe to share because this build target has no
/// threads. Constructing one asserts that at compile time.
pub struct SingleThreaded<T>(T);

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
compile_error!("SingleThreaded is only sound on wasm32-unknown-unknown");

unsafe impl<T> Send for SingleThreaded<T> {}
unsafe impl<T> Sync for SingleThreaded<T> {}
```

If someone later builds this crate for a threaded target, it fails to compile instead of racing.

---

## 6. Dependency summary

| Crate | Upstream typst deps | Other |
| --- | --- | --- |
| `typst-session` | `typst`, `typst-layout`, `typst-syntax`, `typst-ide` (for `IdeWorld`), `typst-pdf` | `comemo`, `ecow`, `serde` |
| `typst-lsp-core` | `typst`, `typst-syntax`, `typst-ide` | `lsp-types` 0.97, `serde_json`, `typstyle-core`, `rustc-hash` |
| `typst-preview-core` | `typst`, `typst-layout`, `typst-svg`, `typst-render`, `typst-ide` | `rustc-hash` |
| `typst-lsp-wasm` | — (transitively all) | `wasm-bindgen`, `js-sys`, `serde-wasm-bindgen`, `console_error_panic_hook` |

All typst crates pinned to `0.15.1`, all from crates.io, **none patched**. `typst-timing`'s `wasm` feature
is deliberately left off ([spike.md §2](../research/spike.md#2-does-upstream-typst-build-for-wasm-unpatched)).

---

## 7. Build and workspace registration

The root `Cargo.toml` needs the change verified in [spike.md §8](../research/spike.md#9-workspace-layout-for-cratestypst) —
`members = ["crates/*"]` alone cannot express a grouping directory, and `exclude` alone drops the children:

```toml
[workspace]
members = [
  "crates/*",
  "crates/typst/typst-session",
  "crates/typst/typst-lsp-core",
  "crates/typst/typst-preview-core",
  "crates/typst/typst-lsp-wasm",
]
exclude = ["crates/typst"]      # stops the `crates/*` glob choking on the grouping dir
resolver = "3"
```

Explicit members override `exclude`, so the existing glob keeps working for every other crate. New typst
crates must be listed by hand.

Extension build scripts follow the repo convention verbatim:

```jsonc
// extensions/typst-ultra/package.json
"build:wasm": "cd ../../crates/typst/typst-lsp-wasm && wasm-pack build --target nodejs --out-dir ../../../extensions/typst-ultra/wasm --out-name typst_lsp_wasm",
"build":     "tsdown",
"package":   "pnpm run build:wasm && pnpm run build && vsce package --no-dependencies --allow-missing-repository",
"vscode:prepublish": "pnpm run build:wasm && tsdown --minify"
```

`extensions/*/wasm/` is already covered by the root [.gitignore](../../../../.gitignore) and ships in the VSIX.

Two build-profile notes from the spike:

- Keep the workspace's `opt-level = "s"`. `opt-level = 3` grew the artifact by 2 MB and produced no
  measurable speedup.
- `wasm-opt -Os` takes ~14 s and saves ~12 MB of raw size (34 MB → 22 MB). Worth it for `package`, so
  `[package.metadata.wasm-pack.profile.release] wasm-opt = ["-Os", "--strip-debug"]`. Several other
  crates in this repo set `wasm-opt = false` for build speed; this one earns the time.
