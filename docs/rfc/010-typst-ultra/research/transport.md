# RFC 010 — Transport Latency

Closes the RFC's largest research debt: roughly 50 ms of the 65 ms preview
repaint budget was **estimated**, not measured
([spike.md §10](spike.md#10-what-the-spike-did-not-cover),
[preview.md §8](../design/preview.md#8-performance-budget)). Owned by
[P3-05](../tasks/phase-3-preview.md), which the phase deliberately sequenced
*before* the webview work, because a bad answer would have promoted
[0006](../decisions/0006-preview-rendering.md)'s escape hatches out of Phase 4.

**It was not a bad answer.** The wire is nearly free.

---

## 1. Method

Measured by
[`extensions/typst-ultra/server/transport.test.ts`](../../../../extensions/typst-ultra/server/transport.test.ts),
which runs as part of `pnpm test` and doubles as a regression guard.

- A `#lorem`-heavy A4 page is compiled and rendered by the **real engine**
  through the **real WASM artifact** — not a synthetic string.
- That page renders to 220 KB. To stay comparable with the spike's densest page
  (386 KB, [§7](spike.md#7-page-svg-anatomy)), the payload is grown to 394 KB by
  repeating its `<use>` elements, which are 88% of a real page's bytes.
- Each step is timed over 10–20 runs after a warm-up; the median is reported.
- Node IPC is a **real** round trip through a forked child process with an `ipc`
  channel — the transport `TransportKind.ipc` actually uses. Fork cost is
  excluded; what is timed is the message crossing.

| | |
| --- | --- |
| Host | macOS (Darwin 25.6.0), aarch64 |
| Node | v20+ (repo minimum 20.19.0) |
| Payload | 394 KB page SVG inside a `typst/renderPages` response |
| Date | 2026-08-17 |

---

## 2. Results

| Step | Estimated ([preview.md §8](../design/preview.md#8-performance-budget)) | **Measured** |
| --- | --- | --- |
| JSON-RPC serialize (server) | — | **0.4 ms** |
| JSON-RPC parse (host) | — | **0.4 ms** |
| Node IPC round trip | ~15 ms (with serialization) | **2.9 ms** |
| `postMessage` to webview | ~15 ms | **0.1 ms** (`structuredClone` proxy) |
| **Wire total** | **~30 ms** | **3.7 ms** |
| `DOMParser` + adopt + paint | ~20 ms | **not closed** — see §3 |

**The wire half is ~8× cheaper than estimated.** Serialization is negligible;
the single measurable cost is the IPC hop, and 2.9 ms for 394 KB is roughly
136 MB/s, which is what a pipe on this machine does.

### What this changes

- **The repaint budget is comfortable.** Engine 7 ms + measure 2 ms + one page
  render 5 ms + wire 3.7 ms ≈ **18 ms**, against a 120 ms target — before the
  DOM step, which is the only remaining unknown.
- **[0006](../decisions/0006-preview-rendering.md)'s escape hatches stay in
  Phase 4.** Neither coordinate rounding ([P4-11](../tasks/phase-4-polish.md))
  nor PNG mode ([P4-05](../tasks/phase-4-polish.md)) needs promoting; the
  condition that would have forced it did not occur.
- **Per-page diffing is still what matters.** 3.7 ms is per *page*; shipping all
  thirty pages of a document on every keystroke would be ~110 ms of wire alone.
  The hash diff is doing the work, not the transport being fast.

---

## 3. What is still open

The DOM half. `happy-dom` reports **46 ms** to parse and strip a 394 KB page,
but that number is **not usable in either direction**:

- `happy-dom` is a pure-JavaScript DOM. Chromium's XML parser is native and
  should be substantially faster, so 46 ms is likely a large overestimate of the
  parse.
- `happy-dom` does **no layout and no paint at all**. The number that would
  actually matter — what a compositor does with ~3,000 `<use>` elements — is not
  measurable from a Node process.

So the honest statement is: **the wire is measured and cheap; the DOM step is
still an estimate.** Closing it needs a real browser, and belongs with
[P4-08](../tasks/phase-4-polish.md)'s real-world corpus work, where a webview
can be instrumented against documents that are actually heavy.

The `happy-dom` figure is nonetheless recorded here rather than dropped, because
it is a genuine upper bound on the *parse*: even at 46 ms — a pure-JS parser,
no native acceleration — the total stays under the original 65 ms budget.

---

## 4. Regression guard

The test asserts only the **wire total** (< 30 ms, the original estimate) and
reports the DOM number without asserting it. Asserting a `happy-dom` timing
would make the test a coin flip on a loaded CI machine while telling us nothing
about a browser.

If the wire assertion ever fails, the likely causes in order are: a payload that
stopped being diffed per page, a JSON encoding regression, or a change of
transport away from `ipc`.
