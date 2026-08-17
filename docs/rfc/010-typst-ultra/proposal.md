# RFC 010: Typst Ultra — a WASM Typst language server and live preview

**Status**: Draft, awaiting review
**Date**: 2026-08-17
**Extension name**: `wx-vsce-typst-ultra`
**Rust crates**: `crates/typst/typst-session`, `crates/typst/typst-lsp-core`, `crates/typst/typst-preview-core`, `crates/typst/typst-lsp-wasm` (all new)
**References**: [`temp/typst`](../../../temp/typst) — typst 0.15.1, Apache-2.0; [`temp/tinymist`](../../../temp/tinymist) — tinymist 0.15.4-rc1, Apache-2.0
**Affected components**: `extensions/typst-ultra` (new), `crates/typst/` (new), root `Cargo.toml` (workspace members)

---

## 1. Motivation

[Typst](https://typst.app) is a markup-based typesetting language — a modern LaTeX replacement with a real
programming language attached. It compiles fast, has good error messages, and its compiler is a set of
clean, published, permissively-licensed Rust crates. What it does not have is first-party editor tooling.

Today a VSCode user has two options:

1. **[tinymist](https://github.com/Myriad-Dreamin/tinymist)** — excellent and comprehensive: LSP,
   preview, DAP, export, testing, templates, a package manager UI. It is also ~25 Rust crates and a
   3,765-line TypeScript extension, it ships a **per-platform native binary** (the VSIX either bundles
   one target or downloads at install time), and — the part that matters most for anyone building on it —
   it depends on a **patched fork of the typst compiler**:

   ```toml
   # temp/tinymist/Cargo.toml:326
   [patch.crates-io]
   typst = { git = "https://github.com/Myriad-Dreamin/typst.git", tag = "tinymist/v0.15.0" }
   # …and typst-eval, typst-html, typst-layout, typst-library, typst-macros,
   #    typst-pdf, typst-realize, typst-bundle — nine crates in total
   ```

   Every upstream typst release requires re-forking. That is a real cost, and it is a cost we would
   inherit wholesale by depending on their crates.

2. **[typst-lsp](https://github.com/nvarner/typst-lsp)** — the predecessor, now unmaintained and
   superseded by tinymist.

Neither runs without a platform-specific binary. This repo has spent nine extensions establishing that a
Rust crate compiled to WASM and loaded from Node is a *better* distribution story than native binaries:
one artifact, no per-platform CI matrix, no install-time download, no code-signing, no "unsupported
architecture" bug reports.

The question this RFC answers is whether that story extends to something as heavy as a typesetting
compiler. **It does.** A working prototype (documented in full in [spike.md](research/spike.md)) establishes:

| Claim | Measured |
| --- | --- |
| Upstream typst 0.15.1 builds for `wasm32-unknown-unknown` **with no patches** | ✅ 304 crates, clean |
| Compile → layout → SVG → PDF runs correctly in Node under WASM | ✅ |
| `typst-ide` completion / hover / definition work in WASM | ✅ 181 completions, hover, goto-def |
| `typstyle` formatting works in WASM | ✅ |
| Incremental recompile, 30-page document | **6 ms** (p95 9 ms) |
| Incremental recompile, 75-page document | **15 ms** (p95 18 ms) |
| Cold compile, 75-page document | **262 ms** |
| WASM heap, 75-page document | **106 MB** |
| Artifact size | **22 MB** wasm, **8.4 MB** gzipped |

6 ms keystroke-to-recompile on a 30-page document, from a single portable artifact, is a good place to
build from.

## 2. Goals and Non-Goals

### Goals

1. **A Typst language server written in Rust, compiled to one WASM artifact**, working identically on
   macOS/Linux/Windows and on x64/arm64, with no install-time download and no native binary.
2. **Built on unmodified upstream typst crates** from crates.io. No fork, no `[patch.crates-io]`. Upgrading
   typst must be a version bump plus whatever API drift the compiler introduces, never a rebase of a fork.
3. **The core IDE feature set**: diagnostics, completion, hover, goto-definition, references, rename,
   document/workspace symbols, semantic tokens, selection ranges, folding ranges, document links,
   formatting, and inlay hints. See [lsp-features.md](design/lsp-features.md) for the per-feature breakdown.
4. **A live preview** that updates as you type, renders pages as SVG, and syncs **both ways** with the
   editor (cursor → page position, click on page → source position) using upstream's
   `typst_ide::jump_from_cursor` / `jump_from_click`.
5. **A custom editor** registration so a `.typ` file can be opened directly as a rendered document
   (`priority: "option"`, opt-in through `workbench.editorAssociations`) — the same pattern as
   [markdown-preview-ultra](../009-markdown-preview-ultra/proposal.md).
6. **Export** to PDF, SVG, and PNG, all in-process through `typst-pdf` / `typst-svg` / `typst-render`.
7. **Typst Universe packages** (`#import "@preview/cetz:0.4.2"`) resolved, downloaded, and cached by the
   host, sharing typst-cli's standard cache directory so nothing is downloaded twice.
8. **System font discovery** plus the bundled default font set, so output matches `typst compile`
   byte-for-byte out of the box.
9. **Clean crate boundaries** so the engine is testable natively with `cargo test`, with WASM bindings
   isolated in one thin crate — the repo's established engine/adapter split.

### Non-Goals

- **Beating tinymist on feature count.** Tinymist has a symbol picker, a font browser, a template gallery,
  DAP debugging, coverage profiling, a test runner, `tinymist.lock` project resolution, and a
  drag-and-drop/paste asset pipeline. We are building the ~20% of the surface that covers ~95% of daily
  editing, and doing it on a maintenance base we can actually carry.
- **Forking typst.** If a feature genuinely requires compiler internals that upstream does not export, the
  feature is cut or the export is proposed upstream. This is the constraint that makes the whole project
  affordable, and it is not negotiable within this RFC.
- **DAP / debugging**, **coverage**, **profiling flamegraphs**, **`typst test`**.
- **HTML export.** `typst-html` comes along as a transitive dependency; we do not surface it. (Cheap to
  add later — see §10.)
- **A web extension (vscode.dev).** The design deliberately runs the WASM in a Node child process
  (see [architecture.md §2](design/architecture.md#2-why-a-child-process-and-not-the-extension-host)). A browser
  build is possible later since the artifact is already WASM, but it is not a Phase-1 concern. Every other
  extension in this repo has the same constraint.
- **Editing the document from the preview.** The preview is strictly read-only, per RFC 009's principle.
- **A LaTeX/Word/Markdown importer.**
- **Multi-root project graphs beyond one compile root.** One workspace folder, one root path, one main
  file (pinnable). Tinymist's `tinymist.lock` model is explicitly out of scope.

## 3. What We Take from Upstream — and What We Write Ourselves

The single most important design fact: **`typst-ide` already exists**, is published, and is written by the
typst authors. It gives us the semantically hard half of a language server for free.

| Capability | Source | Notes |
| --- | --- | --- |
| Parsing, incremental reparse | `typst-syntax` | `Source::edit(range, text)` reparses incrementally |
| Compile, layout, diagnostics | `typst`, `typst-layout` | `typst::compile::<PagedDocument>(world)` → `Warned<SourceResult<T>>` |
| Completion (181 items at a bare cursor) | `typst_ide::autocomplete` | Context-aware: code, markup, math, params, imports, labels, packages |
| Hover | `typst_ide::tooltip` | Named-param docs, font info, label preview, import target |
| Goto-definition | `typst_ide::definition` | Local bindings, imports, std-library items |
| Preview ↔ source sync | `typst_ide::jump_from_click`, `jump_from_cursor` | SyncTeX-equivalent, both directions, upstream |
| Symbol/label analysis | `typst_ide::named_items`, `analyze_labels`, `deref_target` | The building blocks for references/rename/symbols |
| Syntax → token tags | `typst_syntax::highlight` → `Tag` | 22 tags; the semantic-token source of truth |
| SVG rendering | `typst-svg` | `svg(page, opts)` per page; `svg_merged` for whole-document |
| PNG rendering | `typst-render` | tiny-skia pixmap |
| PDF export | `typst-pdf` | krilla-based; +2 MB on the WASM artifact |
| Formatting | `typstyle-core` 0.15.1 | Also has a `partial` module for range formatting |
| Default fonts | `typst-assets` files | Shipped as VSIX assets, **not** embedded in the WASM (§6.3) |

What we write:

| Capability | Why it is ours |
| --- | --- |
| `World` implementation + VFS | Upstream's `typst-kit` `SystemFiles` needs `std::fs`; WASM has none. We implement `World` over host callbacks ([crates.md §2](design/crates.md#2-typst-session)) |
| Font provider | Same reason. Plus lazy loading, so system fonts don't sit in the WASM heap |
| Package resolution | `typst-kit`'s downloader is `ureq`-based. Ours is Node-side, async, with a "recompile when it lands" protocol |
| LSP protocol layer | Dispatch, state, capabilities, cancellation |
| References, rename, document symbols, workspace symbols, folding, selection ranges, document links, inlay hints | `typst-ide` gives the primitives; these are syntax-tree walks we write |
| Semantic tokens | Mapping `typst_syntax::Tag` → LSP token types, with delta encoding |
| Per-page render + diff protocol | The preview's incremental update mechanism |
| The extension host and webview | All TypeScript, ours |

Explicitly **not** taken from tinymist: any code. We reference their architecture (their web-worker LSP
build in [`temp/tinymist/editors/vscode/src/web/server.ts`](../../../temp/tinymist/editors/vscode/src/web/server.ts)
proved the WASM-LSP shape is viable, and their feature list is a good checklist), and we credit them in
`LICENSE.md` — see [references.md](research/references.md). Their TextMate grammar is a possible future
adoption, weighed and declined for now in [0007](decisions/0007-textmate-grammar.md).

## 4. Architecture in One Diagram

Full detail in [architecture.md](design/architecture.md); this is the shape.

```
┌─ VSCode ─────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  ┌─ Extension Host (Node) ──────────────────┐   ┌─ Webview (preview) ─────┐  │
│  │                                          │   │                         │  │
│  │  extension.ts                            │   │  page list (virtual)    │  │
│  │  ├── LanguageClient ─────────────┐       │   │  ├── <svg> per page     │  │
│  │  ├── PreviewManager ─────────────┼───────┼──▶│  ├── zoom / fit         │  │
│  │  ├── ExportCommands              │       │   │  ├── click → source     │  │
│  │  └── FontIndexCache              │       │   │  └── scroll sync        │  │
│  └──────────────────────────────────┼───────┘   └─────────────────────────┘  │
│                                     │ LSP over Node IPC                      │
│  ┌─ Server child process (Node) ────▼──────────────────────────────────────┐ │
│  │  server.js                                                              │ │
│  │  ├── JSON-RPC loop (vscode-languageserver)                              │ │
│  │  ├── host services (SYNCHRONOUS, called from Rust mid-compile):         │ │
│  │  │     readFile(path) · listDir(path) · fontData(index) · today()       │ │
│  │  ├── async services: package download, font indexing                    │ │
│  │  └── require('typst_lsp_wasm.js')  ← 22 MB WASM                         │ │
│  │        ┌──────────────────────────────────────────────────────────────┐ │ │
│  │        │ typst-lsp-wasm    thin #[wasm_bindgen] surface               │ │ │
│  │        │ typst-lsp-core    dispatch + IDE features (lsp-types)        │ │ │
│  │        │ typst-preview-core  page SVG, page hashing, jump mapping     │ │ │
│  │        │ typst-session     World, VFS, fonts, packages, compile       │ │ │
│  │        │   └── upstream: typst · typst-ide · typst-svg · typst-pdf    │ │ │
│  │        │                 typst-render · typst-syntax · typstyle-core  │ │ │
│  │        └──────────────────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
```

Three properties worth naming:

- **The server owns all typst state.** The extension host holds no document model. The preview is fed by
  the same server that answers LSP requests, over custom `typst/*` requests, so the preview and the
  diagnostics can never disagree about what the document is.
- **IDE requests never wait on a compile.** Completion, hover, and definition are answered from the
  incrementally-maintained syntax tree plus the *last completed* document. A compile in flight never
  blocks a keystroke. ([architecture.md §5](design/architecture.md#5-concurrency-model-one-thread-two-clocks))
- **The Rust side never does I/O.** Every file read, font byte, and package fetch crosses the WASM
  boundary through a host callback — verified working synchronously mid-compile in the spike
  ([spike.md §6](research/spike.md#6-synchronous-host-vfs-callbacks)).

## 5. The Rust Crates

Four crates under `crates/typst/`, detailed in [crates.md](design/crates.md).

| Crate | Responsibility | Depends on WASM? |
| --- | --- | --- |
| `typst-session` | `World` impl, VFS/font/package ports, compile session, incremental edit, cache eviction, export | No — pure Rust, natively testable |
| `typst-lsp-core` | LSP dispatch + all IDE features, `lsp-types` shapes | No |
| `typst-preview-core` | Per-page SVG, page hashing/diff, cursor↔page mapping | No |
| `typst-lsp-wasm` | `#[wasm_bindgen]` surface, host-callback glue | Yes — and *only* this crate |

The ports (`FileProvider`, `FontProvider`, `PackageProvider`) are traits. `typst-lsp-wasm` implements them
over JS callbacks; the test suites implement them over `std::fs`. That means **the entire server is
testable with `cargo test` on the host**, with WASM reserved for integration tests. This is the same split
RFC 007/008 established for `log-engine`.

Workspace registration needs a small change, because `members = ["crates/*"]` chokes on a grouping
directory that has no `Cargo.toml` (verified). The working form — explicit members override the exclude:

```toml
[workspace]
members = [
  "crates/*",
  "crates/typst/typst-session",
  "crates/typst/typst-lsp-core",
  "crates/typst/typst-preview-core",
  "crates/typst/typst-lsp-wasm",
]
exclude = ["crates/typst"]
resolver = "3"
```

## 6. Three Decisions That Shape Everything

### 6.1 Unmodified upstream typst

Tinymist patches nine typst crates to export internals. We accept a smaller feature set in exchange for a
version bump being a version bump. Where a feature needs something upstream does not expose, the options
in order are: (a) rebuild it from what *is* exposed, (b) cut the feature, (c) send a PR upstream. Forking
is not on the list. §10 lists the features currently known to be affected.

### 6.2 One WASM artifact instead of per-platform binaries

| | tinymist | Typst Ultra |
| --- | --- | --- |
| Artifact | native binary per (os, arch) | one `.wasm` |
| VSIX | per-platform VSIX, or download on activation | single VSIX, ~14 MB |
| CI | cross-compilation matrix | `cargo build --target wasm32-unknown-unknown` |
| Code signing | required on macOS/Windows | none |
| Cold compile, 75 pages | faster (native, multi-threaded) | 262 ms (measured) |
| Keystroke recompile, 30 pages | ~13 ms (measured, uncontrolled) | 6 ms (measured) |

The performance tax is real but small in absolute terms, and it buys away an entire class of distribution
problems. `rayon` compiles and runs on `wasm32-unknown-unknown` — single-threaded, no panic (verified) —
so typst's parallel layout degrades to sequential rather than failing. That is most of the cold-compile
gap.

The native column is deliberately vague because the spike's native baseline was **not a controlled
comparison** ([spike.md §4.3](research/spike.md#43-native-baseline-same-document-same-machine)); it is
enough to establish that the tax is a small constant factor, not an order of magnitude.

### 6.3 Fonts ship as VSIX assets, not inside the WASM

typst's default font set is 9.5 MB (Libertinus Serif, New Computer Modern + NCM Math, DejaVu Sans Mono, and
the Foxit PDF base-14 set). Embedding them via `typst-assets/fonts` would put all 9.5 MB in the WASM
artifact and in the WASM heap, permanently.

Instead they ship as plain files under `assets/fonts/`, and the server reads them through the same host
callback the VFS uses. Fonts are loaded in two stages: the host extracts `FontInfo` metadata to build the
`FontBook` at startup, then hands over the actual bytes only for faces the document really uses. That also
makes **system font discovery** affordable — a user with 400 MB of installed fonts gets them indexed
(cached on disk by path+mtime+size), without 400 MB entering the WASM heap.

VSIX budget: ~8.4 MB compressed WASM + ~5 MB compressed fonts ≈ **14 MB**. Large for this repo, small next
to any extension shipping native toolchains.

## 7. Extension Surface

Full listing in [lsp-features.md §7](design/lsp-features.md#7-commands-and-settings) and
[preview.md](design/preview.md). Summary:

- **Language contribution**: one `typst` id claiming **`.typ` and `.typc`**
  ([0009](decisions/0009-file-extensions.md)), a language-configuration file, and a modest hand-written
  TextMate grammar for the pre-semantic-token paint. Real coloring comes from semantic tokens produced by
  the actual parser ([0007](decisions/0007-textmate-grammar.md)).
- **11 commands**: show preview / to side, sync-to-cursor, pin/unpin the compile root, export
  (PDF/SVG/PNG), restart server, show log, clear package cache, toggle preview color inversion.
- **24 settings** under `typstUltra.` — root path, main file, fonts (system/paths), packages
  (enabled/registry/cache), compile trigger and debounce, preview (scroll sync, cursor indicator, invert
  colors, background, render mode), diagnostics, semantic tokens, formatter (mode/width/indent), inlay
  hints, export output path, trace, and two memory knobs.
- **A compile-root indicator** in the status bar, showing whether the server is following the focused file
  or compiling a pinned main file, with a QuickPick to switch ([0008](decisions/0008-compile-root.md)).
- **A custom editor** `typstUltra.preview` at `priority: "option"` for `.typ`.

## 8. Performance Targets

Derived from measured spike numbers ([spike.md §4](research/spike.md#4-compile-latency)), with headroom for the
real server's extra bookkeeping.

| Scenario | Target | Spike measurement |
| --- | --- | --- |
| Extension activation (no `.typ` open) | < 5 ms | server starts lazily |
| Server start → `initialized` | < 400 ms | WASM instantiate + font index |
| Cold compile, 10-page document | < 150 ms | 81 ms |
| Cold compile, 75-page document | < 300 ms | 262 ms |
| Keystroke → diagnostics, 30-page document | < 50 ms | 6 ms compile |
| Keystroke → diagnostics, 30-page document (p95) | < 80 ms | 9 ms |
| Keystroke → preview repaint, 30-page document | < 120 ms | 6 ms compile + ~5 ms/page SVG + IPC (**IPC estimated**) |
| Completion / hover / definition | < 30 ms | syntax-tree only; never waits on compile |
| Format, 30-page document | < 50 ms | typstyle is a pure syntax pass |
| WASM heap, 30-page document | < 150 MB | 55 MB |
| WASM heap, 75-page document | < 250 MB | 106 MB |

All figures are at the default `memory.evictAge: 1`. Tail latency, not the median, is what a typist feels,
which is why p95 is a target rather than a footnote — at eviction age 10 the p95 at 75 pages is 278 ms
against age 1's 18 ms ([0005](decisions/0005-cache-eviction-policy.md)).

Two caveats worth carrying forward:

- **The preview repaint budget is ~75% estimate.** Roughly 50 ms of it is JSON-RPC, IPC, `postMessage`, and
  DOM parsing, none of which has been measured. Closing that is the first task of Phase 3
  ([P3-05](tasks/phase-3-preview.md)).
- **Getting the compile/evict *ordering* wrong costs 50×**, which the spike found the hard way. The
  invariant is enforced structurally in `typst-session`, not by convention. See
  [architecture.md §6](design/architecture.md#6-memory-and-the-comemo-cache).

## 9. Security

1. **No document-controlled execution.** Typst is a pure language with no shell-out, no FFI, and no
   network access from documents. The only I/O a document can request is reading files, which we mediate.
2. **VFS confinement.** Every host `readFile` is resolved against the project root or the package cache
   and rejected if it escapes. `typst_syntax::VirtualPath` normalizes `..` before it reaches us; the host
   re-checks anyway.
3. **Package downloads** go only to the configured registry (default `packages.typst.org`) over HTTPS,
   are extracted with a path-traversal-checked untar, and land in a cache directory. Downloading is
   default-on but gated by `typstUltra.packages.enabled`.
4. **Webview**: strict CSP (`default-src 'none'`, nonce'd scripts, `img-src` limited to the webview
   source and `data:`), a typed and validated message protocol with `seq` staleness checks, no remote
   loads. SVG from the compiler is engine-generated, not user HTML — but it is still inserted as
   markup, so [preview.md §4](design/preview.md#4-svg-injection-and-why-it-is-safe) documents exactly why that
   is safe and what we strip.
5. **No credential or token access of any kind.**

## 10. Known Limitations from the No-Fork Constraint

Honest accounting of what tinymist gets from its patches that we will not have at Phase 3:

| Feature | Why it needs more than upstream exposes | Our answer |
| --- | --- | --- |
| Signature help with live argument values | Needs evaluation-time introspection of partially-typed calls | Derive signatures from `Func` metadata; no live values. Good enough for ~90% of cases |
| Rename across packages | Package sources are read-only | Rename within project files only; refuse with a clear message otherwise |
| "Find all references" for std-library items | Would need a whole-universe index | Scoped to the compile root's file graph |
| Inline value preview on hover | `typst::trace` exists upstream but is expensive | Phase 4 experiment behind a setting |
| Content-hint-free preview positioning | Tinymist's `no-content-hint` patch removes a marker that perturbs layout | Use upstream `jump_from_cursor`/`jump_from_click`, which are designed for exactly this and need no patch |
| HTML export target | Upstream `typst-html` is available but the export UX is a project of its own | Deferred; the dependency is already linked in |

None of these are load-bearing for daily editing.

## 11. Open Questions

All eight questions raised in the first draft are now resolved. Each has a decision record carrying the
reasoning, the consequences, and — importantly — the conditions under which it should be reversed. This
section is the index; [decisions/](decisions/README.md) is the content.

| # | Question | Resolution | Record |
| --- | --- | --- | --- |
| 1 | Hand-write a TextMate grammar, or adopt tinymist's generated one? | Hand-write a minimal one; semantic tokens from the real parser do the actual coloring | [0007](decisions/0007-textmate-grammar.md) |
| 2 | Preview as SVG, or PNG for low memory? | SVG by default; **PNG accepted as a planned low-memory mode** (`preview.renderMode`, Phase 4) | [0006](decisions/0006-preview-rendering.md) |
| 3 | Preview in the same process as the LSP? | Share one process; isolation comes from "never block an IDE request on a compile" | [0003](decisions/0003-server-in-child-process.md) |
| 4 | Expose the comemo eviction age, or hard-code it? | **Expose it, default `1`** — the low-latency setting, which measurement showed is also the low-memory one | [0005](decisions/0005-cache-eviction-policy.md) |
| 5 | Bundle the default fonts, or download them? | Bundle. A document that renders differently from `typst compile` is a bug report we cannot close | [0004](decisions/0004-bundle-default-fonts.md) |
| 6 | Focused file as compile root, or an explicit main file? | **Both** — follow the focused file by default, pin a main file for projects, with a status bar and a one-shot suggestion bridging them | [0008](decisions/0008-compile-root.md) |
| 7 | Extension name | `typst-ultra`, crates `typst-{session,lsp-core,preview-core,lsp-wasm}` | [0010](decisions/0010-naming.md) |
| 8 | Claim `.typc` as well as `.typ`? | **Both**, under one `typst` language id, both parsed as markup — because [measurement showed](research/spike.md#8-typc-and-code-mode) the compiler ignores the distinction | [0009](decisions/0009-file-extensions.md) |

Two of these answers were changed by measurement rather than argument, which is the main reason the spike
was worth running:

- **Q4** looked like a latency-vs-memory trade-off. The sweep found it is not a trade-off at all: eviction
  age 1 is simultaneously the fastest (p95 18 ms vs 278 ms at 75 pages) and the smallest (106 MB vs
  394 MB). The intuition that a bigger cache is faster was simply wrong here.
- **Q8** looked like a question about mirroring tinymist's `typst-code` language. Testing what the compiler
  actually does showed that a genuine code-mode `.typc` **fails to import** — so mirroring the convention
  would have made the editor disagree with the compiler.

New questions should be raised as records with status **Proposed**, not appended here.

## 12. Phased Plan

Each phase is independently shippable and ends at a milestone that can be demonstrated.

### Phase 1 — Server skeleton and diagnostics
`crates/typst/typst-session` (World, VFS/font ports, compile session, eviction policy) and
`typst-lsp-wasm`. The Node server child process with synchronous host callbacks. `vscode-languageclient`
wiring, language contribution, minimal TextMate grammar, bundled fonts. LSP surface: `didOpen`/`didChange`/
`didSave`/`didClose`, **diagnostics**, and nothing else.
**Milestone: open a `.typ` file, see typst's real errors and warnings inline, as you type.**

### Phase 2 — The IDE feature set
`typst-lsp-core` in full: completion, hover, goto-definition, references, rename, document + workspace
symbols, semantic tokens, selection ranges, folding, document links, formatting via `typstyle-core`.
System font discovery with the on-disk index cache. Package resolution and download.
**Milestone: `#import "@preview/cetz:0.4.2"` completes, resolves, and jumps to definition.**

### Phase 3 — Live preview
`typst-preview-core`: per-page SVG, page hashing, patch protocol. The preview webview: virtualized page
list, zoom/fit, two-way scroll sync, click-to-source, cursor indicator, color inversion. Custom editor
registration. Export commands (PDF/SVG/PNG).
**Milestone: type in the editor, watch the page repaint in under 120 ms; click a word in the preview and
land on it in the source.**

### Phase 4 — Polish and stretch
Inlay hints, code actions, code lenses, on-enter list continuation, `svg_merged` whole-document export,
optional PNG preview mode, an `Instant`-free trace/profiling view, HTML export, a browser-worker build for
vscode.dev.

## 13. Testing

Following repo practice — tests live with the code they test, and there are no scratch scripts.

- **`typst-session` (Rust)**: the port traits are implemented over `std::fs` in `tests/`, so the whole
  compile pipeline is exercised natively. Fixture corpus of `.typ` documents, snapshot-tested via `insta`
  for diagnostics (message + span). Property test: applying an arbitrary edit sequence through
  `Source::edit` yields the same tree as a fresh parse.
- **`typst-lsp-core` (Rust)**: per-feature snapshot tests over fixtures with a cursor marker — the shape
  `typst-ide`'s own tests use ([`temp/typst/crates/typst-ide/src/tests.rs`](../../../temp/typst/crates/typst-ide/src/tests.rs)).
  Semantic-token delta encoding gets dedicated unit tests.
- **`typst-preview-core` (Rust)**: page-hash diff correctness (applying patches to the previous page list
  reproduces the new one), and round-trip `jump_from_cursor` → `jump_from_click` on a fixture document.
- **Extension (vitest, in-tree `*.test.ts`)**: message-protocol guards, page-patch applier, scroll-map
  interpolation, font-index cache invalidation, package-path traversal rejection.
- **Integration (Rust, `wasm32` target)**: one `wasm-pack test --node` suite that loads the real artifact
  and compiles a fixture, so the binding layer is covered.
- **Manual acceptance checklist** in the PR: a real paper (multi-file, bibliography, figures), a
  CeTZ-heavy document, a 200-page document, package download from a cold cache, system fonts on all three
  platforms.

## 14. Risks

| Risk | Likelihood | Mitigation |
| --- | --- | --- |
| WASM heap growth on very large documents (106 MB at 75 pages, never returned to the OS) | Low–Medium | Child-process isolation; compile-then-evict at age 1 ([0005](decisions/0005-cache-eviction-policy.md)); server restart above a configurable ceiling with a status-bar notice. Downgraded from High once the eviction sweep cut the figure from 394 MB |
| Upstream typst API churn each release | High | It is a compiler under active development; but the surface we use (`World`, `compile`, `typst-ide`, `typst-svg`) is the *published* surface, which is exactly what upstream keeps stable. Contrast with tinymist, which depends on unpublished internals |
| Per-page SVG (386 KB) is heavy over IPC | Medium | Virtualized rendering (visible pages only) + per-page hash diffing means a keystroke typically ships one page. Measured, with a PNG fallback as a documented escape hatch (§11.2) |
| `typst-ide` completions are less rich than tinymist's | Medium | Accepted. 181 completions at a bare cursor is already good. Postfix/UFCS completions are a tinymist extension we can rebuild later if missed |
| `rayon` behaviour changes on `wasm32` in a future release | Low | Currently runs single-threaded without panicking (verified). Pinned via `Cargo.lock`; a CI job builds the WASM target on every PR |
| Font indexing of a large system font directory is slow on first run | Medium | Two-stage load: bundled fonts first (instant, correct for most documents), system fonts indexed in the background with an on-disk cache, then a recompile |
| Package download failures (offline, proxy, registry down) | Medium | Cache-first; clear diagnostic on the `#import` line naming the package and the reason; `typstUltra.packages.enabled: false` for air-gapped setups |
| 14 MB VSIX deters installs | Low | Comparable extensions ship more. Revisit as §11.5 if it becomes a complaint |
| `vscode-languageclient` is a new dependency for this repo | Low | It is the standard, Microsoft-maintained client; the alternative is hand-rolling ~20 providers |

## 15. Summary

Build `extensions/typst-ultra` on four new Rust crates under `crates/typst/`, compiled to a single
`wasm32-unknown-unknown` artifact and run in a Node child process as a language server. Use **unmodified
upstream typst 0.15.1 crates** — `typst`, `typst-ide`, `typst-syntax`, `typst-svg`, `typst-render`,
`typst-pdf`, plus `typstyle-core` — which a working prototype confirms build and run in WASM with 7 ms
incremental recompiles on a 30-page document and a 22 MB artifact. The extension host is a
`vscode-languageclient` plus a preview webview that receives per-page SVG patches and syncs both ways with
the editor using upstream's own jump machinery.

Relative to tinymist this drops DAP, coverage, testing, templates, the symbol/font browsers, and the lock
file — and with them a patched compiler fork and a per-platform binary matrix. What is left is a language
server and a live preview that install as one file, work identically everywhere, and can follow upstream
typst releases with a version bump.
