# RFC 002: Proto3 IntelliSense via a Rust Language Analyzer

**Status**: Draft
**Date**: 2026-04-21
**Extension name**: `wx-vsce-protobuf` (single extension — grammar + analyzer together)
**Rust crate**: `crates/proto3-analyzer`

> **History note (2026-04-21).** An earlier revision of this RFC proposed a
> two-extension split (`wx-vsce-protobuf` for grammar, `wx-vsce-protobuf-intellisense`
> for analyzer) modeled on `ms-python.python` + `ms-python.vscode-pylance`. On
> review the split was scrapped: that pattern exists because Pylance is a
> separately-licensed product from a different team. Both of ours ship from
> this repo under the same license and author — the split was pure overhead
> (marketplace dependency resolution, version skew, two `package.json`s to
> maintain, two activation paths). One extension owns both.

---

## 1. Motivation

Protocol Buffers (proto3) remains a core interchange and RPC schema language across backend services, yet the VSCode ecosystem for `.proto` editing is notably thinner than for mainstream programming languages. What exists is, broadly:

1. **`zxh404.vscode-proto3`** ([github.com/zxh0/vscode-proto3](https://github.com/zxh0/vscode-proto3)) — syntax highlighting, snippets, `protoc` invocation, field-renumber commands. **Deprecated / unmaintained**; explicitly redirects users to successors. No semantic analysis.
2. **`pbkit.vscode-pbkit`** ([marketplace.visualstudio.com/items?itemName=pbkit.vscode-pbkit](https://marketplace.visualstudio.com/items?itemName=pbkit.vscode-pbkit)) — highlighting, basic go-to-definition, auto-complete. Written in TypeScript using the `pbkit` parser. Import-path handling is limited; no `buf.yaml` awareness.
3. **`DrBlury.protobuf-vsc`** ([github.com/DrBlury/protobuf-vsc-extension](https://github.com/DrBlury/protobuf-vsc-extension)) — the most feature-complete option today. Positions itself as successor to zxh404's extension. In-process TypeScript implementation, no shared LSP.
4. **Buf's official integration** (`buf lsp serve`, announced at [buf.build/blog/protobuf-lsp](https://buf.build/blog/protobuf-lsp), docs at [buf.build/docs/cli/editors-lsp/](https://buf.build/docs/cli/editors-lsp/)) — a Go-implemented LSP bundled inside the `buf` CLI. Tight BSR / `buf.yaml` integration. Requires users to install the `buf` binary and keep it on PATH. No support for workspaces that do not use Buf.
5. **Third-party Rust LSPs** — `protols` ([github.com/coder3101/protols](https://github.com/coder3101/protols)) using tree-sitter + protoc; `pbls` ([github.com/rcorre/pbls](https://github.com/rcorre/pbls)) using `protobuf-parse`; `protobuf-language-server` ([github.com/lasorda/protobuf-language-server](https://github.com/lasorda/protobuf-language-server)) in Go. See §14 for full matrix.

Gaps we see across this landscape:

- **No first-class "plug and play" VSCode experience that does not require a side-installed binary** (`buf` CLI, `protoc`, or `cargo install protols`). Users expect an extension from the marketplace to work after install.
- **Inconsistent handling of non-Buf workspaces.** `buf lsp serve` assumes `buf.yaml`; `pbls` uses a hand-rolled `.pbls.toml`; the DrBlury extension has its own settings. None auto-resolves a googleapis-style include-path layout well.
- **Brittle import resolution.** Extensions routinely fail to find `google/protobuf/descriptor.proto` etc. unless the user plumbs include paths manually.
- **No common story for Rust monorepos.** Rust backends that generate code via `prost-build` / `tonic-build` (which internally use `protox`) get code-gen diagnostics that don't agree with what the editor says.

**Why Rust.** This repo already ships a Rust-powered analyzer (`crates/wgsl-analyzer`, backing `wx-vsce-wgsl-shader`) built on `naga` and compiled to WASM. Rust gives us deterministic performance for workspace-wide analysis on large trees (googleapis is ~300 `.proto` files), a mature WASM toolchain, and the same build/publish pipeline the repo already uses. We deliberately do **not** take a runtime dependency on the existing third-party proto parsing crates (`protox-parse`, `protobuf-parse`, `protobuf-ast-parser`) — see §5 for the decision — and instead write the lexer, parser, and AST by hand inside `crates/proto3-analyzer`. Proto3's grammar is small and stable; owning the parser avoids being blocked by an upstream that is single-maintainer (`protox-parse`), explicitly unstable ("not meant to be used directly" — `protobuf-parse`), or shaped for batch compilation rather than editor use (fail-fast, no partial ASTs, spans hidden behind `SourceCodeInfo`).

**Why a new extension, not a fork.** None of the existing Rust LSPs are designed to run in-process in a VSCode extension host. Shipping `cargo install` as a prereq is a non-starter for marketplace distribution. Bundling a native binary per platform works but duplicates a lot of engineering. The WASM pipeline we already have for `wgsl-analyzer` gives us a single artifact that runs on every platform VSCode supports, including web (`vscode.dev`) if we keep the surface area disciplined.

**Shape of `wx-vsce-protobuf`.** A single extension owns everything:

- Registers the `proto3` language id, the `.proto` file association, the grammar, brackets/comments config, and snippets (the syntax-only surface that was already shipping at `0.1.0`).
- Bundles the Rust analyzer via `wasm-pack` and registers the LSP-style providers (DocumentSymbol, WorkspaceSymbol, Diagnostics, and the Phase 2+ providers below).
- Activates on `onLanguage:proto3`. Users who only want highlighting pay the activation cost once — a few milliseconds to load a ~350 KB WASM bundle — which is negligible compared to any real editing session.
- Graceful degradation: if the WASM bundle is missing (e.g. a dev checkout before `pnpm run build:wasm`), the grammar still works; the analyzer providers log a warning and no-op.

## 2. Design Goals

1. **Go-to-definition** -- Jump to the definition of a message, enum, enum value, field type, service, or RPC. Must work across files (same package and imported), across nested definitions, and into well-known types (`google/protobuf/*.proto`).
2. **Find-all-references** -- All usages of a symbol across the workspace, including within `rpc` return types and `map<K, V>` value types.
3. **Hover** -- Rich hover showing: qualified type name (`.google.protobuf.Timestamp`), leading comments, field number, field cardinality (`optional` / `repeated`), `map<K,V>` key/value kinds, oneof membership, and for well-known types a one-line description.
4. **Completion** -- Context-aware completions for (a) keywords at top-level and inside message/service blocks, (b) scalar types (`int32`, `string`, ...) and well-known types in field-type position, (c) symbol names from imports in any type position, (d) import-path completion inside `import "..."` (driven by include-path scan), (e) field-name completion inside `option (...) = { ... };` message-literal bodies, (f) reserved-name completion for `reserved "..."`.
5. **Diagnostics** -- Parse errors, unresolved imports, unknown type names, duplicate field numbers, reserved-number collisions, field numbers outside [1, 536870911] or inside the reserved [19000, 19999] range, duplicate message/enum/service names, conflicting oneof memberships, packed-encoding validity on scalar fields.
6. **Document symbols & outline** -- Hierarchical outline: packages → services & top-level messages/enums → fields / rpcs / nested types. Breadcrumb-friendly.
7. **Workspace symbols** -- Fuzzy search across the entire proto workspace.
8. **Rename** -- Safe rename of messages, enums, enum values, fields, services, and RPC methods across the workspace, limited to the language surface (does not touch generated code).
9. **Formatting (optional, Phase 4)** -- Either shell out to `buf format` / `clang-format` or implement a minimal in-crate pretty-printer.
10. **Import resolution across include paths** -- Resolve imports using (a) an explicit `proto3.includePaths` setting, (b) the directory of the open file, (c) auto-discovered `buf.yaml` / `buf.work.yaml`, (d) bundled well-known types fallback.
11. **Workspace-wide analysis** -- The entire discovered tree is analyzed, not just open files. Changes in one file invalidate and re-analyze dependents.
12. **Reasonable performance on real-world trees** -- Cold open of a googleapis-style ~300-file tree under 1.5 s on a modern laptop. Edit-to-diagnostic latency under 100 ms for a single-file edit, under 500 ms for a transitive re-check.
13. **Zero external prerequisites** -- The extension works immediately after install, with no requirement to install `protoc`, `buf`, or a Rust toolchain.
14. **Behavioral parity with canonical `protoc` / `prost-build` on valid inputs** -- A file that `protoc` or `prost-build` compiles cleanly must produce zero parse/resolution diagnostics from our analyzer. Error *locations* are not guaranteed byte-identical (we implement our own parser — see §5), but error *verdicts* on well-formed vs malformed inputs must agree. We pin this with a conformance suite that diffs our AST→descriptor lowering against `protoc --descriptor_set_out` on a fixture corpus (§16 Phase 1).

## 3. High-Level Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ VSCode Extension Host (Node.js)                                   │
│                                                                    │
│   extension.ts                                                     │
│     ├─ activate(context)                                           │
│     ├─ register DocumentSymbol / Hover / Definition / …            │
│     │   providers (thin wrappers around the analyzer)              │
│     └─ WorkspaceWatcher ─ fs.watch on **/*.proto + include paths   │
│                                                                    │
│   ┌────────────────────────────────────────────────────────────┐  │
│   │   AnalyzerBridge (TypeScript)                               │  │
│   │                                                              │  │
│   │     - require('./wasm/proto3_analyzer.js')                   │  │
│   │     - wraps a single long-lived Analyzer handle              │  │
│   │     - marshal/unmarshal (JSON over wasm-bindgen)             │  │
│   │     - emits VSCode Diagnostics / CompletionItems / …         │  │
│   └──────────────────┬─────────────────────────────────────────┘  │
│                      │                                              │
│                      │ synchronous JS ↔ WASM calls                  │
│                      ▼                                              │
│   ┌────────────────────────────────────────────────────────────┐  │
│   │   proto3-analyzer (Rust, compiled to WASM via wasm-pack)    │  │
│   │                                                              │  │
│   │   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │  │
│   │   │ Lexer /      │→ │ AST +        │→ │ Name         │     │  │
│   │   │ Parser       │  │ source-map   │  │ Resolution / │     │  │
│   │   │ (protox-     │  │ (per-file)   │  │ Symbol Index │     │  │
│   │   │  parse fork) │  └──────────────┘  │ (workspace)  │     │  │
│   │   └──────────────┘                    └──────┬───────┘     │  │
│   │         ▲                                     │             │  │
│   │         │                                     ▼             │  │
│   │   ┌──────────────┐                    ┌──────────────┐     │  │
│   │   │ Virtual FS   │                    │ Diagnostics  │     │  │
│   │   │ (file_set)   │                    │ Engine       │     │  │
│   │   │ + include    │                    └──────────────┘     │  │
│   │   │ path router  │                                          │  │
│   │   └──────────────┘                                          │  │
│   │                                                              │  │
│   │   Exposed API (JSON in, JSON out):                           │  │
│   │     - analyzer_new() -> handle                               │  │
│   │     - analyzer_set_include_paths(h, paths)                   │  │
│   │     - analyzer_update_file(h, uri, source)                   │  │
│   │     - analyzer_remove_file(h, uri)                           │  │
│   │     - analyzer_diagnostics(h, uri) -> Diagnostic[]           │  │
│   │     - analyzer_document_symbols(h, uri) -> Symbol[]          │  │
│   │     - analyzer_definition(h, uri, pos) -> Location | null    │  │
│   │     - analyzer_references(h, uri, pos) -> Location[]         │  │
│   │     - analyzer_hover(h, uri, pos) -> Hover | null            │  │
│   │     - analyzer_completion(h, uri, pos) -> CompletionItem[]   │  │
│   │     - analyzer_rename(h, uri, pos, new) -> WorkspaceEdit     │  │
│   └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

### 3.1 Transport Decision: WASM In-Process vs Native LSP Binary

Three plausible transports:

| Transport | Pros | Cons |
|-----------|------|------|
| **A. WASM (wasm-pack --target nodejs), loaded via `require`** — the `wgsl-analyzer` pattern | Single artifact across macOS/Linux/Windows/web. No binary download. Synchronous in-process calls with low marshal overhead. Works in `vscode.dev`. | No threads (wasm32-unknown-unknown). No direct filesystem access — extension must feed source text in. Memory pressure for very large trees lives inside the extension-host process. |
| **B. Native LSP binary + `vscode-languageclient` over stdio** — the `rust-analyzer` pattern | Full Rust ecosystem including rayon, tokio, `notify`-based file watching. Crash isolation from the extension host. Debuggable as a standalone process. | Per-platform builds (`-target darwin-arm64`, `linux-x64`, `win32-x64`, `alpine-x64` at minimum). `vsce package --target <triple>` multiplies CI steps by 4-6. Binary download fallback for unsupported platforms. Does not run on `vscode.dev`. |
| **C. Hybrid: WASM + in-webview LSP over message-channel transport** | Runs in-browser; uses `vscode-languageclient/browser`. | Adds a second serialization hop without meaningful benefit for our shapes. Complicates debugging. |

**Recommendation: A (WASM), matching the `wgsl-analyzer` precedent.**

Justification:

1. **Consistency with the repo.** `wgsl-analyzer` already proves the pattern works for a VSCode language extension with a Rust backend. The build pipeline (`wasm-pack build --target nodejs --out-dir ../../extensions/<name>/wasm`), the loader (`require(path.join(extensionPath, 'wasm', '<name>.js'))`), and the JSON-over-wasm-bindgen marshaling are already understood and have repo-local conventions.
2. **Distribution.** A single `.vsix` works everywhere. We do not want to run a per-platform build matrix for a v1 tool.
3. **Performance headroom.** A proto file is small. A parsed AST is small. The workspace is bounded by "what the user has in their repo" — typically a few hundred files max. WASM without threads is fast enough; a well-tuned hand-written proto3 parser comfortably handles a 500-line file in single-digit milliseconds, which is the shape of latency we need.
4. **Migration path.** If we ever hit WASM ceilings (e.g. need `rayon` for a 10K-file monorepo), we can add transport B behind a setting without re-architecting the Rust crate — the analyzer's public API is already a JSON-boundary request/response protocol that maps trivially to an LSP `Server`. The same crate can also be built into `crates/proto3-lsp` binary later.

**Trade-offs we accept.** The extension host must hold the entire workspace symbol index in its JS heap (the WASM linear memory). For googleapis-scale, back-of-envelope: ~300 files × ~2 KB of AST per file + ~1 KB per symbol ≈ 2 MB. Negligible. If a user opens a pathologically large internal schema (10K+ files, e.g. a monorepo including third-party vendored BSR modules) we will reconsider and cut over to B.

## 4. Key Components

### 4.1 Rust crate (`crates/proto3-analyzer`)

#### 4.1.1 Parser layer

Hand-written lexer + recursive-descent parser, living entirely inside `crates/proto3-analyzer`. No third-party proto-grammar crate is pulled in at parse time — the full pipeline from `&str` to our typed AST is owned by this crate (see §5 for the rationale and alternatives evaluated).

The pipeline is split into three stages so each can be tested and recovered from independently:

1. **Lexer** (`src/lexer.rs`) — scans the source into `Token` values tagged with a `ByteSpan`. Handles proto3's full lexical grammar: identifiers, dotted names (`foo.bar.Baz`), integer/float literals (decimal, hex, octal, with optional sign), string literals with C-style escapes (including concatenated adjacent string literals), block and line comments (retained as `Trivia` attached to the following token so hover can surface leading doc-comments), and punctuation. Unterminated strings, stray characters, and invalid escape sequences become `LexError` tokens rather than aborting the stream.

2. **Parser** (`src/parser/`) — recursive-descent, one entry per top-level production (`parse_file`, `parse_message`, `parse_enum`, `parse_service`, `parse_option`, `parse_field`, `parse_oneof`, `parse_map_field`, `parse_reserved`, `parse_extensions`). Every production consumes tokens through a single `Parser` cursor that tracks the current position and an error sink. On a syntactic mismatch the parser emits a diagnostic and calls `sync_to` with a set of recovery tokens — usually `{ SEMI, RBRACE, KW_MESSAGE, KW_ENUM, KW_SERVICE, KW_RPC, KW_OPTION, EOF }` — so one broken field does not blow away the rest of the file. This is the crucial property for editor use: a half-typed line must still leave the surrounding message parseable.

3. **AST** (`src/ast.rs`) — our own typed nodes, not `FileDescriptorProto`. Nodes own `ByteSpan` ranges for every identifier, type reference, and literal, which lets go-to-definition, references, and rename operate on exact source ranges without fishing positions out of `source_code_info`. When an external consumer (a future `cargo proto3-check` CLI, a build-tool integration) needs a Google-canonical descriptor, a small `ast::to_descriptor` adapter lowers the AST to `prost_types::FileDescriptorProto` — gated behind the optional `descriptor-adapter` Cargo feature and not shipped in the editor WASM build.

```rust
// crates/proto3-analyzer/src/parse.rs

/// The canonical parsed representation for a single .proto file.
pub struct ParsedFile {
    pub uri: FileUri,
    pub source: String,
    /// Typed AST owned by this crate. Every node carries a ByteSpan.
    pub ast: ast::File,
    /// Line/column ↔ byte-offset index used by every feature provider.
    pub spans: SpanTable,
    /// Parse-time diagnostics. Recoverable errors included; the parser
    /// never returns early on a single bad token.
    pub diagnostics: Vec<ProtoDiagnostic>,
}

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    let tokens = lexer::lex(&source);
    let mut parser = parser::Parser::new(&source, tokens);
    let ast = parser.parse_file();
    let (spans, diagnostics) = parser.finish();
    ParsedFile { uri, source, ast, spans, diagnostics }
}
```

**Error recovery strategy.** Recursive-descent with follow-set synchronization gives us per-statement recovery by construction:

- Top-level: on error, skip to the next `message` / `enum` / `service` / `import` / `option` / `package` / `;`.
- Inside a message body: skip to the next `;` or the matching `}`.
- Inside a field declaration: the full declaration is emitted as a best-effort `ast::FieldDecl` with any missing pieces marked `Missing`; downstream passes tolerate `Missing` so hover/completion on the *surrounding* file still works.
- The lexer never panics; lexical errors are preserved as tokens and surface as `PROTO0001-0003` diagnostics in the parser.

This gives us the full recovery story — including mid-statement partial ASTs — that a repair-pass wrapper around a fail-fast upstream parser could not deliver.

#### 4.1.2 Workspace / VFS

```rust
// crates/proto3-analyzer/src/vfs.rs

pub struct Workspace {
    /// All known files indexed by canonical URI.
    files: FxHashMap<FileUri, ParsedFile>,
    /// Search order for `import "..."` resolution.
    include_paths: Vec<IncludePath>,
    /// Bundled well-known types (google/protobuf/*.proto) baked into the WASM
    /// binary via include_str! at build time.
    well_known: &'static [(&'static str, &'static str)],
    /// Reverse index: which files import a given file.
    reverse_imports: FxHashMap<FileUri, FxHashSet<FileUri>>,
}

impl Workspace {
    pub fn update_file(&mut self, uri: FileUri, source: String) -> ChangedFiles {
        let parsed = parse(uri.clone(), source);
        let previous_imports = self.files.get(&uri).map(|f| f.imports().collect());
        self.files.insert(uri.clone(), parsed);
        self.rebuild_reverse_imports(&uri, previous_imports);
        self.compute_affected_files(&uri)
    }

    pub fn resolve_import(&self, importer: &FileUri, path: &str) -> Option<FileUri> {
        for inc in &self.include_paths {
            if let Some(uri) = inc.probe(path) { return Some(uri); }
        }
        if let Some(uri) = self.probe_well_known(path) { return Some(uri); }
        None
    }
}
```

#### 4.1.3 Symbol table / name resolution

```rust
// crates/proto3-analyzer/src/resolve.rs

pub struct SymbolIndex {
    /// Fully-qualified name ("google.protobuf.Timestamp") -> definition site.
    by_fqn: FxHashMap<Fqn, DefId>,
    /// Per-file symbol trees for document-symbol requests.
    by_file: FxHashMap<FileUri, FileSymbolTree>,
    /// Cross-file "symbol X is used here" index — powers find-references and
    /// also invalidation when a definition changes.
    references: FxHashMap<DefId, Vec<Ref>>,
    defs: Arena<Def>,
}

impl SymbolIndex {
    /// Resolve a type name appearing at `site`, applying proto3 scoping rules:
    ///   1. current message scope (nested messages/enums visible first)
    ///   2. enclosing message scopes, outward
    ///   3. current package scope
    ///   4. each imported package scope (only `public import` chains are
    ///      transitively visible)
    pub fn resolve_type(&self, site: &UseSite, name: &str) -> Result<DefId, Unresolved> {
        for scope in site.scopes_outward() {
            if let Some(def) = self.by_fqn.get(&scope.join(name)) {
                return Ok(*def);
            }
        }
        Err(Unresolved { name: name.into(), site: site.span() })
    }
}
```

Scoping rules follow the [protobuf language spec](https://protobuf.dev/reference/protobuf/proto3-spec/): a reference like `Foo.Bar` searches innermost-out, and a leading `.` anchors to the global namespace.

#### 4.1.4 Diagnostics engine

Diagnostic producers are chained:

```rust
pub fn run_all_checks(ws: &Workspace, uri: &FileUri) -> Vec<ProtoDiagnostic> {
    let mut out = Vec::new();
    out.extend(ws.file(uri).parse_diagnostics());
    out.extend(check_imports(ws, uri));
    out.extend(check_unknown_types(ws, uri));
    out.extend(check_field_numbers(ws, uri));
    out.extend(check_reserved(ws, uri));
    out.extend(check_duplicate_names(ws, uri));
    out.extend(check_oneof(ws, uri));
    out
}
```

See §7 for the full catalog.

#### 4.1.5 Query layer / public API

The public surface to WASM is a flat, handle-based C-like API. All arguments and return values are JSON strings for `wasm-bindgen` friendliness (same convention as `wgsl-analyzer`'s `validate_wgsl`).

```rust
// crates/proto3-analyzer/src/wasm_api.rs
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Analyzer(RefCell<Workspace>);

#[wasm_bindgen]
impl Analyzer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Analyzer {
        console_error_panic_hook::set_once();
        Analyzer(RefCell::new(Workspace::with_bundled_well_known_types()))
    }

    pub fn set_include_paths(&self, paths_json: &str) {
        let paths: Vec<String> = serde_json::from_str(paths_json).unwrap_or_default();
        self.0.borrow_mut().set_include_paths(paths);
    }

    pub fn update_file(&self, uri: &str, source: &str) -> String {
        let changed = self.0.borrow_mut().update_file(uri.into(), source.into());
        serde_json::to_string(&changed).unwrap()
    }

    pub fn diagnostics(&self, uri: &str) -> String {
        let diags = self.0.borrow().diagnostics_for(&uri.into());
        serde_json::to_string(&diags).unwrap()
    }

    pub fn completion(&self, uri: &str, line: u32, col: u32) -> String {
        let items = self.0.borrow().completion_at(&uri.into(), line, col);
        serde_json::to_string(&items).unwrap()
    }
    // ...definition, references, hover, document_symbols, workspace_symbols,
    //    rename, prepare_rename, semantic_tokens, formatting...
}
```

Is Salsa-style incrementality in scope for v1? **No.** We get coarse incrementality for free: `Workspace::update_file` recomputes only the touched file plus its transitive importers. We do not memoize query-level computations. `rust-analyzer`-style [salsa](https://github.com/salsa-rs/salsa) crate integration is a Phase 4 item if measured profiles show hot spots; for proto's tiny AST it's unlikely to matter.

### 4.2 Extension host (TypeScript)

Activation follows the `wgsl-shader` pattern almost exactly, but registers many more providers:

```typescript
// extensions/protobuf/src/extension.ts

import * as vscode from 'vscode';
import * as path from 'path';
// eslint-disable-next-line @typescript-eslint/no-require-imports
type WasmModule = typeof import('../wasm/proto3_analyzer');

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const wasm: WasmModule = require(
    path.join(context.extensionPath, 'wasm', 'proto3_analyzer.js'),
  );
  const analyzer = new wasm.Analyzer();

  await bootstrapIncludePaths(analyzer);
  await preloadWorkspace(analyzer);

  const diagCollection = vscode.languages.createDiagnosticCollection('proto3');
  context.subscriptions.push(diagCollection);

  const selector: vscode.DocumentSelector = { scheme: 'file', language: 'proto3' };

  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider(selector,
      new AnalyzerDefinitionProvider(analyzer)),
    vscode.languages.registerReferenceProvider(selector,
      new AnalyzerReferenceProvider(analyzer)),
    vscode.languages.registerHoverProvider(selector,
      new AnalyzerHoverProvider(analyzer)),
    vscode.languages.registerCompletionItemProvider(selector,
      new AnalyzerCompletionProvider(analyzer), '.', '"', '/', '('),
    vscode.languages.registerDocumentSymbolProvider(selector,
      new AnalyzerDocumentSymbolProvider(analyzer)),
    vscode.languages.registerWorkspaceSymbolProvider(
      new AnalyzerWorkspaceSymbolProvider(analyzer)),
    vscode.languages.registerRenameProvider(selector,
      new AnalyzerRenameProvider(analyzer)),
    vscode.languages.registerDocumentFormattingEditProvider(selector,
      new AnalyzerFormattingProvider(analyzer)),
  );

  // File / doc change wiring
  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument(e => {
      if (e.document.languageId !== 'proto3') return;
      analyzer.update_file(e.document.uri.toString(), e.document.getText());
      refreshDiagnostics(e.document, analyzer, diagCollection);
    }),
    vscode.workspace.onDidOpenTextDocument(doc => {
      if (doc.languageId !== 'proto3') return;
      analyzer.update_file(doc.uri.toString(), doc.getText());
      refreshDiagnostics(doc, analyzer, diagCollection);
    }),
    vscode.workspace.onDidDeleteFiles(e => {
      for (const uri of e.files) analyzer.remove_file(uri.toString());
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('proto3.restart', async () => {
      /* recreate Analyzer handle, re-preload workspace */
    }),
    vscode.commands.registerCommand('proto3.showSymbolTree', async () => {
      /* open a TreeView with analyzer.workspace_symbol_tree() */
    }),
  );
}
```

A typical provider is a thin marshaling layer:

```typescript
class AnalyzerCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly analyzer: Analyzer) {}

  provideCompletionItems(
    doc: vscode.TextDocument,
    pos: vscode.Position,
  ): vscode.CompletionItem[] {
    const raw = this.analyzer.completion(
      doc.uri.toString(),
      pos.line,
      pos.character,
    );
    const items: AnalyzerCompletionItem[] = JSON.parse(raw);
    return items.map(toVsCodeCompletionItem);
  }
}
```

## 5. Parser Choice

**Decision: a hand-written lexer + recursive-descent parser, implemented in this repo as `crates/proto3-analyzer/src/lexer.rs` and `crates/proto3-analyzer/src/parser/`.** No third-party grammar or parser crate is pulled in at runtime.

### 5.1 Why own the parser

Proto3 is a small, stable grammar — the full [language spec](https://protobuf.dev/reference/protobuf/proto3-spec/) fits in a long afternoon's worth of productions. Owning it costs a bounded one-time implementation and pays off permanently on four axes:

1. **No unmaintained-crate or unstable-API risk.** The leading candidate crates all carry structural risk: `protox-parse` is effectively single-maintainer with releases tied to `prost` bumps; `protobuf-parse`'s README explicitly warns "not meant to be used directly; no stable API"; `protobuf-ast-parser` is brand-new with no ecosystem traction. Taking a runtime dependency on any of them means inheriting their cadence for proto-editions support, spec fixes, and WASM-compatibility.
2. **Editor-grade error recovery by default, not as a band-aid.** Every candidate parser is fail-fast — they return `Result<Descriptor, ParseError>` and bail on the first error, which is exactly backwards for an editor. A repair-pass wrapper can recover the prefix but never a mid-statement partial AST. A recursive-descent parser designed for recovery hands us partial ASTs with `Missing` nodes, which drive usable completions and hover even on broken input.
3. **First-class span retention.** We do not want to store data as `FileDescriptorProto` + `SourceCodeInfo` and fish spans out of `location.path` at query time — that indirection is fine for a compiler but awkward for a language server that needs exact-range operations on identifiers and nested type references. Our AST carries `ByteSpan` on every node by construction.
4. **Customization latitude.** We can add diagnostics, lints, and syntactic sugar (e.g. surfacing `map<K,V>` in the AST even though the wire format desugars it to a synthetic nested message) without waiting on upstream.

The costs: roughly 1500–2500 lines of Rust for lexer + parser + AST + tests, and the obligation to track proto editions (2023, 2024) ourselves. We budget this into Phase 1 (§16).

### 5.2 Alternatives considered

| Option | Source | Why we passed |
|--------|--------|---------------|
| **`protox-parse`** | [crates.io/crates/protox-parse](https://crates.io/crates/protox-parse), [github.com/andrewhickman/protox](https://github.com/andrewhickman/protox) | Stable public API, miette-quality diagnostics, pure-Rust, WASM-clean, `prost-build`'s current frontend. Still: single-maintainer; fail-fast with no public hooks for recovery; stores everything as `FileDescriptorProto` with spans behind `source_code_info`. Adopting it locks our analyzer's data model to someone else's release cadence. |
| **`protobuf-parse`** (rust-protobuf) | [crates.io/crates/protobuf-parse](https://crates.io/crates/protobuf-parse) | Battle-tested inside `rust-protobuf`, but the author explicitly documents "not meant to be used directly; no stable API." Unfit as a foundational dependency. |
| **`protobuf-ast-parser`** | [crates.io/crates/protobuf-ast-parser](https://crates.io/crates/protobuf-ast-parser) | Newer crate; preserves comments as AST nodes. Too new, too narrow, too small a maintenance base. |
| **`tree-sitter-proto`** | [github.com/mitchellh/tree-sitter-proto](https://github.com/mitchellh/tree-sitter-proto), [github.com/coder3101/tree-sitter-proto](https://github.com/coder3101/tree-sitter-proto) | Free incremental re-parsing and good error recovery. But the runtime is C — WASM distribution is possible but non-trivial — and tree-sitter produces a CST, not a typed AST, so we would still implement a lowering pass and a name-resolution layer on top. Net: comparable work to writing the parser ourselves, with a heavier dependency footprint. |
| **`pest`** | [pest.rs](https://pest.rs) | PEG grammar is readable but has poor error recovery; no community proto3 grammar that tracks the spec precisely. |
| **`nom`** | [github.com/rust-bakery/nom](https://github.com/rust-bakery/nom) | Combinator-heavy; proto3's size makes the grammar-in-types style painful compared to plain recursive descent. |
| **Hand-written recursive-descent (in-repo)** | this RFC | *Chosen.* Bounded scope, editor-grade recovery by construction, owns its AST and span model, zero supply-chain exposure, full customization. |

### 5.3 What we still borrow

- `prost-types` *only* in the offline `ast::to_descriptor` adapter behind the `descriptor-adapter` Cargo feature, for the future CLI-parity path (§15 #6). Not compiled into the editor WASM build.
- `miette` (optional, `default-features = false`) for `Diagnostic` trait derivation on our own error types — lightweight and WASM-safe. Can be dropped if linear-memory overhead becomes a concern.

### 5.4 Grammar coverage

The parser targets proto3 as specified, plus:

- **proto2 compatibility parsing** (so a proto3 file can `import "legacy.proto"` without the import target's parser failing). `required`, `optional` (proto2-style default field), `group`, and `extensions` / `extend` blocks are parsed; in a proto3 file they emit a diagnostic.
- **Editions 2023 / 2024** (`edition = "2023";`). New syntax (`features = { ... }`, edition-specific field presence) is recognized and folded into the AST; full semantic validation of edition features is a Phase 4 item.
- **Custom options** (`option (my.package.opt) = ...;`). Parsed into `ast::OptionValue` trees including nested message literals (`{ key: value, nested: { ... } }`). Name resolution for the extension field is a resolver-layer concern.

### 5.5 Incremental re-parsing

The parser re-parses whole files on edit. Proto3 files are small (typical: a few hundred lines; pathological: 10K), and a straight-line recursive-descent parser comfortably handles that in single-digit milliseconds. If profiling shows whole-file re-parse is a bottleneck on very large schemas, we revisit with a token-stream diff / subtree re-parse — the AST is already node-per-declaration, which makes that retrofit tractable.

## 6. Symbol Resolution & Import Handling

### 6.1 Scoping rules

Proto3 scoping (as codified in [protobuf.dev/reference/protobuf/proto3-spec](https://protobuf.dev/reference/protobuf/proto3-spec/)) resolves a name `Foo.Bar` by walking outward from the current scope:

```
message A {
  message B {
    message C { int32 x = 1; }
    C c = 1;   // resolves to .pkg.A.B.C
    A.B.C d = 2; // also resolves to .pkg.A.B.C
  }
}
```

A leading `.` pins the search to the file's package root. Across files, symbols are visible only if:

- they live in a file the current file `import`s, **and**
- they live in an imported package scope, **and**
- if the import is transitive, only `public import` chains carry visibility.

The symbol index stores fully-qualified names (`.google.protobuf.Timestamp`) and tracks `public`-ness on each import edge.

### 6.2 Include-path configuration

Resolved in priority order:

1. **VSCode setting `proto3.includePaths`** (`string[]`). Workspace-relative or absolute paths. Closest analog to `protoc -I ...`.
2. **The directory of the current file.** Gives zero-config "it just works" behavior for small projects.
3. **Auto-discovered `buf.yaml` / `buf.work.yaml`.** If the workspace root (or any ancestor) contains one of these, parse it and:
   - Add each module directory as an include root.
   - If `buf.lock` lists BSR dependencies, add the cached module path (`~/.cache/buf/v2/modules/…`) if the directory exists on disk. We do **not** fetch from BSR; we rely on `buf mod update` having already populated the cache.
4. **Bundled well-known types fallback.** The crate embeds `google/protobuf/*.proto` via `include_str!` at build time: `descriptor.proto`, `any.proto`, `timestamp.proto`, `duration.proto`, `empty.proto`, `struct.proto`, `wrappers.proto`, `field_mask.proto`, `api.proto`, `source_context.proto`, `type.proto`, `compiler/plugin.proto`. These are shipped with the `.vsix` (≈80 KB total uncompressed).

### 6.3 Edge cases

| Case | Handling |
|------|----------|
| **Circular imports** (`a.proto` imports `b.proto`, `b.proto` imports `a.proto`) | Build symbol index in two passes: (1) collect all declarations per file, (2) resolve all type references. Cycles do not trip the resolver. We emit a `warning` diagnostic (proto3 spec *allows* cycles, but many tools complain). |
| **`public import`** | The importing file transitively re-exports the imported file's symbols. `b.proto: public import "a.proto"; c.proto: import "b.proto";` — `c.proto` sees `a.proto`'s symbols. We propagate this during resolution. |
| **`weak import`** | Accepted but treated like a regular import for analysis. Unknown-type errors downgraded to warnings when the weak target cannot be resolved. |
| **Nested messages/enums** | Indexed by FQN; the resolver's scope walk handles them. |
| **Groups (proto2 carry-over)** | We parse (proto3 files that vendor proto2 definitions do exist), flag `group` usage as a warning in proto3 files. |
| **Map fields** | `map<K, V>` is desugared to a synthetic nested message `FooEntry { K key=1; V value=2; }` per the spec. We surface it to the user as `map<K,V>` but resolve `V` as a type reference. |
| **Same-file two-pass references** | A field can reference a message defined later in the same file — name resolution runs after full file parse, so this just works. |

## 7. Diagnostics Catalog

| Code | Severity | Message template | Source |
|------|----------|------------------|--------|
| `PROTO0001` | Error | Unexpected token `{token}` | parser |
| `PROTO0002` | Error | Expected `{expected}`, found `{found}` | parser |
| `PROTO0003` | Error | Unterminated string literal | parser |
| `PROTO0010` | Error | Cannot resolve import `"{path}"` (searched: {paths}) | resolver |
| `PROTO0011` | Warning | Import `"{path}"` is never used | resolver |
| `PROTO0012` | Warning | Circular import: `{a}` ⇄ `{b}` | resolver |
| `PROTO0020` | Error | Unknown type `{name}` (did you mean `{suggestion}`?) | resolver |
| `PROTO0021` | Error | Cannot use `{name}` as a type (it is a `{kind}`) | resolver |
| `PROTO0030` | Error | Duplicate field number `{n}` in `{message}` (previously used by `{other}`) | check_field_numbers |
| `PROTO0031` | Error | Field number `{n}` is outside the valid range 1..=536870911 | check_field_numbers |
| `PROTO0032` | Error | Field number `{n}` is in the reserved range 19000..=19999 | check_field_numbers |
| `PROTO0033` | Error | Field number `{n}` conflicts with `reserved` in `{message}` | check_reserved |
| `PROTO0034` | Error | Field name `{name}` conflicts with `reserved` name in `{message}` | check_reserved |
| `PROTO0040` | Error | Duplicate {kind} name `{name}` | check_duplicate_names |
| `PROTO0041` | Error | Duplicate enum value `{name}` in `{enum}` | check_duplicate_names |
| `PROTO0042` | Error | Enum `{name}` first value must be 0 in proto3 | check_proto3_semantics |
| `PROTO0050` | Error | Oneof `{oneof}` cannot contain `repeated` field `{field}` | check_oneof |
| `PROTO0051` | Error | Oneof `{oneof}` cannot contain `map` field `{field}` | check_oneof |
| `PROTO0060` | Error | `{type}` cannot be `packed` | check_packed |
| `PROTO0061` | Error | `map<{K}, V>` key type must be an integral or string type | check_map |
| `PROTO0070` | Warning | Service `{name}` does not follow UpperCamelCase convention | style (optional) |
| `PROTO0071` | Warning | Field `{name}` does not follow lower_snake_case convention | style (optional) |
| `PROTO0072` | Warning | Message `{name}` is empty | style (optional) |
| `PROTO0080` | Info | `option deprecated = true` — usages will trigger strike-through rendering | marker |

Codes in the `0070-0079` range are only emitted when `proto3.diagnostics.style` is `"on"` (default `"off"` to avoid fighting with `buf lint`).

## 8. LSP Feature Coverage Matrix

| LSP capability | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
|----------------|:-------:|:-------:|:-------:|:-------:|
| `textDocument/publishDiagnostics` (parse + syntax) | ✅ | | | |
| `textDocument/publishDiagnostics` (semantic, full catalog) | partial | ✅ | | |
| `textDocument/documentSymbol` | ✅ | | | |
| `workspace/symbol` | partial | ✅ | | |
| `textDocument/definition` | | ✅ | | |
| `textDocument/hover` | | ✅ | | |
| `textDocument/completion` (keywords, types) | partial | ✅ | | |
| `textDocument/completion` (imports, message literals) | | | ✅ | |
| `textDocument/references` | | | ✅ | |
| `textDocument/rename` + `prepareRename` | | | ✅ | |
| `textDocument/codeAction` (organize imports, quick-fix unknown type) | | | | ✅ |
| `textDocument/semanticTokens/full` (optional, finer than TM grammar) | | | | ✅ |
| `textDocument/formatting` | | | | ✅ |
| `textDocument/foldingRange` | | ✅ | | |
| `textDocument/documentHighlight` | | ✅ | | |
| `textDocument/signatureHelp` (for `rpc` and options) | | | | ✅ |
| `textDocument/inlayHint` (field numbers) | | | | ✅ |

## 9. File Structure

Existing layout in this repo:

```
vscode-extensions/
├── Cargo.toml                         (workspace members = ["crates/*"])
├── pnpm-workspace.yaml                (extensions/*, packages/*)
├── crates/
│   └── wgsl-analyzer/                 (existing precedent)
├── extensions/
│   ├── askama-templates/
│   ├── base64-tools/
│   ├── gitignore-generator/
│   ├── markdown-live-preview/
│   └── wgsl-shader/
└── docs/rfc/
    ├── 001-markdown-live-preview-editor.md
    └── 002-proto3-language-server.md   ← this doc
```

### 9.1 Decision on directory names

One extension directory, one crate:

```
extensions/protobuf/         (package name: wx-vsce-protobuf)
crates/proto3-analyzer/      (Rust crate, compiled to WASM into extensions/protobuf/wasm/)
```

**We deliberately do not use `extensions/proto3-language/`** because the language id is `proto3` but the user-facing product name and the filename association (`.proto`) are colloquially "protobuf". The directory name mirrors the marketplace display name.

### 9.2 Rust crate layout

```
crates/proto3-analyzer/
├── Cargo.toml
├── src/
│   ├── lib.rs                  # Re-exports, module wiring
│   ├── wasm_api.rs             # #[wasm_bindgen] surface (Analyzer struct)
│   ├── vfs.rs                  # Workspace, FileUri, IncludePath
│   ├── parse.rs                # Pipeline: lex → parse → ParsedFile
│   ├── lexer.rs                # Hand-written proto3 tokenizer
│   ├── parser/
│   │   ├── mod.rs              # Parser cursor, error sink, sync_to recovery
│   │   ├── file.rs             # top-level: syntax/edition, package, import, option
│   │   ├── message.rs          # message, nested, map, oneof, reserved, extensions
│   │   ├── enum_.rs            # enum bodies and values
│   │   ├── service.rs          # service, rpc
│   │   └── options.rs          # option values, message literals, custom extensions
│   ├── ast.rs                  # Typed AST nodes — own every ByteSpan
│   ├── descriptor.rs           # Optional ast::to_descriptor adapter (prost-types)
│   ├── spans.rs                # SpanTable (line/col ↔ byte offset)
│   ├── resolve/
│   │   ├── mod.rs              # SymbolIndex
│   │   ├── scope.rs            # Scope walk
│   │   └── imports.rs          # Import graph, public/weak handling
│   ├── diagnostics/
│   │   ├── mod.rs              # ProtoDiagnostic type, run_all_checks
│   │   ├── field_numbers.rs    # PROTO0030-0032
│   │   ├── reserved.rs         # PROTO0033-0034
│   │   ├── duplicates.rs       # PROTO0040-0042
│   │   ├── oneof.rs            # PROTO0050-0051
│   │   ├── maps.rs             # PROTO0060-0061
│   │   └── style.rs            # PROTO0070+
│   ├── features/
│   │   ├── completion.rs
│   │   ├── definition.rs
│   │   ├── hover.rs
│   │   ├── references.rs
│   │   ├── rename.rs
│   │   ├── document_symbols.rs
│   │   ├── workspace_symbols.rs
│   │   ├── folding.rs
│   │   └── semantic_tokens.rs
│   └── well_known/
│       ├── mod.rs              # include_str! bundled .proto texts
│       └── protos/google/protobuf/*.proto
└── tests/
    ├── fixtures/               # snapshots of real .proto trees
    │   ├── googleapis-subset/
    │   └── buf-example/
    ├── conformance/            # diff our ast::to_descriptor output
    │   │                       # against `protoc --descriptor_set_out`
    │   └── run.rs              # zero-tolerance drift gate
    ├── snapshot_lexer.rs       # insta-powered token streams
    ├── snapshot_parse.rs       # insta-powered AST
    ├── snapshot_recovery.rs    # parser error-recovery goldens
    ├── snapshot_diagnostics.rs
    ├── snapshot_completion.rs
    └── snapshot_definition.rs
```

### 9.3 Extension layout

```
extensions/protobuf/
├── package.json
├── tsconfig.json
├── tsdown.config.mts
├── .vscodeignore
├── language-configuration.json
├── syntaxes/
│   └── proto3.tmLanguage.json
├── wasm/                               # output of wasm-pack (git-ignored)
│   ├── proto3_analyzer.js
│   ├── proto3_analyzer.d.ts
│   └── proto3_analyzer_bg.wasm
├── src/
│   ├── extension.ts                    # activate() wiring
│   ├── analyzer.ts                     # AnalyzerBridge around the WASM module
│   ├── workspaceBootstrap.ts           # preload .proto files, watch changes
│   ├── includePaths.ts                 # config + buf.yaml auto-discovery
│   ├── diagnostics.ts                  # refreshDiagnostics pump
│   ├── providers/
│   │   ├── definition.ts
│   │   ├── references.ts
│   │   ├── hover.ts
│   │   ├── completion.ts
│   │   ├── documentSymbol.ts
│   │   ├── workspaceSymbol.ts
│   │   ├── rename.ts
│   │   ├── formatting.ts
│   │   ├── foldingRange.ts
│   │   └── semanticTokens.ts
│   ├── commands/
│   │   ├── restart.ts
│   │   └── showSymbolTree.ts
│   └── types.ts                        # shared types mirroring the Rust JSON
└── test/
    ├── suite/
    │   ├── completion.test.ts
    │   └── definition.test.ts
    └── runTest.ts
```

## 10. `package.json` Sketch

```jsonc
{
  "name": "wx-vsce-protobuf",
  "displayName": "Protocol Buffers (proto3)",
  "description": "Protocol Buffers (proto3) editing — syntax highlighting, outline, and diagnostics powered by a Rust analyzer compiled to WebAssembly.",
  "version": "0.2.0",
  "private": true,
  "publisher": "weixu",
  "engines": { "vscode": "^1.96.0" },
  "categories": ["Programming Languages", "Linters"],
  "activationEvents": [
    "onLanguage:proto3"
  ],
  "main": "./dist/extension.js",
  "contributes": {
    "languages": [
      {
        "id": "proto3",
        "aliases": ["Protocol Buffers", "Proto3", "proto3", "proto"],
        "extensions": [".proto"],
        "configuration": "./language-configuration.json"
      }
    ],
    "grammars": [
      {
        "language": "proto3",
        "scopeName": "source.proto3",
        "path": "./syntaxes/proto3.tmLanguage.json"
      }
    ],
    "commands": [
      { "command": "proto3.restart",         "title": "Proto3: Restart Analyzer",       "category": "Proto3" },
      { "command": "proto3.showSymbolTree",  "title": "Proto3: Show Workspace Symbols", "category": "Proto3" },
      { "command": "proto3.revealDefinition","title": "Proto3: Reveal Descriptor FQN",  "category": "Proto3" }
    ],
    "configuration": {
      "title": "Protocol Buffers (proto3)",
      "properties": {
        "proto3.includePaths": {
          "type": "array",
          "items": { "type": "string" },
          "default": [],
          "markdownDescription": "Additional `-I` include paths for `import \"…\"` resolution. Workspace-relative or absolute. Merged with auto-discovered Buf modules and the current file's directory."
        },
        "proto3.buf.autoDiscover": {
          "type": "boolean",
          "default": true,
          "description": "Automatically discover buf.yaml / buf.work.yaml and add their module directories as include paths."
        },
        "proto3.wellKnownTypes.bundled": {
          "type": "boolean",
          "default": true,
          "description": "Fall back to bundled google/protobuf/*.proto definitions when no include path provides them."
        },
        "proto3.hover.enabled": {
          "type": "boolean",
          "default": true
        },
        "proto3.diagnostics.enabled": {
          "type": "boolean",
          "default": true
        },
        "proto3.diagnostics.style": {
          "type": "string",
          "enum": ["off", "on"],
          "default": "off",
          "description": "Emit style / naming-convention warnings (PROTO0070+)."
        },
        "proto3.diagnostics.onType": {
          "type": "boolean",
          "default": true,
          "description": "Recompute diagnostics as you type (debounced). Turn off to only recompute on save."
        },
        "proto3.format.enabled": {
          "type": "boolean",
          "default": false,
          "description": "Enable document-formatting provider (Phase 4)."
        },
        "proto3.format.command": {
          "type": "string",
          "enum": ["builtin", "buf", "clang-format"],
          "default": "builtin",
          "description": "Which formatter to use when `proto3.format.enabled` is true."
        },
        "proto3.completion.imports.enabled": {
          "type": "boolean",
          "default": true
        },
        "proto3.trace.server": {
          "type": "string",
          "enum": ["off", "messages", "verbose"],
          "default": "off",
          "description": "Trace analyzer calls to the 'Protocol Buffers' output channel."
        }
      }
    }
  },
  "scripts": {
    "build:wasm": "cd ../../crates/proto3-analyzer && wasm-pack build --target nodejs --out-dir ../../extensions/protobuf/wasm --out-name proto3_analyzer",
    "build": "tsdown",
    "clean": "rm -rf dist wasm",
    "typecheck": "tsc --noEmit",
    "watch": "tsdown --watch",
    "package": "pnpm run build:wasm && pnpm run build && vsce package --no-dependencies --allow-missing-repository",
    "vscode:prepublish": "tsdown --minify",
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "dependencies": {},
  "devDependencies": {
    "@types/node": "catalog:build",
    "@types/vscode": "^1.96.0",
    "@vscode/vsce": "catalog:build",
    "tsdown": "catalog:build",
    "typescript": "catalog:build",
    "vitest": "catalog:build"
  }
}
```

**Note on `vscode-languageclient`:** Because we are embedding the analyzer in-process as WASM (transport A), we do **not** depend on `vscode-languageclient`. Providers are registered directly against VSCode's `vscode.languages.register*` APIs. Should we ever add transport B (native binary) as an alternate, we would add `vscode-languageclient: ^9.x` as a dependency and stand up a `LanguageClient` in `activate()` gated on a setting.

**Note on `contributes.languages` / `contributes.grammars`:** Owned by this same extension. A user who only wants highlighting still installs the full package — activation cost is trivial (a few ms for the WASM instantiation).

## 11. `Cargo.toml` Sketch

```toml
# crates/proto3-analyzer/Cargo.toml

[package]
name = "proto3-analyzer"
version = "0.1.0"
edition = "2021"
description = "Proto3 (Protocol Buffers) language analyzer — parser, resolver, diagnostics — compilable to WASM for VSCode"
license = "MIT"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
# Data structures
rustc-hash   = "2"             # FxHashMap / FxHashSet
smol_str     = "0.3"           # interned identifier strings
indexmap     = "2"
id-arena     = "2"

# Errors & spans
miette       = { version = "7", default-features = false }
thiserror    = "1"

# WASM boundary
wasm-bindgen = "0.2"
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
console_error_panic_hook = "0.1"

# Optional — enables ast::to_descriptor for the future proto3-check CLI
# and build-tool integrations. NOT compiled into the editor WASM build.
prost-types  = { version = "0.13", optional = true }

# Optional (gated) — native-only LSP binary path (Phase 4)
[target.'cfg(not(target_family = "wasm"))'.dependencies]
tower-lsp = { version = "0.20", optional = true }
tokio     = { version = "1",    optional = true, features = ["full"] }

[features]
default            = []
descriptor-adapter = ["prost-types"]
lsp-server         = ["tower-lsp", "tokio"]

[dev-dependencies]
insta     = "1"
proptest  = "1"
pretty_assertions = "1"

[package.metadata.wasm-pack.profile.release]
wasm-opt = false
```

## 12. Build & Distribution

Transport A (WASM, selected in §3.1) dictates a one-shot build flow that is a close clone of `wgsl-shader`:

### 12.1 Local build

```bash
# First-time setup, once per developer:
cargo install wasm-pack

# From the extension directory:
cd extensions/protobuf
pnpm install
pnpm run build:wasm      # compiles the Rust crate to wasm/, ~5-15 s
pnpm run build           # bundles the TypeScript host code to dist/
```

`pnpm run package` runs the full pipeline and invokes `vsce package` to produce a `.vsix`. No `--target` flag is needed — the single vsix is universal.

### 12.2 Bundled artifacts in the vsix

The resulting `.vsix` contains:

```
extension/
├── package.json
├── dist/extension.js         # ~50 KB bundled TS
├── wasm/
│   ├── proto3_analyzer.js    # wasm-bindgen glue
│   └── proto3_analyzer_bg.wasm  # ~350-500 KB (hand-written parser + AST)
└── README.md
```

Rough size estimate: `wgsl-analyzer`'s WASM bundle is ~300 KB; we expect ~350–500 KB for proto3 given the hand-written lexer/parser/AST and the absence of `protox-parse` or `prost-types` on the default build. Compiling with the `descriptor-adapter` feature would add roughly ~200 KB (prost-types), but that variant is not what the editor ships — it is for the future `proto3-check` CLI.

### 12.3 CI build

A GitHub Actions workflow (or whatever this repo uses) runs:

1. `cargo test -p proto3-analyzer` — unit + insta snapshot tests.
2. `wasm-pack test --node crates/proto3-analyzer` — smoke-test the WASM boundary.
3. `pnpm --filter wx-vsce-protobuf run build:wasm && pnpm --filter wx-vsce-protobuf run build`.
4. `pnpm --filter wx-vsce-protobuf run package` — produces the vsix as a build artifact.

No platform matrix. A single Ubuntu runner suffices.

### 12.4 Fallback plan (transport B sketch)

If we later need transport B, the build delta is:

- Add a `proto3-lsp` bin target inside `crates/proto3-analyzer` gated on `--features lsp-server`, using `tower-lsp` and `tokio`.
- `cargo build --release --features lsp-server --target {aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, x86_64-unknown-linux-musl, x86_64-pc-windows-msvc}`.
- Bundle each triple's binary under `server/{triple}/proto3-lsp{.exe}`. Use `@vscode/vsce package --target <vsix-platform>` to emit per-platform vsixes.
- For unrecognized platforms, extension activation downloads a binary from the release page and pins its hash.

We are explicitly **not** doing this in v1 (§15 Open Question #1).

## 13. Performance Targets

All on a reference machine (M2 MacBook Pro, Node 22):

| Scenario | Target |
|----------|--------|
| Cold analyzer boot (no files) | < 20 ms (WASM instantiation only) |
| Preload 50 files × ~500 lines average (googleapis-subset) | < 500 ms |
| Preload 300 files (full googleapis) | < 1500 ms |
| Single-file edit → `update_file` → diagnostics for that file | < 50 ms |
| Edit that invalidates 5 dependent files → full re-check | < 300 ms |
| Edit that invalidates 50 dependent files → full re-check | < 1000 ms |
| `textDocument/completion` response | < 30 ms |
| `textDocument/definition` response | < 20 ms |
| `workspace/symbol` fuzzy query across 300 files | < 100 ms |
| Memory overhead (WASM linear memory) for googleapis | < 30 MB |

We measure these with a harness in `crates/proto3-analyzer/benches/` (criterion-based) and a separate extension-host integration test that drives the WASM module through the real `Analyzer` API. A regression of >20% fails CI.

## 14. Prior Art

| Project | URL | Lang | What it does well | What's missing for our use case |
|---------|-----|------|-------------------|---------------------------------|
| `buf lsp` (inside `buf` CLI) | [buf.build/docs/cli/editors-lsp/](https://buf.build/docs/cli/editors-lsp/), blog [buf.build/blog/protobuf-lsp](https://buf.build/blog/protobuf-lsp) | Go | Best-in-class semantic analysis, tight `buf.yaml`/BSR integration, fast on huge modules | Requires `buf` CLI on PATH; coupled to Buf's workspace conventions; Go binary must be shipped separately |
| `bufls` (prototype, archived) | [github.com/bufbuild/buf-language-server](https://github.com/bufbuild/buf-language-server) | Go | Pioneered the space | Archived; superseded by `buf lsp` |
| `protols` (coder3101) | [github.com/coder3101/protols](https://github.com/coder3101/protols), [lib.rs/crates/protols](https://lib.rs/crates/protols) | Rust | tree-sitter-based parsing, shelling out to `protoc` for deep diagnostics, formatter via `clang-format`, active development (~176⭐, Dec 2025 updates). Published to crates.io. | Requires user to `cargo install protols`; VSCode integration ([github.com/ianandhum/vscode-protobuf-support](https://github.com/ianandhum/vscode-protobuf-support)) is community-run and unofficial. Tree-sitter grammar requires C toolchain to build. |
| `protols` (kralicky) | [github.com/kralicky/protols](https://github.com/kralicky/protols) | Go | Built on the `golang/tools` LSP infrastructure (same base as gopls) | Same name as the Rust one — confusing. No VSCode integration. |
| `pbls` (rcorre) | [github.com/rcorre/pbls](https://github.com/rcorre/pbls), [git.sr.ht/~rrc/pbls](https://git.sr.ht/~rrc/pbls) | Rust | Simple, focused, built on `protobuf-parse`, has `nvim-lspconfig` entry | Hand-rolled `.pbls.toml` config; minimal feature set; no `buf.yaml` awareness; `cargo install` prerequisite |
| `protobuf-language-server` (lasorda) | [github.com/lasorda/protobuf-language-server](https://github.com/lasorda/protobuf-language-server) | Go | Supports `.proto` and embedded-in-C++ snippets; Rust port in progress at [github.com/lasorda/protobuf-lsp](https://github.com/lasorda/protobuf-lsp) | Installation friction; inconsistent with VSCode UX expectations |
| `protobuf-lsp` (hudbrog) | [github.com/hudbrog/protobuf-lsp](https://github.com/hudbrog/protobuf-lsp) | TS | In-extension analysis | Narrow scope; not competitive |
| `vscode-proto3` (zxh404) | [github.com/zxh0/vscode-proto3](https://github.com/zxh0/vscode-proto3), [marketplace.visualstudio.com/items?itemName=zxh404.vscode-proto3](https://marketplace.visualstudio.com/items?itemName=zxh404.vscode-proto3) | TS | Historical dominance; baseline highlighting + `protoc` invocation | **Deprecated.** No LSP. |
| `protobuf-vsc` (DrBlury) | [github.com/DrBlury/protobuf-vsc-extension](https://github.com/DrBlury/protobuf-vsc-extension), [marketplace.visualstudio.com/items?itemName=DrBlury.protobuf-vsc](https://marketplace.visualstudio.com/items?itemName=DrBlury.protobuf-vsc) | TS | Advertised successor to zxh404 with 30+ diagnostics, organize-imports, reference counts | Pure TS parser; no shared analysis with build tools; no WASM/Rust speed story |
| `vscode-pbkit` (pbkit) | [marketplace.visualstudio.com/items?itemName=pbkit.vscode-pbkit](https://marketplace.visualstudio.com/items?itemName=pbkit.vscode-pbkit), parser at [github.com/pbkit/pbkit](https://github.com/pbkit/pbkit) | TS | Deno-native parser, go-to-definition, snappy | Limited ecosystem; import-path handling primitive |
| `vsc-proto-lang` (tscpp) | [github.com/tscpp/vsc-proto-lang](https://github.com/tscpp/vsc-proto-lang) | TS | IntelliSense + type-check; small project | Appears inactive; narrow scope |

**Our positioning vs the above.** `buf lsp` is the quality bar to meet and the project we respect most. We are explicitly *not* trying to re-do what Buf has done — instead we target the VSCode-only audience that (a) doesn't use Buf, (b) wants zero-install UX, (c) benefits from the same Rust analyzer their `prost-build` uses at compile time. In workspaces that **do** use Buf, we recommend and auto-detect `buf.yaml`, and a future Phase 5 could let us delegate heavy analysis to `buf lsp serve` if it's on PATH.

## 15. Open Questions

1. **Should we ship transport B (native binary) in v1 as an opt-in?** Some users will have 10K-file internal schemas where WASM's single-thread execution is the bottleneck. We think "measure first, ship second" — defer to when we have a concrete regression.
2. **Grammar conformance corpus.** The hand-written parser's correctness depends on a large, representative fixture set. Do we lift a known-good suite (googleapis, envoy, grpc protos, bufbuild/protovalidate examples) into `tests/fixtures/` and diff our AST-to-descriptor against `protoc --descriptor_set_out` in CI? Strong yes — tracked as a Phase 1 deliverable. Open question is the size/scope of the vendored corpus (shallow-clone vs vendored subset vs git submodule).
3. **Comment-preservation fidelity.** Our lexer attaches leading/trailing `Trivia` to every token, which handles doc-comments and inline comments correctly. Open question: *detached* comments (blank-line-separated block comments with no attached declaration). Current plan: attach them to the nearest following token and expose via a dedicated `ast::Trivia` stream for formatters. Revisit if hover or formatting surface shows odd behavior.
4. **BSR (Buf Schema Registry) module resolution.** We auto-detect `buf.yaml` and read `buf.lock`, but we do not download modules. Should we? That crosses a line from "editor tool" to "build tool." Punt.
5. **Proto editions** (2023 / 2024 syntax). Our parser recognizes `edition = "2023";` and parses edition-specific syntax into the AST from day one. Full semantic validation of edition features (`features = { field_presence: ... }`, default-value handling, etc.) is Phase 4 — lexer/parser support is the cheap part; correctly modeling feature resolution rules is the work.
6. **Interaction with the `prost-build` ecosystem.** A CLI `cargo proto3-check` that lowers our AST via `ast::to_descriptor` (gated behind the `descriptor-adapter` feature), emits a `FileDescriptorSet`, and runs the same diagnostics as the editor is highly desirable — it means builds and editors share one checker. Parking lot for post-v1.
7. **Semantic tokens vs the TextMate grammar.** The syntax extension ships a TM grammar; if we add a LSP-driven semantic tokens provider, VSCode will layer them on top. Need to decide if that's additive (good — disambiguates type vs field) or fights the grammar (bad).
8. **Rename safety for generated-code consumers.** Renaming a message field is a *wire-compatibility* concern, not just a source concern. Do we warn on rename with "field numbers are what matter for compatibility, name changes are safe on-the-wire but break generated Go/Java/Python code"? Probably yes, as an info-level message attached to the rename preview.
9. **When does the TypeScript host preload the whole workspace vs lazy-load on open?** Preloading is correct for accurate cross-file diagnostics but costs cold-start time. Compromise: scan the workspace for `.proto` files on activation, index only their headers (package + imports + top-level declarations), hydrate full bodies on first open/completion. Not committed.
10. **Web extension support (vscode.dev).** WASM-nodejs target works in the desktop extension host but not in the browser. If we want web support we need `wasm-pack build --target web` and a `browser` entrypoint. Out of scope for v1.

## 16. Implementation Phases

### Phase 1 — Lexer + Parser + Syntax Diagnostics + Symbols (3 weeks)

- `crates/proto3-analyzer` skeleton with `wasm_api.rs`.
- **`lexer.rs`** — full proto3 tokenizer: identifiers, dotted names, numeric literals (dec/hex/oct, float, signed), string literals with escapes and adjacency concatenation, block/line comments attached as `Trivia`, lex-error tokens. insta-snapshotted against a fixture corpus.
- **`parser/`** — recursive-descent parser for the full proto3 grammar (syntax/edition, package, import, option, message, nested, enum, service, rpc, oneof, map, reserved, extensions, extend, custom option values with message literals). Sync-to-recovery on errors, partial ASTs with `Missing` nodes.
- **`ast.rs`** — typed AST with `ByteSpan` on every node.
- **`descriptor.rs`** — optional `ast::to_descriptor` adapter behind the `descriptor-adapter` feature (not shipped in the editor WASM).
- **Conformance harness** in `tests/conformance/`: a fixture corpus (googleapis subset + internal goldens) is lowered via `ast::to_descriptor` and diffed against `protoc --descriptor_set_out`. Zero-tolerance drift gate in CI.
- `vfs.rs` with file upsert/remove, include paths from a setting.
- `SpanTable` (line/col ↔ offset).
- Bundled well-known types (`include_str!`).
- `features::document_symbols` (full tree: services, messages, nested, enums, rpcs, fields).
- Diagnostics: all PROTO0001-0009 (parse errors) + PROTO0030-0034 (field-number + reserved) + PROTO0040-0042 (duplicates).
- `extensions/protobuf/` — grow the existing syntax-only extension into the full package: add activation, AnalyzerBridge, DocumentSymbolProvider, DiagnosticCollection wiring, and `update_file` on edit/open alongside the already-shipping grammar.
- `package.json` with the config properties listed in §10, no formatter.
- CI: `cargo test` + conformance gate + `wasm-pack test --node` + `pnpm run build:wasm && pnpm run build`.

**Acceptance:** Open a single `.proto` file. Outline populates. Red squiggles on duplicate field numbers and syntax errors. Parser survives mid-statement edits without losing surrounding symbols. Corpus conformance diff against `protoc` is empty for all fixtures. Grammar highlighting and analyzer providers activate from the same `wx-vsce-protobuf` package.

### Phase 2 — Resolution + Hover + Completion + Go-to-Definition (3 weeks)

- Full `resolve/` module: `SymbolIndex`, scope walk, public/weak imports, cross-file resolution.
- Diagnostics: PROTO0010-0012 (imports), PROTO0020-0021 (unknown types), PROTO0050-0051 (oneof), PROTO0060-0061 (maps).
- `features::definition` — works across files and to well-known types.
- `features::hover` — type info, field number, cardinality, leading comments.
- `features::completion` — keywords, scalar types, well-known types, symbols-from-imports (no message-literal smarts yet).
- `features::folding`, `features::document_highlight`.
- Workspace preload in the extension host: scan `**/*.proto`, `update_file` each.
- `buf.yaml` / `buf.work.yaml` auto-discovery, gated by `proto3.buf.autoDiscover`.
- Partial workspace symbols.

**Acceptance:** Cmd-click on `google.protobuf.Timestamp` jumps into the bundled descriptor. Hover over a field shows `field_name: Type = 3 [deprecated = true]` with doc-comments. Completing `int` offers `int32`, `int64`.

### Phase 3 — References + Rename + Workspace (2 weeks)

- Reference index + `features::references`.
- `features::rename` + `prepare_rename` producing a `WorkspaceEdit`.
- Full workspace symbols (`workspace/symbol`).
- Completion smarts: imports (`import "goo…"` autocompletes file paths under include roots), field names inside `option (…) = { … };` message literals.
- Reverse-dependency invalidation (already partial from Phase 2) fully covered by snapshot tests.

**Acceptance:** Rename a message across 20 importing files, with undo-safe atomic WorkspaceEdit. Find-references on a field lists every usage including oneof memberships and message literals.

### Phase 4 — Formatting + Buf + Incrementality + Polish (3 weeks)

- `features::semantic_tokens_full` (optional, gated).
- `features::formatting` with three backends: builtin in-crate pretty-printer (default), `buf format` passthrough, `clang-format` passthrough.
- Code actions: organize imports, quick-fix "add missing import" when an unknown type's FQN is found elsewhere in the workspace, "convert group to message" (proto2 carry-over).
- Inlay hints: show field numbers next to field names when not explicit.
- Signature help inside `rpc` declarations.
- Salsa-style memoization (only if benches show it's needed).
- Style diagnostics PROTO0070+ (off by default).
- Optional native-binary build via `--features lsp-server` (transport B) for internal testing.

**Acceptance:** All performance targets in §13 met with 10% headroom. Format-on-save works. `buf format` passthrough produces byte-identical output to running `buf format` manually.
