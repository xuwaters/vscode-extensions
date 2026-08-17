# RFC 010 — Feasibility Spike

Everything in this document was **measured**, not estimated. The prototype is a standalone crate that
depends on unmodified upstream typst 0.15.1 from crates.io, implements a minimal `World`, and is built for
`wasm32-unknown-unknown` and run under Node.

The spike lives in the scratch area (`target/tmp/claude/typst-wasm-spike`, gitignored) and is disposable —
[§11](#11-reproducing) records what it did so it can be rebuilt. Its purpose was to kill the RFC early if
any load-bearing assumption failed. None did; two measurements changed the design
([§4.1](#41-the-measurement-that-changed-the-design), [§4.2](#42-eviction-age-sweep)) and one killed a
planned feature ([§8](#8-typc-and-code-mode)).

Every number here feeds a [decision record](../decisions/README.md). When a measurement is redone, update
this document and check whether the decisions that cite it still hold.

---

## 1. Environment

| | |
| --- | --- |
| Host | macOS (Darwin 25.6.0), aarch64 |
| Toolchain | rustc 1.99.0-nightly, cargo 1.99.0-nightly |
| Targets | `aarch64-apple-darwin` (baseline), `wasm32-unknown-unknown` |
| wasm-bindgen | 0.2.118 (crate pinned `=0.2.118` to match the installed CLI) |
| binaryen | `wasm-opt` from Homebrew |
| Upstream | `typst`, `typst-layout`, `typst-svg`, `typst-render`, `typst-pdf`, `typst-html`, `typst-eval`, `typst-ide`, `typst-syntax` — all `0.15.1`; `typstyle-core` `0.15.1`; `comemo` `0.5` |
| Profile | `opt-level = "s"`, `lto = true` |

---

## 2. Does upstream typst build for WASM, unpatched?

**Yes.** `cargo check --target wasm32-unknown-unknown` locked and compiled **304 crates** with zero
patches, zero feature juggling, and zero build errors from the dependency graph. The only compile errors
were in spike code that had drifted from 0.15.1's API (`FileId::new` now takes a `RootedPath`,
`PagedDocument::pages` is a method, `today()` takes `Option<Duration>`); once fixed, the build was clean.

This is the single most important result in the RFC. It is what makes the "no fork" goal
([proposal.md §6.1](../proposal.md#61-unmodified-upstream-typst)) affordable.

Two things that could have blocked it and did not:

- **`rayon`** is a dependency of `typst-library` and `typst-utils` (parallel page layout, `Deferred`).
  It compiles for `wasm32-unknown-unknown` and runs single-threaded without panicking:

  ```
  rayon_threads=1 available=Err(Error { kind: NotFound,
    message: "the number of hardware threads is not known for the target platform" })
  ```

- **`typst-timing`** already handles `wasm32` explicitly — it has a `wasm` feature using `web-sys`
  `Performance`, and *without* that feature its `Timestamp::now()` returns `0.0` on wasm rather than
  calling `SystemTime::now()` ([`temp/typst/crates/typst-timing/src/lib.rs:243`](../../../../temp/typst/crates/typst-timing/src/lib.rs#L243)).
  We leave the feature off; timing is disabled by default anyway.

---

## 3. Artifact size

| Build | Raw `.wasm` | After `wasm-opt -Os --strip-debug --strip-producers` | gzipped |
| --- | --- | --- | --- |
| Compile + layout + SVG + IDE + syntax + typstyle | 34.0 MB | **22.4 MB** | **8.4 MB** |
| …plus PDF export (`typst-pdf`/krilla) and jump reachable | 37.3 MB | **24.4 MB** | **9.1 MB** |
| Same, `opt-level = 3` instead of `"s"` | 39.4 MB | — | — |

Two conclusions:

- **PDF export costs ~2 MB optimized / ~0.7 MB gzipped.** Cheap enough to include unconditionally.
- **`opt-level = 3` is not worth it.** It grows the artifact by 2 MB and, per §4, produced no measurable
  speedup. Keep the workspace's `opt-level = "s"`.

`wasm-opt` took 14 s wall (8.9× parallel) — fine for `package`, not for `watch`.

---

## 4. Compile latency

### 4.1 The measurement that changed the design

The first latency run reported **411 ms** for a single-keystroke recompile of a 30-page document. Native
was 13 ms. A 30× gap would have made the live preview untenable and forced a rethink.

It was the harness, not WASM. The spike called `comemo::evict()` **before** each compile. `evict` itself
is cheap (0–1 ms), but it drops memoized layout entries, so the *following* compile has to redo the work
it just discarded. typst-cli's watch loop gets the order right —
[`temp/typst/crates/typst-cli/src/watch.rs:82`](../../../../temp/typst/crates/typst-cli/src/watch.rs#L82)
calls `comemo::evict(10)` **after** `compile_once`. Reordering to match:

| Document | Wrong order (evict → compile) | Correct order (compile → evict) |
| --- | --- | --- |
| 30 pages, keystroke recompile | 411 ms | **7 ms** |

This is now a documented invariant of the design
([architecture.md §6](../design/architecture.md#6-memory-and-the-comemo-cache)), and it gets a regression test.

### 4.2 Eviction age sweep

Method: build a synthetic A4 document (`= Section n` + `#lorem(140)` + a display equation + `#lorem(120)`,
repeated), compile once cold, then perform 40 single-character insertions at the end of the document,
compiling and **then** evicting after each. **One fresh Node process per configuration** — comemo's cache is
global, so running configurations in a single process contaminates the cold-compile column.

| Pages | evict age | Cold | Warm-up (edits 1–10) | Steady (31–40) | p95 | WASM heap |
| --- | --- | --- | --- | --- | --- | --- |
| 10 | **1** | 81 ms | **3 ms** | 3 ms | **4 ms** | **32 MB** |
| 10 | 3 | 81 ms | 3 ms | 3 ms | 6 ms | 41 MB |
| 10 | 5 | 80 ms | 5 ms | 3 ms | 5 ms | 50 MB |
| 10 | 10 | 80 ms | 6 ms | 3 ms | 8 ms | 71 MB |
| 30 | **1** | 134 ms | **8 ms** | 6 ms | **9 ms** | **55 MB** |
| 30 | 3 | 137 ms | 8 ms | 7 ms | 20 ms | 81 MB |
| 30 | 5 | 132 ms | 15 ms | 6 ms | 35 ms | 106 MB |
| 30 | 10 | 133 ms | 41 ms | 7 ms | 57 ms | 170 MB |
| 75 | **1** | 262 ms | **17 ms** | 15 ms | **18 ms** | **106 MB** |
| 75 | 3 | 261 ms | 19 ms | 16 ms | 107 ms | 170 MB |
| 75 | 5 | 260 ms | 85 ms | 15 ms | 166 ms | 234 MB |
| 75 | 10 | 259 ms | 196 ms | 15 ms | 278 ms | 394 MB |

The result is monotonic and one-sided: **age `1` is best on every axis at every size.**

- **Steady-state latency is identical across ages** (3 / 6 / 15 ms). Keeping more cache buys nothing once
  the working set is hot.
- **Warm-up and p95 are where the ages differ**, and they differ a lot: at 75 pages, age 1 has a p95 of
  18 ms against age 10's 278 ms. Tail latency is what a typist actually feels.
- **Heap scales with age**, 2.2–3.7× between age 1 and age 10.
- **Cold compile is unaffected** by eviction age, as expected — there is nothing cached yet.

This is counter-intuitive ("more cache should be faster") but consistent: age `1` still retains everything
touched by the immediately preceding compile, which *is* the working set incremental recompilation needs.
Higher ages retain generations that will never be reused, slowing lookups and holding memory.

Resolved in [decisions/0005](../decisions/0005-cache-eviction-policy.md): default `evictAge` to `1`, not
typst-cli's `10`.

> ⚠️ Superseded numbers: an earlier revision of this document reported 423 MB at 75 pages and a 233 ms
> warm-up. Those were age-10 measurements, quoted before the sweep was run. The age-1 figures above are the
> ones the design now assumes.

### 4.3 Native baseline (same document, same machine)

| Sections | Pages | Cold compile | Keystroke recompile |
| --- | --- | --- | --- |
| 5 | 3 | 22 ms | 4 ms |
| 20 | 10 | 49 ms | 6 ms |
| 60 | 30 | 143 ms | 13 ms |

Comparing against §4.2's age-1 column at 30 pages: cold 134 ms (WASM) vs 143 ms (native), incremental 6 ms
(WASM) vs 13 ms (native).

Those numbers say WASM is at parity or better, which cannot be literally true — native has multi-threaded
`rayon` layout that WASM does not. The likely explanation is that the two harnesses differ: the native
benchmark rebuilds its `SpikeWorld` per configuration and does not use the identical eviction schedule.
**Treat the native column as indicative, not as a controlled comparison.**

What the comparison does support, and all the design needs, is the weaker claim: **the WASM tax is within a
small constant factor, not an order of magnitude.** A properly controlled native-vs-WASM benchmark is worth
running if cold-compile latency ever becomes the binding constraint; it is not one today.

### 4.4 comemo works correctly in WASM

Recompiling with no edit at all:

```
cold compile:        131ms  (30 pages)
  noop recompile 1:  0.6ms
  noop recompile 2:  0.0ms
  noop recompile 3:  0.0ms
```

Memoization is fully effective. And with *no* eviction at all, latency degrades as the cache grows
(15 → 21 → 33 → 36 → 41 ms over five edits) while heap climbs ~128 MB per 10 edits — confirming that
eviction is necessary, just not before the compile.

---

## 5. Do the IDE features work in WASM?

All exercised on a real document with fonts loaded, in Node:

```
pages=1 first_svg_bytes=64098 pdf_bytes=20275 highlights=74
formatted_bytes=331 ide=(completions=181 tooltip=true definition=true) diags=[]
```

| Capability | Upstream entry point | Result |
| --- | --- | --- |
| Completion | `typst_ide::autocomplete` | 181 items at a bare cursor |
| Hover | `typst_ide::tooltip` | returns a tooltip |
| Goto-definition | `typst_ide::definition` | resolves |
| Semantic tokens | `typst_syntax::highlight` walk | 74 tagged nodes |
| Formatting | `typstyle_core::Typstyle::format_source(..).render()` | 331 bytes out |
| SVG export | `typst_svg::svg(page, opts)` | 64 KB for a small page |
| PDF export | `typst_pdf::pdf(doc, opts)` | 20 KB valid PDF |
| Cursor → page position | `typst_ide::jump_from_cursor` | returns positions |

Note `typst_ide::autocomplete`, `tooltip`, and `definition` all take `Option<impl AsOutput>` — a compiled
document is optional. That is what makes the "IDE requests never wait on a compile" rule in
[architecture.md §5](../design/architecture.md#5-concurrency-model-one-thread-two-clocks) implementable: pass the
last-good document, or `None`, and answer immediately.

---

## 6. Synchronous host VFS callbacks

The design's foundation is that Rust calls back into JS **synchronously, from inside `World::file`, in the
middle of a compile**. If that did not work, the whole VFS/font/package architecture would need an async
redesign.

The spike implemented a `World` whose `file()` calls a `js_sys::Function`, backed by Node's
`fs.readFileSync`, and compiled a document with a real `#import`:

```typst
#import "helper.typ": boxed, TITLE
= #TITLE
#boxed[Imported function works.]
```

```
ok pages=1 host_reads=[helper.typ]
sync host callbacks made: 1
```

And the failure path produces a correct typst diagnostic rather than a crash:

```
err ["file not found (searched at missing.typ)"] host_reads=[missing.typ]
```

Confirmed: synchronous host services work under `wasm-bindgen --target nodejs`, mid-compile, including
error propagation through `FileResult`.

---

## 7. Page SVG anatomy

This drives the preview protocol. A text-heavy A4 page from a 20-section document:

| Metric | Value |
| --- | --- |
| Page SVG | 386,344 bytes |
| gzipped | 37,653 bytes (10.3×) |
| `<defs>` block (glyph outlines) | 46,611 bytes, 58 `<symbol>` elements |
| Body | 3,144 `<use>` elements, 61 `<path>` elements |

Whole-document totals (fresh compile each time, so these include cold cost):

| Pages | Compile + render all pages | Total SVG | Largest page | PDF |
| --- | --- | --- | --- | --- |
| 1 | 11 ms | 212 KB | 212 KB | 18 KB |
| 3 | 24 ms | 972 KB | 380 KB | 28 KB |
| 10 | 71 ms | 3.8 MB | 382 KB | 64 KB |
| 30 | 195 ms | 11.4 MB | 382 KB | 161 KB |

Design consequences, carried into [preview.md](../design/preview.md):

1. **Glyph outlines are only 12% of a page.** The bulk is `<use href=… x=… y=…>` — one per glyph, at full
   float precision. There is no cheap win from deduplicating glyphs across pages.
2. **Never render the whole document.** 11.4 MB for 30 pages is prohibitive. Render only pages in and near
   the viewport.
3. **Diff per page.** ~5 ms to render one page's SVG means a keystroke costs one page render, not thirty.
4. `typst_svg::svg_merged(document, opts, gap)` exists upstream for whole-document output — the right tool
   for *export*, not for the live preview.

---

## 8. `.typc` and code mode

Typst tooling has a convention that `.typ` is markup mode and `.typc` is code mode; tinymist ships a
separate `typst-code` language id for it. Before mirroring that, the spike asked what the **compiler**
actually does, by importing two `.typc` files — one written in genuine code mode, one in markup mode:

```typst
// helpers.typc, code mode
let TITLE = "code-mode file"

// helpers2.typc, markup mode
#let TITLE2 = "markup-mode file with .typc extension"
```

```
import .typc written in CODE mode:    err ["unresolved import"]
import .typc written in MARKUP mode:  ok pages=1
```

**Upstream typst parses every imported file as markup, regardless of extension.** A `.typc` written in
genuine code mode does not import.

`typst-syntax` does expose `parse_code` alongside `parse`, and `Source::with_root` would let us build a
code-mode tree. But the incremental reparser's fallback path calls `parse(text)` unconditionally
([`temp/typst/crates/typst-syntax/src/reparser.rs:23`](../../../../temp/typst/crates/typst-syntax/src/reparser.rs#L23)),
so a code-mode `Source` silently reverts to markup on the first non-incremental edit. There is no way to
hold a durable code-mode `Source` on the published API.

Resolved in [decisions/0009](../decisions/0009-file-extensions.md): claim `.typ` and `.typc` under one
`typst` language id, both parsed as markup — matching the compiler rather than the convention.

---

## 9. Workspace layout for `crates/typst/`

The root workspace uses `members = ["crates/*"]`. A grouping directory without a `Cargo.toml` breaks it:

```
members = ["crates/*", "crates/typst/*"]
→ error: failed to load manifest for workspace member `…/crates/typst`
  referenced via `crates/*`
```

`exclude` alone also fails — it excludes the entire subtree, children included:

```
members = ["crates/*", "crates/typst/*"], exclude = ["crates/typst"]
→ members: ['a']          # crates/typst/b silently dropped
```

Two forms work, both verified with `cargo metadata`. **Explicit members override `exclude`**, which lets
the existing glob stay:

```toml
members = ["crates/*", "crates/typst/b"]
exclude = ["crates/typst"]
→ members: ['a', 'b']     ✅
```

```toml
members = ["crates/a", "crates/typst/*"]   # fully enumerate the top level
→ members: ['a', 'b']     ✅
```

The RFC adopts the first: keep `crates/*`, add `exclude = ["crates/typst"]`, and list each typst crate
explicitly. New typst crates must be added to `members` by hand — a one-line cost, noted in
[crates.md §7](../design/crates.md#7-build-and-workspace-registration).

---

## 10. What the spike did *not* cover

Stated plainly so nobody reads more into the numbers than is there. Each gap is tracked as a
[research debt](../tasks/README.md#research-debts) owned by a task, so it gets closed rather than quietly
inherited.

| Not covered | Consequence | Owner |
| --- | --- | --- |
| **Synthetic documents only** — `#lorem` plus one display equation per section. No images, figures, bibliographies, large tables, or CeTZ/Fletcher diagrams | Every latency and SVG-size number could be optimistic for graphics-heavy documents | [P4-08](../tasks/phase-4-polish.md) |
| **No LSP layer.** All figures are engine-level; JSON-RPC, Node IPC, and extension-host handling sit on top | ~50 ms of the 65 ms preview budget is estimated, not measured | [P3-05](../tasks/phase-3-preview.md) |
| **No packages.** Universe download, extraction, caching unimplemented | An entire Phase-2 feature is unproven | [P2-14](../tasks/phase-2-ide-features.md) |
| **No system fonts.** 17 bundled files were loaded; indexing hundreds of MB of installed fonts is unmeasured | First-run cost on a font-heavy machine is unknown | [P2-13](../tasks/phase-2-ide-features.md) |
| **Single file.** Multi-file import proven functional ([§6](#6-synchronous-host-vfs-callbacks), [§8](#8-typc-and-code-mode)) but never benchmarked | Diagnostic fan-out cost across a compile graph is unknown | [P1-16](../tasks/phase-1-foundation.md) |
| **Font parsing cost never isolated** — reading 17 files took 4 ms, but `FontInfo` extraction was folded into a cold-start number | Affects the "server start < 400 ms" target | [P1-11](../tasks/phase-1-foundation.md) |
| **One machine, one OS** — aarch64 macOS | WASM makes cross-platform parity likely, not certain | [P4-09](../tasks/phase-4-polish.md) |
| **Native baseline not controlled** ([§4.3](#43-native-baseline-same-document-same-machine)) | The native-vs-WASM ratio is indicative only | — (not blocking) |

---

## 11. Reproducing

The spike crate depends only on crates.io. To rebuild it:

```bash
cargo new --lib typst-wasm-spike && cd typst-wasm-spike
# Cargo.toml: crate-type = ["cdylib", "rlib"]; deps as listed in §1;
#             [profile.release] opt-level = "s", lto = true
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --out-dir pkg --out-name typst_spike \
  target/wasm32-unknown-unknown/release/typst_wasm_spike.wasm
node run.mjs <path-to-typst-assets-fonts>
```

Fonts come from the `typst-assets` 0.15.1 crate (`files/fonts/`, 17 `.ttf`/`.otf` files, 9.5 MB).

If any of this needs to become permanent, it belongs in `crates/typst/typst-session/benches/`, not in a
scratch directory — see [proposal.md §13](../proposal.md#13-testing).
