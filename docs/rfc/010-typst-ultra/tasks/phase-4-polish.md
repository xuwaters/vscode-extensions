# Phase 4 — Polish and stretch

**Goal**: the features that are genuinely useful but not load-bearing, plus closing the RFC's remaining
research debts.
**Exit criterion**: none — every item here is independently shippable and independently droppable.
**Status**: ◐ 14 / 16 done, 1 scoped, 1 blocked

Unlike Phases 1–3, this list is a menu, not a sequence. Items are ordered by expected value.

## Validation — do these first

These close [research debts](README.md#research-debts) that the whole RFC currently rests on. If P4-08
finds that real documents behave nothing like the synthetic corpus, several earlier decisions need
revisiting — better to learn that early.

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-08 | Real-world benchmark corpus: a paper with images + bibliography + tables, a CeTZ/Fletcher-heavy document, a 200-page book, a presentation. Re-run compile latency, SVG page size, and heap. Update [research/spike.md](../research/spike.md) and any decision the results contradict | [spike.md §10](../research/spike.md#10-what-the-spike-did-not-cover) | ☑ |
| P4-09 | Verify on Windows and Linux: font discovery paths, package cache directory, path separators in the VFS, child-process startup | [spike.md §10](../research/spike.md#10-what-the-spike-did-not-cover) | ⊗ |
| P4-10 | Re-run the eviction sweep against the real-world corpus; confirm or revise the `evictAge: 1` default | [0005](../decisions/0005-cache-eviction-policy.md) | ☑ |

**P4-08 — closed, with two corrections.** Full record in [research/corpus.md](../research/corpus.md).
Four documents, measured both natively and through the real WASM artifact. What changed:

- **A real page is 470 KB, not 386 KB.** The two-column paper packs more glyphs per page than the
  spike's single-column fixture.
- **Cold compile on a 104-page structured book is 524 ms**, against the RFC's *< 300 ms at 75 pages*
  target. Normalized that is 5.0 ms/page against 3.5 ms/page — a structured document with an outline and
  running headers costs ~44% more per page than `#lorem`. It is a **cold** compile, paid once per open.
  The target should be restated per page; that is a scope decision, so it is recorded rather than edited
  into `proposal.md`.
- **Vector graphics are *cheaper* than prose** (147 KB vs 470 KB), which was not the expectation. Raster
  images remain untested.

Also the first **controlled** native-vs-WASM comparison the project has: **2.0–2.4×**, where
[spike.md §4.3](../research/spike.md#43-native-baseline-same-document-same-machine) explicitly was not
controlled.

**P4-09 — blocked.** Verifying Windows and Linux needs Windows and Linux; neither is available here. What
was done to shrink the risk rather than pretend it away:

- Every platform branch is now **unit-tested from any host**. `systemFontCandidates(platform, env)` and
  `defaultCacheDir()` are pure, and `server/platform.test.ts` exercises the macOS, Windows, and Linux
  paths — including `XDG_CACHE_HOME`, `LOCALAPPDATA`, and the per-user Windows font directory.
- Path confinement is tested on both separators, and `VirtualPath` normalizes before we see it.
- CI runs the Rust and Node suites on **ubuntu-latest**, so Linux is covered for everything that does not
  need a real editor.

What remains genuinely unverified: a forked Node child process on Windows, real font files in
`C:\Windows\Fonts`, and a real package cache under `%LOCALAPPDATA%`. **Needs a person with those machines.**

**P4-10 — closed, default confirmed.** Age 1 still wins on every axis at every size on the real corpus.
The margins are narrower than the synthetic sweep's (18.6 ms vs 22.8 ms at p95, against 18 ms vs 278 ms),
because a repeated synthetic section over-caches in a way real documents do not. Ordering unchanged;
[0005](../decisions/0005-cache-eviction-policy.md) stands.

## Optimizations

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-05 | **PNG low-memory render mode**: `typstUltra.preview.renderMode` = `svg` \| `png` \| `auto`, with `auto` switching per page above a ~1 MB SVG threshold. Re-render on zoom-step change; document the loss of find-in-preview | [0006](../decisions/0006-preview-rendering.md) | ☑ |
| P4-11 | SVG coordinate rounding to 2 decimal places — targets the 88% of page bytes that are `<use>` positioning. Cheaper than P4-05 and worth trying first | [0006](../decisions/0006-preview-rendering.md) | ☑ |
| P4-12 | Server heap watchdog: surface `memory.restartThresholdMb` in the status bar with a one-click restart | [architecture.md §6](../design/architecture.md#6-memory-and-the-comemo-cache) | ☑ |

**P4-05 notes.** `PagePatch::Replace` now carries a `format` alongside its `content`, so a page can arrive
as SVG or as base64 PNG without a second protocol. The webview re-requests everything when the mode
changes or when the zoom crosses a half-step, because a raster page is baked at one resolution — and the
viewport message carries the zoom so the server rasterizes at the size the page will be shown. The
inversion filter cannot exempt images in PNG mode, since the page is one flat image; that is noted in the
stylesheet where someone will hit it.

**P4-11 — implemented, and worth much less than the RFC assumed.** The premise was that page bytes are
`<use>` elements "at full float precision". They are `<use>` elements, but **not at full precision**:
`typst-svg` already rounds to 9 decimal places and formats through `ryu`, which emits the shortest
round-tripping representation. Measured saving on a real page: **2.9%** (170 KB → 165 KB), not the
substantial cut the task predicted. Kept because it is free and correct, but it is **not a lever** — if
page size ever binds, P4-05 is the one that moves it.

## Language features

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-01 | Inlay hints: parameter names at call sites from `Func` metadata; default off | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features) | ☑ |
| P4-02 | Signature help from `Func` params — declared parameters, types, and defaults only; **no evaluated argument values**, which would need the fork we are not doing | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features), [0001](../decisions/0001-unmodified-upstream-typst.md) | ☑ |
| P4-03 | Code actions: add missing import, wrap in `#{…}`, string → content block, add `<label>` to a heading | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features) | ☑ |
| P4-04 | Code lenses: "Preview" and "Export as…" above the first line | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features) | ☑ |
| P4-13 | Postfix / UFCS completions (`x.rect` → `rect(x)`) — a pure syntax feature, rebuilding what tinymist offers | [lsp-features.md §3.1](../design/lsp-features.md#31-completion) | ☑ |

**P4-02 notes.** Parameter types come from `CastInfo`, which has no `Display` — `walk` flattens the union
and a long one is truncated, because a signature line is not a type reference. Only native functions carry
parameter documentation; a closure written in the document genuinely has none, which is a missing *thing*
rather than a missing export.

**P4-13 notes.** The subtlety is where to offer them. In **markup**, `#value.` parses as a value followed
by a full stop — because that is what people usually mean — and the dot is a `Text` node, not a
`FieldAccess`. Both shapes are handled, matching upstream's own `complete_field_accesses`. Items sort
under a `z` prefix so upstream's real field completions always come first.

## Stretch

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-06 | Browser-worker build for vscode.dev: `vscode-languageclient/browser` + `Worker`, `wasm-pack --target web`. Needs a VFS over `vscode.workspace.fs` (async) — a real redesign of the synchronous host services, so scope it before starting | [0003](../decisions/0003-server-in-child-process.md) | ◐ |
| P4-07 | HTML export via `typst-html` — already linked in transitively, so the cost is UX, not dependencies | [proposal.md §10](../proposal.md#10-known-limitations-from-the-no-fork-constraint) | ☑ |
| P4-14 | Template / `init` support: scaffold a project from a Universe template package | — | ☑ |
| P4-15 | Adopt or generate a full TextMate grammar, if [0007](../decisions/0007-textmate-grammar.md)'s revisit conditions are met | [0007](../decisions/0007-textmate-grammar.md) | ⊘ |
| P4-16 | Upstream contributions: propose exports for anything [0001](../decisions/0001-unmodified-upstream-typst.md)'s option 3 identified — the honest alternative to forking | [0001](../decisions/0001-unmodified-upstream-typst.md) | ☑ |

**P4-06 — scoped, not implemented**, which is what the task asked for as a precondition. Full analysis in
[design/browser.md](../design/browser.md). The short version: everything carries over except the
synchronous `World::file` callback, and the fix is `SharedArrayBuffer` + `Atomics.wait` in a worker —
which needs cross-origin isolation headers that **vscode.dev may or may not serve**, and that an extension
cannot set. One experiment answers it (`crossOriginIsolated` in a web-extension worker); until then,
starting would be building on an unknown. The recommendation is also that it be a **separate extension**
sharing the crates, since a second 26 MB artifact would double the VSIX for every desktop user.

**P4-07 notes.** HTML is a **separate compilation target**, not a rendering of the paged document, and it
is still experimental upstream behind `Feature::Html`. The session's own library deliberately does *not*
enable it — that would make `html.*` available in ordinary documents and diverge from what
`typst compile` accepts — so the export runs against a world that is the session's in every respect
except its library.

**P4-14 notes.** Template packages are ordinary packages with a `[template]` section, so the download and
cache machinery was already there; this is the UX on top. The manifest reader is a focused parser rather
than a TOML dependency (three string keys), tested including the case where a later `[tool.*]` section
must not leak keys into it. Scaffolding **refuses a non-empty directory** — a scaffolder that writes into
one eventually overwrites someone's work.

**P4-15 — dropped for now, correctly.** [0007](../decisions/0007-textmate-grammar.md)'s revisit conditions
are all about *usage*: complaints about the pre-semantic-token flash, people running with
`semanticTokens: "disable"`, or maintenance turning out to cost more than adopting tinymist's generator.
None can be assessed before shipping, and none are met. Re-open when there are users.

## Explicitly not planned

DAP debugging · coverage · profiling flamegraphs · `typst test` · symbol picker · font browser ·
`tinymist.lock`-style project resolution · LaTeX/Word import · editing from the preview.

These are [proposal.md §2](../proposal.md#2-goals-and-non-goals) non-goals. Listing them here so that
"why doesn't it do X?" has an answer in the place people will look.
