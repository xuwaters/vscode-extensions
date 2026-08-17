# Phase 1 — Foundation

**Goal**: the whole architecture running end to end, carrying exactly one feature.
**Exit criterion**: open a `.typ` file, see typst's real errors and warnings inline, as you type.
**Status**: ☐ Not started — 0 / 16

Phase 1 deliberately ships one LSP feature. Diagnostics require the compile session, the VFS, fonts, the
WASM boundary, the child process, and span mapping — so if diagnostics work, the architecture works.
Everything in Phase 2 is then additive.

## Tasks

### Scaffolding

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P1-01 | Scaffold `extensions/typst-ultra`: `package.json`, `tsconfig.json`, `tsdown.config.mts` with three bundles (host cjs/node, server cjs/node, webview esm/browser), `README.md`. Run `pnpm sync-vscodeignore` — do not hand-write `.vscodeignore` | [architecture.md §8](../design/architecture.md#8-file-layout) | ☐ |
| P1-02 | Root `Cargo.toml`: add the four crates to `members`, add `exclude = ["crates/typst"]`. Verify with `cargo metadata` that all four resolve | [crates.md §7](../design/crates.md#7-build-and-workspace-registration), [0010](../decisions/0010-naming.md) | ☐ |
| P1-14 | Build wiring: `build:wasm` script, `[package.metadata.wasm-pack.profile.release] wasm-opt = ["-Os", "--strip-debug"]`, `package` / `vscode:prepublish` scripts, and a CI job that builds the `wasm32-unknown-unknown` target on every PR | [crates.md §7](../design/crates.md#7-build-and-workspace-registration) | ☐ |
| P1-15 | `LICENSE.md`: `NO LICENSE` + third-party notices for the typst compiler, typstyle, and **all five font licenses** including the GPL'd `NewCM10-Regular.otf`. Wire `cargo-about` for the transitive crate graph, commit its output | [references.md §5](../research/references.md#5-what-goes-in-extensionstypst-ultralicensemd), [0004](../decisions/0004-bundle-default-fonts.md) | ☐ |

### `typst-session`

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P1-03 | Port traits `FileProvider` / `FontProvider` / `PackageProvider`, plus `std::fs` implementations in `tests/` so the engine is testable natively | [crates.md §2.1](../design/crates.md#21-the-ports) | ☐ |
| P1-04 | `SessionWorld`: `impl World` + `impl IdeWorld`, VFS overlay (open documents beat disk), lazy `FontSlots`, `today()` memoized per compile | [crates.md §2.2](../design/crates.md#22-sessionworld) | ☐ |
| P1-05 | Compile lifecycle: `Session::compile` doing **compile then evict** as one non-bypassable operation, last-good-document retention, version stamping. **Test: assert the warm/cold recompile ratio** so a reordering regression fails loudly | [crates.md §2.3](../design/crates.md#23-the-compile-lifecycle), [0005](../decisions/0005-cache-eviction-policy.md) | ☐ |
| P1-06 | Diagnostics: `SourceDiagnostic` → severity, `WorldExt::range` → byte range, `hints` appended, `trace` → related information, grouped by `FileId` | [lsp-features.md §2](../design/lsp-features.md#2-diagnostics-phase-1) | ☐ |
| P1-16 | Snapshot corpus (`insta`) for diagnostics across a fixture set, **including a multi-file project** so diagnostic fan-out to non-open files is covered and its cost observed | [proposal.md §13](../proposal.md#13-testing) | ☐ |

### `typst-lsp-core` and `typst-lsp-wasm`

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P1-07 | Dispatch skeleton, `State`, capability negotiation, and `convert.rs` for byte-offset ⇄ UTF-16 position mapping. **Property test** round-tripping every offset in a document with CJK, ZWJ emoji, and combining marks | [crates.md §3](../design/crates.md#3-typst-lsp-core) | ☐ |
| P1-08 | `didOpen` / `didChange` (incremental, via `Source::edit`) / `didSave` / `didClose`, debounced compile scheduling, `publishDiagnostics` **including clearing URIs that no longer have diagnostics**, and dropping results for superseded versions | [lsp-features.md §2](../design/lsp-features.md#2-diagnostics-phase-1) | ☐ |
| P1-09 | `typst-lsp-wasm`: `TypstServer` bindings, JS-callback port implementations, and the `SingleThreaded<T>` marker with its `compile_error!` guard rather than a bare `unsafe impl Send` | [crates.md §5](../design/crates.md#5-typst-lsp-wasm) | ☐ |

### Node server and extension host

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P1-10 | `server/main.ts`: JSON-RPC loop, **synchronous** `HostServices` (`readFile`, `listDir`, `fontData`, `today`), root-confined VFS path resolution, `drain_events` pump after each response, and graceful "engine not built" degradation when `wasm/` is missing | [architecture.md §7.1](../design/architecture.md#71-host-and-wasm-inside-the-server-process) | ☐ |
| P1-11 | Bundled fonts: `assets/fonts/`, host-side indexing via `TypstServer::index_font`, on-disk `FontInfo` cache keyed by `path + mtime + size` in `globalStorage`. **Measure and record font-index cost at startup** — currently an unclosed research debt | [0004](../decisions/0004-bundle-default-fonts.md) | ☐ |
| P1-12 | Extension host: `client.ts` (`LanguageClient`, `TransportKind.ipc`, lazy start on first `.typ`), `config.ts` (settings → `initializationOptions` + `didChangeConfiguration`), output channel, `typstUltra.restartServer` and `typstUltra.showLog` | [architecture.md §3](../design/architecture.md#3-startup-sequence) | ☐ |
| P1-13 | Language contribution: one `typst` id claiming `.typ` **and** `.typc`, `language-configuration.json` (brackets, comments, `$…$` pairing, on-enter rules), and the minimal TextMate grammar | [0009](../decisions/0009-file-extensions.md), [0007](../decisions/0007-textmate-grammar.md) | ☐ |

## Settings live in Phase 1

Only what Phase 1 uses; the rest arrive with their features.

`rootPath` · `mainFile` (read but only mode 3 honoured — see [0008](../decisions/0008-compile-root.md)) ·
`compile.when` · `compile.debounce` · `diagnostics.enabled` · `fonts.paths` ·
`memory.evictAge` (default `1`) · `memory.restartThresholdMb` · `trace.server`

## Definition of done

- [ ] `pnpm run package` produces a VSIX containing `wasm/`, `assets/fonts/`, and `dist/`
- [ ] Opening a `.typ` starts the server lazily; opening a non-typst file does not
- [ ] Errors and warnings appear as you type, and **clear** when fixed
- [ ] An error in an imported file appears in the Problems panel under that file's URI
- [ ] Deleting `wasm/` produces a friendly status-bar message, not a stack trace
- [ ] `cargo test` passes without a WASM toolchain present
- [ ] CI builds `wasm32-unknown-unknown`
