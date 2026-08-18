# RFC 010 — A Browser Build: Scope

[P4-06](../tasks/phase-4-polish.md) asks for a browser-worker build for
vscode.dev, and says: *"Needs a VFS over `vscode.workspace.fs` (async) — a real
redesign of the synchronous host services, so scope it before starting."*

This is that scoping. **The conclusion is that it is buildable, that the cost is
concentrated in one place, and that the deciding question is whether vscode.dev
serves cross-origin isolation headers — which is not ours to control.** No code
has been written.

---

## 1. What already works in a browser

More than one might expect. The artifact is *already* WASM, and none of the
engine touches the platform:

| Piece | Browser-ready? |
| --- | --- |
| `typst-session`, `typst-lsp-core`, `typst-preview-core` | Yes — no I/O, no clock, no threads |
| `typst-lsp-wasm` | Yes, rebuilt with `wasm-pack --target web` instead of `nodejs` |
| The preview webview | Yes — it is already a browser context and talks `postMessage` |
| The extension host code | Mostly — `client.ts` swaps `vscode-languageclient/node` for `/browser` |

What does **not** carry over is exactly one thing, and it is the thing the whole
design rests on.

## 2. The one hard problem

`World::file` is called **synchronously, from inside a compile**, hundreds of
times per document. In Node the host answers with `fs.readFileSync` and returns
before the compiler notices anything happened. In a browser there is no
synchronous file read: `vscode.workspace.fs.readFile` is a `Promise`, and typst
provides no way to suspend a compile on one — `World` is a synchronous trait,
and making it async would mean patching the compiler, which
[0001](../decisions/0001-unmodified-upstream-typst.md) forbids.

So the options are about *how to make an async source look synchronous*, not
about changing the compiler.

### Option A — `SharedArrayBuffer` + `Atomics.wait`

Run the server in a `Worker`. The worker's host callback writes a request into a
`SharedArrayBuffer`, signals the main thread, and calls `Atomics.wait` — which
genuinely blocks the worker thread. The main thread performs the async read and
signals back.

* **Fidelity: complete.** The compile is unchanged; it does not know it waited.
* **Cost: moderate.** ~200 lines of ring buffer, plus request encoding.
* **Blocker: `SharedArrayBuffer` requires cross-origin isolation** —
  `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp` on the serving document. Whether
  vscode.dev sets these for extension workers is the load-bearing unknown, and
  it is not something an extension can change.
* This is the mechanism `@vscode/wasm-wasi` uses, which is evidence it is viable
  in at least some VSCode web configurations.

### Option B — preload the file graph

Before compiling, walk the document's imports and includes, read every reachable
file asynchronously, and populate the VFS. The compile then finds everything
already in memory and never blocks.

* **Fidelity: incomplete, and the gaps are sharp.** A path computed at runtime —
  `image("figures/" + name + ".png")`, `read(sys.inputs.data)` — cannot be
  discovered by walking the syntax tree. Those files would be missing on the
  first compile.
* **Recovery is possible but ugly:** compile, collect the "file not found"
  diagnostics, fetch what was missing, compile again. Two or three compiles for
  a document that uses a computed path, and a flash of wrong diagnostics.
* **Cost: low.** ~100 lines, no headers required, works anywhere.
* It is the honest fallback if Option A is unavailable, and it should be labelled
  as degraded rather than presented as equivalent.

### Option C — no file system

Single-file documents only. Imports, images, and bibliographies all fail.

* Genuinely useful for a scratchpad, and near-zero work.
* Not what anyone means by "typst support in vscode.dev".

### Not an option: threads

`wasm32-unknown-unknown` has no threads, which is precisely why
[`SingleThreaded`](../../../../crates/typst/typst-lsp-wasm/src/single_threaded.rs)
is sound. A browser build does not change that — the worker is still
single-threaded internally. Option A blocks a worker thread from the *outside*;
it does not introduce concurrency inside the WASM instance, and the
`compile_error!` guard stays valid.

## 3. The rest of the work

Smaller, and none of it novel:

| Piece | Work |
| --- | --- |
| Fonts | Bundled fonts fetched over HTTP from the extension's own resources rather than read from disk. `FontIndex` needs an async loader; the two-stage design already tolerates fonts arriving late |
| Packages | `fetch` already; the cache moves from disk to IndexedDB or is dropped in favour of re-fetching |
| Transport | `vscode-languageclient/browser` plus a `Worker`. The `typst/*` extensions are transport-agnostic and need no change |
| Build | A second `wasm-pack --target web` output, so the VSIX carries both. **+26 MB** — the artifact would roughly double the package size, which is its own argument for making the browser build a separate extension rather than a second target in this one |
| Clock | `Date.now()` in the worker. Already a port; nothing to redesign |

## 4. Recommendation

**Do not start until Option A's blocker is resolved.** The specific thing to
find out first, in this order:

1. Does a `Worker` spawned by a VSCode web extension have
   `crossOriginIsolated === true`? One line in a throwaway extension answers it.
2. If yes, build Option A. The engine, the LSP layer, and the preview all carry
   over unchanged; the work is the ring buffer and the build plumbing.
3. If no, decide whether Option B's degraded behaviour is worth shipping. It
   probably is for reading documents, and probably is not for writing them.

**Estimated size** once the blocker is resolved: 2–3 days for Option A
(SharedArrayBuffer transport, async font loading, build wiring), plus whatever
the +26 MB artifact question turns into.

**Do not fold it into this extension.** A browser build that doubles the VSIX for
every desktop user to serve a different runtime is the kind of coupling that
gets regretted. `typst-ultra-web` sharing the same crates is the better shape,
and the crates are already arranged for it — the only WASM-aware code is 400
lines in one crate.

## 5. Status

**Scoped, not implemented.** P4-06's precondition is met; the task itself
remains open, blocked on the question in §4.1.
