# 0005 — Compile then evict; default eviction age `1`

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 4

## Context

[comemo](https://crates.io/crates/comemo) is typst's memoization layer. It is why an incremental recompile
is 6 ms instead of 137 ms, and it is also the only thing standing between a long editing session and
unbounded memory growth. `comemo::evict(max_age)` drops entries not used within the last `max_age`
generations, where a generation is one `evict` call.

Two things had to be settled: **when** to evict relative to the compile, and **what age** to default to.

The "when" was settled the hard way. The first prototype called `evict` *before* each compile and measured
411 ms per keystroke on a 30-page document — a 50× regression, because evicting first discards exactly the
memoized layout the compile is about to need. typst-cli's watch loop gets it right
([`temp/typst/crates/typst-cli/src/watch.rs:82`](../../../../temp/typst/crates/typst-cli/src/watch.rs#L82)):
compile, *then* `comemo::evict(10)`.

The "what age" was open. A full sweep across ages and document sizes
([research/spike.md §4.2](../research/spike.md#42-eviction-age-sweep)):

| Pages | Age | Warm-up (edits 1–10) | Steady (31–40) | p95 | Heap |
| --- | --- | --- | --- | --- | --- |
| 10 | **1** | **3 ms** | 3 ms | **4 ms** | **32 MB** |
| 10 | 10 | 6 ms | 3 ms | 8 ms | 71 MB |
| 30 | **1** | **8 ms** | 6 ms | **9 ms** | **55 MB** |
| 30 | 10 | 41 ms | 7 ms | 57 ms | 170 MB |
| 75 | **1** | **17 ms** | 15 ms | **18 ms** | **106 MB** |
| 75 | 10 | 196 ms | 15 ms | 278 ms | 394 MB |

The result is monotonic and one-sided: **age `1` wins on every axis at every size.** Steady-state latency
is identical, warm-up is 2–11× better, p95 is 2–15× better, and heap is 2.2–3.7× smaller. Higher ages buy
nothing measurable and cost both memory and tail latency.

This is not intuitive — "keep more cache" sounds like it should be faster — but age `1` still retains
everything touched by the immediately preceding compile, which is precisely the working set incremental
recompilation needs. Everything else is dead weight that slows lookups and holds memory.

## Decision

1. **Compile, then evict. Always.** `typst-session` exposes this as a single `Session::compile` method so a
   caller cannot get the order wrong, and a unit test asserts the warm/cold recompile ratio to catch a
   regression.
2. **Default `typstUltra.memory.evictAge` to `1`**, not typst-cli's `10`. Our workload is per-keystroke
   editing; typst-cli's is whole-file saves in a watch loop. The setting is exposed for users who want to
   trade memory for something, but the measurements say there is nothing to trade for.

## Consequences

- Peak heap at 75 pages drops from 394 MB to **106 MB**, which materially improves the memory story in
  [0003](0003-server-in-child-process.md) — noted there rather than silently benefiting from it.
- p95 keystroke latency at 75 pages drops from 278 ms to **18 ms**. Tail latency is what users actually
  feel, so this is the headline number.
- We deviate from upstream's own default. If a future comemo release changes eviction semantics, this
  decision is the first thing to re-measure.

## Revisit if

- comemo changes its eviction or generation semantics in a new release.
- Real-world documents (images, CeTZ diagrams, large bibliographies — none covered by the synthetic
  benchmark) show age `1` thrashing where the synthetic corpus does not.
- A workload appears where compiles are *not* triggered per keystroke, e.g.
  `typstUltra.compile.when: "onSave"`, where a larger age might genuinely help. Worth measuring rather than
  assuming.
