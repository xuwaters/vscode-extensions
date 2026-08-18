# RFC 010 — Real-World Corpus

Closes the research debt the feasibility spike opened in its own §10: every
latency and SVG-size number in the RFC came from **synthetic `#lorem`
documents**, with no images, figures, bibliographies, large tables, or vector
graphics. The spike said plainly that all of them could be optimistic. Owned by
[P4-08](../tasks/phase-4-polish.md) and [P4-10](../tasks/phase-4-polish.md).

**Verdict: mostly confirmed, with two real corrections.** Page size is ~20%
larger than the spike measured, and cold compile on a long structured book
misses the RFC's target. Everything else holds, and `evictAge: 1` survives.

---

## 1. The corpus

Four documents of deliberately different shape, in
[`crates/typst/typst-session/examples/corpus/`](../../../../crates/typst/typst-session/examples/corpus/):

| Fixture | Shape | What it stresses |
| --- | --- | --- |
| `paper.typ` | Two-column conference paper | Floats, a table, a bibliography, cross-references, numbered display math |
| `book.typ` | 25 chapters, A5, ~104 pages | Length, an outline, running headers, `context` in the page header |
| `slides.typ` | 30 16:9 slides | Many small pages, heavy per-page styling, a `#page` call per slide |
| `graphics.typ` | 8 pages of scatter plots and grids | Thousands of vector elements per page |

`graphics.typ` stands in for a CeTZ or Fletcher document and says so in its own
header. The package registry is not reachable from a benchmark that has to run
in CI, so the shapes are drawn with the standard library — the property that
matters is thousands of vector primitives per page, which is what makes those
packages expensive, not the package machinery itself.

## 2. Under WASM, which is where it runs

Measured by
[`extensions/typst-ultra/server/corpus.test.ts`](../../../../extensions/typst-ultra/server/corpus.test.ts),
through the real artifact in a Node process. Every RFC number is a claim about
this configuration.

| Document | Pages | Cold | Keystroke | p95 | Largest page | Heap |
| --- | --- | --- | --- | --- | --- | --- |
| paper | 2 | 195 ms | 3.1 ms | 4.2 ms | **470 KB** | 23 MB |
| book | 104 | **524 ms** | 39.4 ms | 43.2 ms | 419 KB | 222 MB |
| slides | 61 | 25 ms | 2.7 ms | 3.2 ms | 52 KB | 222 MB |
| graphics | 9 | 54 ms | 1.4 ms | 1.5 ms | 147 KB | 222 MB |

Heap is process-wide and cumulative — it does not fall between fixtures, because
WASM linear memory is never returned to the OS. 222 MB is the peak after
compiling all four, and it is reached by the book.

| | macOS 26.0 (Darwin 25.6.0), aarch64 · Node 20 · 2026-08-17 |
| --- | --- |

## 3. Natively, for comparing documents to each other

`cargo run --release -p typst-session --example corpus`. Not interchangeable
with the table above — native has multi-threaded `rayon` layout that WASM does
not — but useful for seeing which document is intrinsically expensive.

| Document | Pages | Cold | Keystroke | p95 | Largest page | All pages | 1 page render |
| --- | --- | --- | --- | --- | --- | --- | --- |
| paper | 2 | 82 ms | 0.9 ms | 1.2 ms | 470 KB | 0.9 MB | 6.7 ms |
| book | 104 | 256 ms | 17.4 ms | 22.0 ms | 419 KB | **26.0 MB** | 3.6 ms |
| slides | 60 | 14 ms | 1.4 ms | 1.9 ms | 52 KB | 1.7 MB | 0.6 ms |
| graphics | 8 | 22 ms | 0.9 ms | 1.4 ms | 147 KB | 1.2 MB | 2.1 ms |

The WASM tax on these documents is **2.0–2.4×** for cold compile and ~2.3× for a
keystroke. That is a real, consistent constant factor, and it is the first
controlled native-vs-WASM comparison this project has — the spike's
[§4.3](spike.md#43-native-baseline-same-document-same-machine) explicitly was
not one.

## 4. What changed

### A real page is 470 KB, not 386 KB

The spike's densest page was 386 KB. The two-column paper's is **470 KB** — 22%
larger, because two columns pack more glyphs onto a page than one does.

Consequences: the transport measurement in [transport.md](transport.md) was
taken at 394 KB, so it is ~16% optimistic. Scaling linearly puts the wire at
~4.4 ms instead of 3.7 ms, which changes nothing about the conclusion. Page
budgets elsewhere should say "~470 KB" rather than "~386 KB".

### Cold compile on a long book misses the target

[proposal.md §8](../proposal.md#8-performance-targets) sets *cold compile,
75 pages < 300 ms*, from a spike measurement of 262 ms. The 104-page book takes
**524 ms**.

Normalized: **5.0 ms/page** against the spike's 3.5 ms/page — the structured
document is ~44% more expensive per page. An outline, running headers with
`context`, and real heading numbering all cost something the synthetic corpus
never paid.

This is a target miss, not a design problem:

- It is a **cold** compile, paid once when a document is opened. The keystroke
  path on the same document is 39 ms.
- The user sees it as a first-open delay, with a status-bar spinner already
  showing.

The honest fix is to the target, not the code: it should be expressed per page
(**< 6 ms/page cold**), because "75 pages" was always a proxy for size, and the
per-page figure is the one that transfers between documents. Left for a follow-up
amendment rather than edited into `proposal.md` here, since changing a stated
target is a scope decision.

### Graphics-heavy pages are *cheaper* than text

A scatter plot with 400 points is 147 KB, against 470 KB for a page of prose.
Vector primitives are compact; the expensive thing in a typst page is one `<use>`
element per glyph. The RFC's worry that "graphics-heavy documents could be much
worse" does not hold for vector graphics — though raster images, which this
corpus does not include, remain untested.

## 5. P4-10: the eviction sweep, re-run

The `evictAge: 1` default was chosen from a synthetic sweep
([spike.md §4.2](spike.md#42-eviction-age-sweep)). Re-run natively on the real
corpus:

| Document | Age | Warm-up | Steady | p95 |
| --- | --- | --- | --- | --- |
| paper | **1** | **1.0 ms** | **1.0 ms** | **1.2 ms** |
| paper | 3 | 1.1 ms | 1.1 ms | 1.3 ms |
| paper | 10 | 1.3 ms | 1.5 ms | 1.8 ms |
| book | **1** | **17.8 ms** | **18.1 ms** | **18.6 ms** |
| book | 3 | 18.8 ms | 18.1 ms | 19.5 ms |
| book | 10 | 22.4 ms | 20.3 ms | 22.8 ms |

**Age 1 still wins on every axis at every size.**
[Decision 0005](../decisions/0005-cache-eviction-policy.md) stands, now on
evidence from documents people would actually write.

The margins are narrower than the synthetic sweep's — 18.6 ms vs 22.8 ms at p95
here, against 18 ms vs 278 ms there. The synthetic document repeated one section
shape 75 times, which made higher ages retain an unusually large amount of
never-reused layout. Real documents vary more per page, so there is less
identical work to over-cache. The ordering is unchanged; the drama is not.

## 6. What is still not covered

Named so nobody reads more into the table than is there:

- **Raster images.** The corpus has none; every figure is vector. A document
  full of photographs would test image decoding and embedding, which nothing
  here touches.
- **Packages.** `graphics.typ` imitates CeTZ's *shape* but imports nothing. The
  cost of resolving, downloading, and compiling a real Universe package is
  measured nowhere.
- **Paint.** Page sizes and render times are measured; what a browser does with
  a 470 KB page is not. See [transport.md §3](transport.md#3-what-is-still-open).
- **One machine, one OS.** aarch64 macOS, as before —
  [P4-09](../tasks/phase-4-polish.md).
