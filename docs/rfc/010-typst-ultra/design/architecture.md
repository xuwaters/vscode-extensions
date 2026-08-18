# RFC 010 — Architecture

Process model, data flow, and the protocols between the pieces. Crate-level detail is in
[crates.md](crates.md); the preview specifics are in [preview.md](preview.md).

---

## 1. The whole system

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ VSCode                                                                               │
│                                                                                      │
│  ┌─ Renderer ────────────────────┐          ┌─ Webview (preview panel) ───────────┐  │
│  │                               │          │                                     │  │
│  │   TextEditor (main.typ)       │          │  virtualized page list              │  │
│  │     ▲          │              │          │   ├─ page 1  <svg>  (rendered)      │  │
│  │     │ reveal   │ edits,       │          │   ├─ page 2  <svg>  (rendered)      │  │
│  │     │ decorate │ cursor       │          │   ├─ page 3  placeholder            │  │
│  └─────┼──────────┼──────────────┘          │   └─ …                              │  │
│        │          │                         │  zoom · fit · invert · find         │  │
│        │          ▼                         └─────────▲────────────┬──────────────┘  │
│  ┌─────┴──────────────────────────────────────────────┼────────────▼──────────────┐  │
│  │ Extension Host (Node)      extensions/typst-ultra  │  postMessage (typed)      │  │
│  │                                                    │                           │  │
│  │  extension.ts ─ activation, commands, status bar   │                           │  │
│  │  client.ts    ─ LanguageClient (vscode-languageclient/node)                    │  │
│  │  preview/     ─ PreviewManager, panel lifecycle, custom editor                 │  │
│  │  sync.ts      ─ cursor→page, page→cursor, loop guards                          │  │
│  │  export.ts    ─ PDF/SVG/PNG commands, save dialogs                             │  │
│  │  fonts.ts     ─ system font discovery + on-disk index cache                    │  │
│  └────────────────────────────────┬───────────────────────────────────────────────┘  │
│                                   │  LSP (JSON-RPC over Node IPC)                    │
│                                   │  standard methods + typst/* extensions           │
│  ┌────────────────────────────────▼───────────────────────────────────────────────┐  │
│  │ Language server — child process (Node)          extensions/typst-ultra/server/ │  │
│  │                                                                                │  │
│  │  server.js                                                                     │  │
│  │   ├─ createConnection(ProposedFeatures.all)   ← vscode-languageserver          │  │
│  │   ├─ HostServices (SYNCHRONOUS — called from Rust during a compile)            │  │
│  │   │    readFile(vpath) → Uint8Array | null      listDir(vpath) → string[]      │  │
│  │   │    fontData(faceId) → Uint8Array            today(offsetMinutes) → i64     │  │
│  │   ├─ AsyncServices (deferred; results fed back, then recompile)                │  │
│  │   │    downloadPackage(spec)  ·  indexSystemFonts()                            │  │
│  │   └─ require('./wasm/typst_lsp_wasm.js')                                       │  │
│  │        ┌────────────────────────────────────────────────────────────────────┐  │  │
│  │        │  typst-lsp-wasm   #[wasm_bindgen] Server { on_request, on_notify } │  │  │
│  │        ├────────────────────────────────────────────────────────────────────┤  │  │
│  │        │  typst-lsp-core        typst-preview-core                          │  │  │
│  │        │   dispatch, features    page SVG, hashing, jump mapping            │  │  │
│  │        ├────────────────────────────────────────────────────────────────────┤  │  │
│  │        │  typst-session   World · Vfs · FontBook · Packages · CompileState  │  │  │
│  │        ├────────────────────────────────────────────────────────────────────┤  │  │
│  │        │  upstream (crates.io, unmodified):                                 │  │  │
│  │        │    typst · typst-layout · typst-ide · typst-syntax                 │  │  │
│  │        │    typst-svg · typst-render · typst-pdf · typstyle-core            │  │  │
│  │        └────────────────────────────────────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     │ host-side I/O only
                                     ▼
       project files (fs)   ·   assets/fonts/ (bundled)   ·   system font dirs
                            ·   ~/.cache/typst/packages   ·   packages.typst.org
```

---

## 2. Why a child process, and not the extension host

Every other Rust-backed extension in this repo `require()`s its WASM straight into the extension host. This
one does not. Decision and alternatives: [0003](../decisions/0003-server-in-child-process.md); the numbers
below are from [spike.md §4.2](../research/spike.md#42-eviction-age-sweep).

| Reason | Number | Consequence in the extension host |
| --- | --- | --- |
| Cold compile blocks the thread | 81 ms (10 pages) → 262 ms (75 pages) | Blocks every other extension's event handling for that long |
| WASM heap is never returned to the OS | 32 MB (10 pages) → 106 MB (75 pages) at the default eviction age | A tenant that only grows is antisocial in a shared host — though at 106 MB this is a weaker argument than it first appeared |
| A compiler panic is fatal | — | In-process, it would take down every extension in the host |

A child process gets us memory that is genuinely reclaimed on restart, crash isolation (a compiler panic
kills the server, not the window), and a natural place to hang the "restart the server" command.

> **Honest note.** An earlier revision justified this with a 423 MB heap figure. That was measured at
> eviction age 10; at the age-1 default chosen in [0005](../decisions/0005-cache-eviction-policy.md) the
> figure is 106 MB. The decision now rests primarily on **blocking and crash isolation**, not memory.

### Options considered

| Option | Verdict |
| --- | --- |
| **Child process, `TransportKind.ipc`** (chosen) | Standard `vscode-languageclient` path, zero custom transport code, full isolation, synchronous `fs` available for the VFS |
| `worker_threads` + custom `MessageReader`/`Writer` | Keeps one process and starts faster, but the WASM heap still lives in the extension host's address space — which is the main thing we are avoiding |
| WASM directly in the extension host (repo default) | Rejected on the memory and blocking numbers above |
| `wasm32-wasip1` + `@vscode/wasm-wasi-lsp` | Gives real `std::fs` inside Rust, but adds a hard dependency on the "WebAssembly Execution Engine" extension. Too much install-time fragility for the benefit |
| Browser worker (`vscode-languageclient/browser`), as tinymist's web build does | The right shape for vscode.dev, and possible later since the artifact is already WASM. Not Phase 1 |

### Cost we accept

~30–40 MB RSS for the Node child process itself, plus fork latency on first `.typ` open (the server starts
lazily, not at activation). Both are ordinary for a language server.

---

## 3. Startup sequence

```
  Extension host                Server process              WASM
       │                              │                       │
  activate (onLanguage:typst)         │                       │
       │                              │                       │
       ├─ discover bundled fonts      │                       │
       │  (assets/fonts/*.otf|ttf)    │                       │
       ├─ read font index cache       │                       │
       │  (globalStorage, keyed by    │                       │
       │   path+mtime+size)           │                       │
       │                              │                       │
       ├─ LanguageClient.start() ────▶ fork server.js         │
       │                              ├─ require wasm ───────▶ instantiate (22 MB)
       │                              │                       │
       ├─ initialize ────────────────▶│──────────────────────▶ Server::new()
       │   rootUri, capabilities,     │                       │
       │   initializationOptions:     │                       ├─ build FontBook from
       │     { fontFaces, settings }  │                       │  host-supplied FontInfo
       │◀──────────── InitializeResult ◀──────────────────────┤  (bytes NOT retained)
       ├─ initialized ───────────────▶│                       │
       │                              │                       │
       ├─ didOpen(main.typ) ─────────▶│──────────────────────▶ Vfs::open, Source::new
       │                              │                       ├─ compile
       │                              │                       │  └─ World::file ──┐
       │                              │◀─── readFile(vpath) ──┘  (synchronous)    │
       │                              ├──── Uint8Array ──────▶                    │
       │◀──── publishDiagnostics ─────┤◀──────────────────────┤                   │
       │                              │                       │                    
       │  ── background, non-blocking ──                      │
       ├─ indexSystemFonts() ────────▶│──────────────────────▶ FontBook rebuilt
       │◀──── typst/fontsChanged ─────┤                       ├─ recompile
       │◀──── publishDiagnostics ─────┤◀──────────────────────┤
```

Two deliberate properties:

- **Bundled fonts first.** The `FontBook` is usable before system fonts are indexed, so the first compile
  is correct for the overwhelming majority of documents (which use typst's defaults). System fonts arrive
  later and trigger one recompile.
- **The font index is cached on disk**, keyed by `(path, mtime, size)`, so the expensive scan happens once
  per machine rather than once per session.

---

## 4. Edit → diagnostics → preview

```
  Editor        Host              Server                WASM                Webview
    │            │                  │                     │                    │
  type ─────────▶│                  │                     │                    │
    │            ├─ didChange ─────▶│                     │                    │
    │            │  (incremental)   ├─ apply ────────────▶ Source::edit(range) │
    │            │                  │                     │  (incremental      │
    │            │                  │                     │   reparse)         │
    │            │                  │                     │                    │
    │            │            debounce 150 ms (typstUltra.compile.debounce)    │
    │            │                  │                     │                    │
    │            │                  ├─ compile ──────────▶ typst::compile      │
    │            │                  │                     ├─ …                 │
    │            │                  │                     ├─ comemo::evict(N)  │  ← AFTER, never before
    │            │◀ publishDiagnostics ◀──────────────────┤                    │
    │◀ squiggles ┤                  │                     │                    │
    │            │                  │                     │                    │
    │            │  preview panel visible? pages in view = [3,4]               │
    │            ├─ typst/renderPages ─▶│                  │                    │
    │            │   {pages:[3,4],      ├─ render ───────▶ typst_svg::svg(p)   │
    │            │    knownHashes:{…}}  │                  ├─ hash each page   │
    │            │◀── {patches:[…]} ────┤◀────────────────┤                    │
    │            ├─ postMessage ────────────────────────────────────────────────▶ swap
    │            │   {type:'pages', seq, patches}          │                    │  changed
    │            │                                          │                   │  <svg> only
```

`knownHashes` is what keeps this cheap: the host tells the server which page hashes the webview already
holds, and the server returns SVG only for pages whose hash changed. A typical keystroke changes one page,
so one ~386 KB string crosses the wire ([spike.md §7](../research/spike.md#7-page-svg-anatomy)) instead of thirty.

---

## 5. Concurrency model: one thread, two clocks

The WASM instance is single-threaded. Everything the server does is serialized. That would be a problem if
IDE requests had to wait behind compiles, so they do not:

> **Invariant: an LSP request is never allowed to trigger or wait for a compile.**

This is implementable because `typst-ide`'s three heavyweight entry points all take the compiled document
as an `Option`:

```rust
// temp/typst/crates/typst-ide/src/complete.rs:38
pub fn autocomplete(
    world: &dyn IdeWorld,
    output: Option<impl AsOutput>,   // ← optional
    source: &Source,
    cursor: usize,
    explicit: bool,
) -> Option<(usize, Vec<Completion>)>
```

So the server keeps two pieces of state on different clocks:

| State | Updated by | Freshness | Feeds |
| --- | --- | --- | --- |
| `Source` tree (per file) | every `didChange`, via `Source::edit` | immediate, incremental | completion, hover, definition, semantic tokens, folding, selection ranges, symbols, formatting |
| `PagedDocument` (last successful compile) | the debounced compile | up to one debounce behind | diagnostics, preview pages, label completions, jump mapping, export |

A completion request arriving mid-typing answers from a fresh syntax tree plus a slightly stale document.
The staleness is invisible in practice — it affects only cross-reference completions and hover previews of
labels — and it is what keeps completion at <30 ms regardless of document size.

### Cancellation

Compiles are cancelled by supersession, not interruption: a `didChange` arriving during the debounce window
resets the timer; a `didChange` arriving *during* a compile cannot stop it (single thread, no yield points),
but its result is discarded if a newer document version exists when the compile returns. Each compile
carries the document version it started from; `publishDiagnostics` for a stale version is dropped.

The practical worst case is one wasted compile of up to ~230 ms. Acceptable, and the alternative
(instrumenting typst for cancellation points) requires patching the compiler.

---

## 6. Memory and the comemo cache

[comemo](https://crates.io/crates/comemo) is typst's memoization layer, and it is the reason incremental
recompiles are 6 ms instead of 134 ms. It is also where all the memory goes. Full rationale:
[0005](../decisions/0005-cache-eviction-policy.md).

**The rule, and it is not optional:**

```rust
// Correct — matches typst-cli's watch loop (temp/typst/crates/typst-cli/src/watch.rs:82)
let result = typst::compile::<PagedDocument>(&world);
comemo::evict(evict_age);

// Wrong — measured at 411 ms/keystroke instead of 7 ms
comemo::evict(evict_age);
let result = typst::compile::<PagedDocument>(&world);
```

Evicting first discards memoized layout that the compile immediately about to run would have reused. The
spike hit this and it cost a 50× latency regression ([spike.md §4.1](../research/spike.md#41-the-measurement-that-changed-the-design)).
`typst-session` exposes compile-then-evict as a single method so the order cannot be got wrong by a caller,
and a unit test asserts the ratio between a warm and a cold recompile.

Eviction age is exposed as `typstUltra.memory.evictAge`, **default `1`** — deliberately not typst-cli's
`10`, because our workload is per-keystroke editing rather than whole-file saves in a watch loop. The sweep
([spike.md §4.2](../research/spike.md#42-eviction-age-sweep)) is one-sided; age 1 wins on every axis:

| Age | Steady, 75 pages | Warm-up | p95 | Heap |
| --- | --- | --- | --- | --- |
| **1** (default) | 15 ms | **17 ms** | **18 ms** | **106 MB** |
| 3 | 16 ms | 19 ms | 107 ms | 170 MB |
| 10 (typst-cli's) | 15 ms | 196 ms | 278 ms | 394 MB |
| none | 15 → 41 ms and rising | — | — | +128 MB per 10 edits, unbounded |

Age 1 still retains everything touched by the previous compile — precisely the working set incremental
recompilation needs. Larger ages retain generations that are never reused, which costs both lookup time
(visible as p95) and memory.

Above `typstUltra.memory.restartThresholdMb` (default 1024, `0` disables) the server reports its heap in a
status-bar item and offers a one-click restart. WASM linear memory cannot be handed back to the OS, so
restarting the child process is the only real reclamation mechanism — and it is cheap, because the server
holds no unsaved state.

---

## 7. Protocols

### 7.1 Host and WASM (inside the server process)

The WASM surface is deliberately tiny — three entry points and a callback bag. Full signatures in
[crates.md §5](crates.md#5-typst-lsp-wasm).

```
JS → WASM   new TypstServer(host, init)                            // init: InitOptions, below
            server.capabilities()          -> ServerCapabilities   // what to answer `initialize` with
            server.onRequest(method, params) -> result             // sync, returns the LSP response
            server.onNotification(method, params)
            server.drainEvents()           -> {method, params}[]   // queued server→client messages
            server.setFontFaces(faces)                             // after system-font indexing
            TypstServer.indexFont(bytes)   -> FaceEntry[]          // static; parses metadata, keeps none
            TypstServer.heapBytes()        -> number               // static; for the watchdog
            TypstServer.typstVersion()     -> string               // static

WASM → JS   host.readFile(root, vpath)     -> Uint8Array | null    SYNC, callable mid-compile
            host.listDir(root, vpath)      -> string[]             SYNC
            host.fontData(faceIndex)       -> Uint8Array | null    SYNC
            host.resolvePackage(spec)      -> "ready"|"pending"|"failed:…"   SYNC
            host.now()                     -> number               SYNC, epoch ms
            host.timezoneOffsetMinutes()   -> number               SYNC, minutes east of UTC
```

Differences from the RFC's sketch, all of them found while building:

- **`readFile` takes a root as well as a path.** A file id is a `(root, vpath)` pair upstream, where the
  root is either the project or a package — and the host is the only side that knows where a package
  lives on this machine. The root crosses as `""` or `"@preview/cetz:0.4.2"`.
- **`resolvePackage` replaces `requestPackage`.** Fire-and-forget was not enough: the compile needs an
  answer *now* to decide between "not there yet" and "will never be there", and those produce different
  diagnostics. The download itself is still fire-and-forget behind it.
- **`now()` + `timezoneOffsetMinutes()` replace `today(offsetMinutes)`.** Marshalling a date across the
  boundary is more work than marshalling an instant; the conversion is arithmetic and belongs in Rust.
- **`log` was not needed.** Everything the server wants to say goes through `drainEvents` as a
  `window/logMessage` notification, which the client already routes to the output channel.
- **`setFontFaces`** rebuilds the font book in place when system-font indexing finishes, rather than
  restarting the session — the open documents are unchanged and would otherwise have to be resent.

`init` is an `InitOptions` object rather than raw `InitializeParams`: `rootUri`, `packageCacheUri`,
`mainPath`, the settings, the font index, and the Universe package list. The host has already read the
font cache and discovered the bundled fonts by the time it constructs the server.

Requests are synchronous by design — [spike.md §6](../research/spike.md#6-synchronous-host-vfs-callbacks) verified
that a `js_sys::Function` call from inside `World::file` works under `--target nodejs`, including error
propagation. Anything that cannot be synchronous (package downloads, system font indexing) is deferred:
the server records the need, finishes the compile with a "package not yet available" diagnostic, emits an
event, and the host recompiles once the resource lands.

`drain_events` exists because a Rust-side request handler may need to send notifications (diagnostics,
progress, log messages) as a side effect. Rather than re-entering JS mid-handler, events are queued and
drained after the response is written — the same shape tinymist's web worker uses
([`temp/tinymist/editors/vscode/src/web/server.ts`](../../../../temp/tinymist/editors/vscode/src/web/server.ts)).

### 7.2 Extension host and server (LSP)

Standard LSP for everything standard. Four extensions, all under the `typst/` namespace:

| Method | Direction | Purpose |
| --- | --- | --- |
| `typst/renderPages` | client → server | `{ uri, pages, knownHashes, mode, ppi }` → page patches, SVG or PNG |
| `typst/documentMetrics` | client → server | `{ uri }` → `{ pageCount, pageSizes, hashes }` — what the webview needs to lay out placeholders |
| `typst/jumpFromClick` | client → server | `{ uri, page, x, y }` → `{ uri, offset } \| { url } \| null` |
| `typst/jumpFromCursor` | client → server | `{ uri, offset }` → `{ page, x, y }[]` |
| `typst/export` | client → server | `{ format: 'pdf'\|'svg'\|'png'\|'html', page?, ppi? }` → base64 bytes |
| `typst/template` | client → server | `{ spec }` → `{ root, template? }` — scaffolding (P4-14). Answered by the **Node** side, not Rust: it needs the network and can wait |
| `typst/compile` | client → server | `{ uri }` — the host's debounce expired. The timer lives in Node, since WASM has no runtime to hang one on |
| `typst/setMain` | client → server | `{ uri \| null }` — pin or unpin the compile root |
| `typst/workspaceFiles` | client → server | `{ uris }` — what `workspace/symbol` may search. The host walks the file system; the server does not |
| `typst/heapBytes` | client → server | `{}` → bytes, polled by the memory watchdog |
| `typst/compileStatus` | server → client | `{ uri, state: 'compiling'\|'ok'\|'error', ms, pageCount }` — status bar |
| `typst/packageStatus` | server → client | `{ spec, state: 'downloading'\|'ready'\|'failed', error? }` |
| `typst/fontsChanged` | server → client | system font indexing finished; document recompiled |
| `typst/engineMissing` | server → client | `wasm/` is not built. The client shows the exact command rather than a stack trace |

### 7.3 Extension host and webview

Typed discriminated unions in `src/preview/messages.ts`, imported by both sides — the repo's existing
convention. Detailed in [preview.md §3](preview.md#3-update-protocol).

---

## 8. File layout

```
extensions/typst-ultra/
  package.json                 # language, grammar, commands, keybindings, settings, customEditors
  tsdown.config.mts            # three bundles: host (cjs/node), server (cjs/node), webview (esm/browser)
  .vscodeignore                # copied from scripts/templates per repo convention
  LICENSE.md                   # NO LICENSE + compiler, typstyle, and font notices
  THIRD-PARTY-NOTICES.md       # generated by `pnpm run licenses` (cargo-about), committed
  README.md
  language-configuration.json
  syntaxes/typst.tmLanguage.json
  assets/fonts/                # typst default font set, 9.2 MB (see proposal.md §6.3)
  src/
    extension.ts               # activation, commands
    client.ts                  # LanguageClient construction, server lifecycle, restart
    config.ts                  # settings → initializationOptions + didChangeConfiguration
    compileRoot.ts             # the two modes, status bar, QuickPick, pin suggestion
    status.ts                  # compile state, package status, heap watchdog
    export.ts                  # export commands, save dialogs
    exportPath.ts              # $dir/$name/$root templating — pure, so it tests without vscode
    template.ts                # scaffold from a Universe template (P4-14)
    preview/
      manager.ts               # panel lifecycle, follow/lock/retarget
      customEditor.ts          # typstUltra.preview custom editor provider
      messages.ts              # typed host ⇄ webview protocol + validation guards
      sync.ts                  # two-way scroll/cursor sync, loop guards
      html.ts                  # the webview document and its CSP
  server/
    main.ts                    # JSON-RPC loop, HostServices, wasm bootstrap, debounce
    vfs.ts                     # readFile/listDir with root confinement
    packages.ts                # Universe download, untar, cache, template manifests
    fonts.ts                   # bundled + system font discovery, on-disk index cache
  webview/
    index.ts                   # bootstrap, message loop, chrome
    pageList.ts                # virtualized page rendering, patch applier, SVG adoption
    styles/preview.css
  wasm/                        # built by build:wasm (gitignored, ships in the VSIX)
  dist/                        # tsdown output
```

Two placements differ from the sketch, both because of where the work actually has to happen:

- **`fonts.ts` is in `server/`, not `src/`.** Font *bytes* are fetched by a synchronous callback from
  inside a compile, so the index has to live in the server process. The extension host never sees a font.
- **`util.ts` never appeared.** Nothing turned out to be generic enough to want it, and a file that exists
  to hold leftovers attracts leftovers.

```
crates/typst/
  typst-session/               # World, VFS/font/package ports, compile session, export
  typst-lsp-core/              # LSP dispatch + IDE features
  typst-preview-core/          # page SVG, hashing/diff, jump mapping
  typst-lsp-wasm/              # #[wasm_bindgen] surface — the only WASM-aware crate
```

---

## 9. Failure modes and what the user sees

| Failure | Detection | User-visible behaviour |
| --- | --- | --- |
| `wasm/` missing (forgot `build:wasm`) | `require` throws at server start | Status bar shows "Typst: engine not built"; output channel prints the exact `pnpm run build:wasm` command. Same graceful degradation as [log-viewer](../../../../extensions/log-viewer/src/wasm.ts) |
| Compiler panic | server process exits non-zero | Client auto-restarts once, then offers a manual restart; the offending document version is logged |
| Compile exceeds a time budget | wall-clock check after the compile returns | Warning notification once per document, suggesting `typstUltra.compile.when: "onSave"` |
| Heap over threshold | `memory_size` polled after each compile | Status-bar item turns yellow with a restart affordance |
| Package download fails | `typst/packageStatus` | Diagnostic on the `#import` line with the reason; retry on next compile |
| A font referenced by the document is missing | `FontBook` lookup miss | Typst's own warning, surfaced as a normal diagnostic |
| Preview webview loses its state (reload) | `WebviewPanelSerializer` | Panel and mode restored; pages re-requested from scratch with an empty `knownHashes` |
