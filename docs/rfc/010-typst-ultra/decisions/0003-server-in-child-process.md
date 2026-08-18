# 0003 — Run the server in a child process; LSP and preview share it

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 3

## Context

Two sub-questions, decided together because they trade against each other.

**Where does the WASM run?** The repo's default is `require()` into the extension host. A typst session is
heavier than anything else here: with the eviction policy in [0005](0005-cache-eviction-policy.md) it holds
32–106 MB for 10–75 pages, and WASM linear memory is **never returned to the OS**. Cold compiles block the
thread for 80–262 ms ([research/spike.md §4.2](../research/spike.md#42-eviction-age-sweep)).

**One process or two?** Splitting the preview renderer from the LSP would stop a runaway preview stalling
completion, but would mean two WASM instances, two compiles of the same document, and double the memory.

## Decision

**One child process**, forked by `vscode-languageclient` with `TransportKind.ipc`, hosting a single WASM
instance that serves both LSP requests and preview rendering (via `typst/*` custom requests).

The isolation that a second process would have bought is obtained instead by an invariant:

> An LSP request is never allowed to trigger or wait for a compile.

This is implementable because `typst_ide::{autocomplete, tooltip, definition}` all take the compiled
document as an `Option`. IDE features answer from the incrementally-maintained syntax tree plus the *last
good* document; only diagnostics and the preview wait on a fresh compile.
See [design/architecture.md §5](../design/architecture.md#5-concurrency-model-one-thread-two-clocks).

## Consequences

**Buys.** Memory genuinely reclaimable (restart the process), crash isolation from compiler panics, no
blocking of other extensions, and a natural home for the "restart server" command. Sharing one process
means the preview and the diagnostics can never disagree about what the document is.

**Costs.** ~30–40 MB RSS for the Node process itself, plus fork latency on first `.typ` open. The server
starts lazily rather than at activation, so this is not paid by users who never open a typst file.

**Note on the strength of the memory argument.** Early measurements suggested 423 MB at 75 pages, which
made process isolation look mandatory. With eviction age `1` that figure is **106 MB**
([0005](0005-cache-eviction-policy.md)) — so the memory case is weaker than it first appeared. The decision
stands on the *blocking* and *crash isolation* arguments, which are unaffected.

**Alternatives considered.**

| Option | Why not |
| --- | --- |
| `worker_threads` + custom `MessageReader`/`Writer` | Faster start, one process — but the WASM heap still lives in the extension host's address space |
| WASM in the extension host (repo default) | 262 ms blocking on cold compile is antisocial in a shared host |
| `wasm32-wasip1` + `@vscode/wasm-wasi-lsp` | Real `std::fs` in Rust, but adds a hard dependency on the WebAssembly Execution Engine extension |
| Separate preview process | Doubles memory and compiles to solve a problem the "never block on compile" invariant already solves |

## Outcome — measured on real documents, 2026-08-17

The numbers this record rests on came from a synthetic 75-page document. On a real 104-page book they are
**worse**, which strengthens the decision rather than weakening it
([research/corpus.md](../research/corpus.md)):

| | Spike, synthetic | Real corpus |
| --- | --- | --- |
| Cold compile | 262 ms (75 pages) | **524 ms** (104 pages) |
| Peak WASM heap | 106 MB | **222 MB** |

Half a second of blocking is well past what any extension should do on a shared thread, and 222 MB that is
never returned to the OS is exactly the tenant this record describes. The **blocking** argument is now the
strongest of the three, where the original revision leaned on memory and then had to walk it back.

The "never block on compile" invariant held throughout implementation: every `typst-ide` entry point takes
the document as an `Option`, so no IDE feature ever needed a fresh compile. No feature had to be cut for
it.

## Revisit if

- The "never block on compile" invariant proves unenforceable in practice — e.g. a feature genuinely needs
  a synchronous fresh compile — at which point a second process becomes the honest fix.
- A browser build for vscode.dev is pursued, which forces the `vscode-languageclient/browser` + Worker
  shape and supersedes this record for that target. **Scoped** in
  [design/browser.md](../design/browser.md): the blocker is whether vscode.dev serves cross-origin
  isolation headers, since `World::file` is synchronous and only `SharedArrayBuffer` + `Atomics.wait` can
  make an async source look synchronous without patching the compiler.
- Node child-process startup becomes a measurable annoyance on cold open.
