# Measurements — RFC 012 §8 budgets

Owner: P5-10, with the release sweep in [§8](#8-the-release-sweep-p6-04) owned by
P6-04. Every number here is reproducible by a test in the repo; the test is named
beside it. Where a budget is missed, the number stays as measured and the gap is
named — a budget that moves to meet the result is not a budget.

| Machine | Apple silicon macOS 26.6.2, `rustc 1.100.0-nightly (e457a7b0d)`, `--release`, Node 24.17 |
| --- | --- |
| Date | 2026-09-01 |

## 1. Summary

| Budget (§8) | Target | Measured | |
| --- | --- | --- | --- |
| `wgsl_lsp_wasm_bg.wasm` growth from this RFC | ≤ +900 KB | **+46 KB** (+2.6 %) | ☑ 19× under |
| Reparse + reanalyse a 1,000-line shader, wasm | ≤ 25 ms | **4.9 ms** | ☑ 5.1× under |
| Reparse + reanalyse a 1,000-line shader, native | ≤ 5 ms | **3.4 ms** | ☑ 1.5× under |
| Generated spec source | ≤ 1.5 MB | 424 KB (P1-09) | ☑ |

All four are met. The native figure was **6.9 ms** when the features were
finished and this file first recorded the miss; §5 is what closed it.

Sections 2–7 are Phase 5's measurements, taken as that phase finished.
[§8](#8-the-release-sweep-p6-04) re-runs all of them against the tree that was
packaged as 0.6.0, and adds the corpus pass rates, the spec-pipeline figures and
a before/after on the three dialect examples. Nothing moved outside the noise.

## 2. Wasm size

Test: rebuild with `pnpm --filter wx-vsce-wgsl-shader build:wasm` and compare.
Both builds are `-Os` through `wasm-opt`, as `Cargo.toml`'s
`package.metadata.wasm-pack` configures.

| Build | Bytes | gzip | Δ from baseline |
| --- | --- | --- | --- |
| Baseline (commit 64fc05c: heuristic GLSL walk + naga `glsl-in`) | 1,787,337 | 674,225 | — |
| With the `glsl-*` crates, naga `glsl-in` still enabled | 1,831,496 | — | +44,159 |
| Features finished, naga `glsl-in` dropped | 1,813,371 | 674,265 | +26,034 |
| Shipped: the same, with §5's performance work | **1,834,673** | 680,646 | **+47,336** |

Four things are worth reading off that table.

- **The three new crates cost ~44 KB**, not the ~230 KB their static data
  suggests. `glsl-spec`'s tables are one contiguous documentation pool plus
  index slices (decision 0006's sibling reasoning in the spec model), which is
  exactly the shape LTO and `wasm-opt` handle best; and 407 lines of
  hand-written GLSL builtins plus a 464-line token walk left `wgsl-syntax` at
  the same time.
- **Dropping naga's `glsl-in` saved 18 KB, not the front end's full weight.**
  Once `wgsl-lsp-core` stopped *calling* `naga::front::glsl`, LTO dropped most
  of it whether or not the feature was on; the feature flag removes the
  residue. Decision 0008 was never a size argument and the measurement confirms
  it did not need to be.
- **The performance work cost 21 KB uncompressed and 6 KB gzipped** — the
  compile-time index arrays of §5.1, and the extra code the byte-at-a-time
  loops of §5.2 turn into. Halving the latency for 1.2 % of the binary, against
  a budget with 850 KB spare, is not a trade worth agonising over.
- **gzip is still nearly flat**: +6,421 bytes on 674 KB. What ships in a VSIX is
  the compressed size, and by that measure this RFC is close to free.

## 3. Latency, native

Test: `crates/wgsl-shader/wgsl-lsp-core/tests/budgets.rs`. It measures and
prints in any build and *asserts* only in an optimised one — a debug build is
five to ten times slower and would measure the optimiser rather than the
analyzer, so `cargo test --release` is what enforces the budget. Best of
twenty runs on a 1,007-line fixture that exercises every
layer — two macros (one function-like), an inactive `#ifdef` branch, an
interface block, a struct, ~197 function definitions and ~800 statements. Dense
on purpose: a real 1,000-line shader has more comment and blank lines, so this
is the conservative end.

| | before §5 | now | |
| --- | --- | --- | --- |
| `GlslDocument::build` — lex, preprocess, parse, analyse, outline, project | 6.94 | **3.35** | 2.1× |
| The same through the server: `didChange` + rebuild | 7.55 | **3.48** | 2.2× |
| §8 budget | 5.00 | 5.00 | |

Per layer, same fixture, best of twenty:

| Layer | before | now | |
| --- | --- | --- | --- |
| `glsl_syntax::tokenize` | 1.21 | **0.16** | lossless — every byte becomes a token or trivia |
| `glsl_syntax::preprocess` | 0.88 | **0.75** | expansion, conditionals, provenance |
| `glsl_syntax::parse` | 1.14 | **0.63** | the flat CST |
| `glsl_analysis::analyze` | 2.33 | **1.14** | scopes, types, overload resolution |
| `glsl_syntax::outline` | 0.44 | **0.37** | symbols, scopes, references |
| `wgsl_lsp_core::glsl::adapter` | 0.87 | **0.20** | the projection the feature layer reads |
| | 6.87 | **3.25** | |

Run-to-run spread on a quiet machine is ±0.05 ms on the layers and ±0.15 ms on
the whole build; six consecutive runs of the whole build gave 3.35 – 3.49 ms.
The test asserts **5 ms**, the budget itself, rather than the regression
ceiling it asserted while the budget was missed.

## 4. Latency, wasm

Test: `extensions/wgsl-shader/src/wasmBudget.test.ts`, best of thirty runs on
the same fixture, through the real `ShaderServer` binding — `didChange` plus a
`wgsl/validate` that forces the analysis, which is what an editor pays on a
keystroke with `validate.onType` on.

| | before §5 | now |
| --- | --- | --- |
| Measured | 9.55 | **4.92** |
| §8 budget | 25.00 | 25.00 |

The test skips when `wasm/` is absent, the same way the corpus tests skip when
`temp/` is, so it costs a contributor without the wasm toolchain nothing.

Wasm is **1.5× native** here, which is a better ratio than this kind of
workload usually gets and is worth recording: the analyzer allocates in a few
large arenas rather than in a scatter of small objects, and that is what the
wasm allocator is good at. The ratio held through §5 — every change below is a
change in how much work is done, not in how well one machine does it.

## 5. Closing the native miss

The first version of this file recorded 6.9 ms against 5 ms and said the six
layers summed to it with none dominating. That was true of the *layers* and
false of the *causes*: a handful of problems were spread across all six, and a
sampling profiler found them in an afternoon.

Method: `sample(1)` against a release build looping the full pipeline. The two
largest leaves were `memcmp` at 17 % of the run and `malloc`/`free` at 18 % —
which is what a pipeline looks like when it is comparing strings it should be
indexing and allocating objects it should be reusing. Everything below was
found that way, and re-measured after; nothing here was changed on a hunch.

One thing the workspace does makes all of this sharper: `[profile.release]` is
`opt-level = "s"`, because the same profile builds the wasm. Nothing gets
unrolled, small functions stay out of line, and a linear scan over a table
costs what it says it costs. Structural fixes are the only lever, which is the
right constraint to be under.

### 5.1 The spec tables were scanned, not indexed

`glsl_spec::basic_type` and `glsl_spec::keyword` were `iter().find()` over 150
and 90 entries. Between the semantic-token classifier and
`glsl_analysis::Type::from_name`, **every identifier in a file** is asked of
both — about 240 string comparisons per identifier, and `memcmp` was the
largest single leaf in the profile.

The tables stay grouped by arrival, because that is the only order they can be
checked against the spec in. Beside them are now `KEYWORD_ORDER` and
`TYPE_ORDER`, alphabetical index arrays built by a `const` insertion sort, plus
a 257-entry first-byte table so that the common answer — no, `helper1` is not a
type — costs one array read. The same treatment went to the parser's
60-spelling `QUALIFIERS`, which `looks_like_declaration` asks about every
statement in a file. Both orders are guarded by a test that they are *strictly*
increasing, which also proves neither table repeats a name.

### 5.2 The lexer worked one logical character at a time

Two things, both worth about the same. Punctuators were matched against a table
of 47 spellings longest-first, so a `;` cost up to 47 `memcmp`s; dispatch is now
a match on the first byte, with only the eleven families that have a longer
member looking further, and how far to advance comes from `Punct::as_str` so the
scanner and the spelling cannot drift apart. And every character went through
`peek`/`bump`, which resolve line continuations — so the letters of an
identifier were being checked for a `\` twice each, through two out-of-line
calls. `Lexer::take_while` now consumes a run of identifier, digit or space
bytes in one loop and drops out only at a `\`, which is the only byte that can
mean something other than itself.

**1.21 → 0.16 ms.** The lexer is no longer a layer worth naming.

### 5.3 The CST builder allocated a `Vec` per node

`cst::build` staged each open node's children in a `Vec<Child>` of its own —
one allocation and one free per node, several thousand per file. An open node's
children are always the *tail* of the whole pending list, because nothing can be
added to an ancestor while a descendant is open, so one buffer does it: closing
a node takes everything from its start index and truncates. The outline had the
same shape in a milder form — seven `.collect::<Vec<_>>()` calls on child lists
that the walk could have iterated directly; they were there to satisfy a borrow
that never existed.

### 5.4 Overload resolution re-derived the world for every candidate

The largest single find. `score_overload` binds a generic family by trying its
members in turn, and each try called `Type::from_name(member)` — a table lookup
plus a spelling decomposition. `clamp` has a dozen overloads of three
parameters, so **one `clamp(…)` was deriving about a hundred types from
strings**, and the fixture calls it two hundred times. The family member types
are now derived once per process into a `OnceLock`; the tables are static, so
the answer cannot change.

Two smaller things in the same path: a candidate's family bindings were keyed by
a freshly allocated `String` per binding and are now keyed by `FamilyId`, and
the binding buffers are reused across candidates rather than allocated per
candidate. And `Analyzer::operator` was matching an operator token's *text*
against 35 spellings when the lexer had already decided which `Punct` it was.

**2.33 → 1.14 ms.**

### 5.5 The preprocessor hashed every identifier against the macro table

The expander asks `MacroTable::lookup` about every identifier in the file, and a
shader defines a handful of macros — so almost every question was a SipHash of a
string against a table that could not have held it. `MacroTable` now keeps a
count of live macros per first byte, and answers "no" from one array read. The
analysis's scope frames, which are asked the same kind of question, swapped
SipHash for FxHash: the keys are the identifiers of the file being edited, they
live for one analysis of one document, and nothing about them is reachable from
a network, so the hash-flooding resistance was buying nothing.

### 5.6 What the recorded fallback order turned out to be worth

[design/architecture.md](../design/architecture.md#performance-posture) records
the order to try: memoise preprocessing, then cheapen analysis, never features.
In the event **neither was needed and nothing was cheapened**. Every change
above leaves the answers byte-identical: the 597 tests that existed before the
work pass unmodified, the corpus gates are unchanged (1,677 files, 0 panics,
byte-for-byte round-trip, 208-file false-positive gate at zero errors), and the
two tests added are guards on the new invariants — that the compile-time orders
are sorted, and that every entry is still reachable by name.

Preprocessing memoisation would have needed `Document` to keep the preprocessed
stream and know which bytes an edit touched. It buys at most 0.75 ms, it is the
first piece of incremental state in a pipeline that deliberately has none, and
the budget is met without it. It is not worth the invariant.

## 6. What is still there

One thing from the original plan was not done, and it is still the largest
single allocation source in the pipeline: **every `PpToken` owns a `String`**.
The expanded stream allocates one per code token — about 5,500 for this fixture
— because expansion can synthesise tokens (`##`, `#`, `__LINE__`) that exist in
no source slice. In the profile that is ~11 % of a full rebuild once the
allocation, the copy and the eventual free are added up, so it is worth roughly
0.35 ms.

It stays for now because the two shapes that would remove it both cost more than
0.35 ms is worth against 1.6 ms of headroom:

- **A `Cow<'a, str>` borrowing the source** is what the note in the first
  version of this file suggested, and it cannot work: `GlslDocument` owns both
  the source and the `Preprocessed`, so the borrow would be self-referential.
- **A span into an arena the `Preprocessed` owns** does work, and it is the
  right end state. It makes `PpToken::text` need the `Preprocessed` in hand,
  which means `glsl_syntax::parse` and `glsl_analysis::analyze` take the source
  as well — a public API change across three crates for one layer's worth of a
  budget that is met. An inline small-string would keep the API and needs
  `from_utf8_unchecked` to read back, and this repo has one `unsafe` block in
  it, in a wasm `Send` shim, which is where that count should stay.

If the budget is ever tightened, or an editor is seen to stutter on a much
larger file, this is the next thing to do and the arena is the shape to do it
in.

## 7. Test counts at the end of Phase 5

| Crate | Tests |
| --- | --- |
| `glsl-spec` | 28 (+ 1 doctest) |
| `glsl-syntax` | 245 (+ 1 doctest) |
| `glsl-analysis` | 117 (+ 1 doctest) |
| `wgsl-syntax` | 36 (+ 1 doctest) |
| `wgsl-lsp-core` | 171 — 64 unit, 56 `features`, 48 `glsl_dialects`, 3 `budgets` |
| `extensions/wgsl-shader` (vitest) | 69 |

`wgsl-lsp-core` had 129 before this phase (73 unit, 56 `features`) and
`wgsl-syntax` 51; the eleven that left `wgsl-syntax` are the ones that tested
the heuristic GLSL walk, which no longer exists. The two added by the
performance work are `glsl-spec`'s
`the_keyword_and_type_orders_are_strictly_sorted` and `glsl-syntax`'s
`the_qualifier_order_is_strictly_sorted_and_finds_every_word`.

## 8. The release sweep (P6-04)

Everything above was measured as its phase finished. This section is the same
machine and the same day, run once more against the tree that was packaged as
`wx-vsce-wgsl-shader-0.6.0.vsix`, so that the numbers the RFC closes on are the
numbers that shipped rather than the numbers that were true when each phase
ended.

Commands, in the order they were run before packaging:

```sh
cargo run -p glsl-spec-gen -- --check
cargo test --release -p glsl-spec -p glsl-spec-gen -p glsl-syntax \
                     -p glsl-analysis -p wgsl-syntax -p wgsl-lsp-core
pnpm --filter wx-vsce-wgsl-shader typecheck
pnpm --filter wx-vsce-wgsl-shader build:wasm     # after rm -rf wasm/
pnpm --filter wx-vsce-wgsl-shader test
pnpm --filter wx-vsce-wgsl-shader package
```

### 8.1 The budgets, as shipped

| Budget (§8) | Target | Shipped | At Phase 5 | |
| --- | --- | --- | --- | --- |
| `wgsl_lsp_wasm_bg.wasm` growth | ≤ +900 KB | **+47,336 B** (1,834,673 total) | +47,336 | ☑ |
| The same, gzipped | — | +6,421 B (680,646 total) | +6,421 | |
| Reparse + reanalyse 1,000 lines, wasm | ≤ 25 ms | **4.86 ms** | 4.92 | ☑ |
| Reparse + reanalyse 1,000 lines, native | ≤ 5 ms | **3.43 ms** | 3.35 | ☑ |
| The same through the server | ≤ 5 ms | **3.46 ms** | 3.48 | ☑ |
| Generated spec source | ≤ 1.5 MB | **424,200 B** in 5 files | 424,200 | ☑ |

Per layer, best of twenty on the 1,007-line fixture: lex 0.16, preprocess 0.78,
parse 0.64, analyse 1.15, outline 0.37, project 0.20 — 3.30 ms summed. Every
figure is inside the ±0.05/±0.15 ms run-to-run spread §3 records; nothing moved.

The wasm is **byte-identical** to the Phase 5 build — 1,834,673 bytes from a
`rm -rf wasm/` rebuild, same gzip. Phase 6 changed one doc comment in
`wgsl-lsp-core` and nothing else in any crate that ships, and the build says so.

### 8.2 Corpus pass rates and diagnostic counts

`temp/glslang/Test`, read in place: 1,677 files, 3,217 KiB. All four gates are in
the run above; each prints the line quoted here.

| Gate | Crate | Result |
| --- | --- | --- |
| `corpus_preprocess` | `glsl-syntax` | 1,677 files, **0 panics**; the token stream tiles every byte of every file |
| `corpus_parse` | `glsl-syntax` | 1,677 files, **0 panics**, byte-for-byte round-trip on all of them. 1,203 parse with no error — **1,187 of the 1,293 that are GLSL** rather than glslang's HLSL front-end tests, i.e. **91.8 %**. 3,566 parse errors in the remaining 106 |
| `corpus_analyze` | `glsl-analysis` | 1,677 files, **0 panics**. 1,555 (**92.7 %**) produce no semantic error; 1,042 errors across the other 122 |
| `corpus_no_false_errors` | `glsl-analysis` | 208 curated valid shaders, desktop 1.10–4.60 and ES 1.00–3.20, every stage: **0 errors** |
| `the_extensions_examples_analyse_cleanly` | `glsl-analysis` | 5 shipped example shaders: **0 errors** |
| `the_shipped_examples_produce_no_errors` | `wgsl-lsp-core` | the same five through the server: **0 errors** |

The two "errors total" figures are a number to watch, not a bar to clear: the
corpus is mostly deliberately broken shaders, and a file that *should* fail has
no right answer for how many diagnostics it deserves. What the gates assert is
the two things that do have a right answer — never panic, and never cry wolf on
a shader known to be valid.

### 8.3 The spec pipeline, regenerated

`cargo run -p glsl-spec-gen -- --check` exits 0 against the committed tables,
which is RFC §9.4 (`rm -rf` the generated files, regenerate, `git diff` is empty)
made into a command that cannot be forgotten.

| | |
| --- | --- |
| docs.gl commit | `e94408a383941cb09df228a3c4bad4e7b799b302` |
| Pages read | 314, with 2 redirect stubs skipped |
| Entries | 161 functions, 31 variables, 41 generic families |
| Overloads | 1,168 prototypes → **717** after merging the desktop and ES profiles |
| Version-table coverage | 1,057 of 1,168 overloads (90 %) matched a version row; the rest inherit their function's mask |
| Output | 424,200 bytes of Rust in 5 files |

Beside it, hand-written and not generated: `keywords.rs` (keywords, basic types,
precision defaults) and `legacy.rs` (39 functions, 58 variables of the
compatibility and ES 1.00 surface docs.gl does not document — decision 0007).

### 8.4 Before and after, on the three dialect examples

`examples/` ships one shader per dialect, and each is a gate. "Before" is commit
`64fc05c`, version 0.5.1: the heuristic token walk in `wgsl-syntax/src/parser/glsl.rs`
(464 lines) for the outline, `wgsl-syntax/src/builtins/glsl.rs` (407 lines, one
signature per function, no overloads, no version or stage gating) for hover and
completion, and naga's `glsl-in` for diagnostics — gated by `analysis/dialect.rs`,
which switched validation *off* for every ES version, for combined samplers and
for implicit block bindings.

| | `test-es300.frag` — ES 3.00 | `test-opengl.frag` — desktop 3.30 core | `test.frag` — Vulkan 4.50 |
| --- | --- | --- | --- |
| **Diagnostics, before** | none — `dialect.rs` skipped every ES version | none — skipped on the combined `sampler2D` and the unbound uniform block | naga's, with five of the seventeen error classes in [0008](../decisions/0008-naga-glsl-in-dropped.md) collapsing to `Function [1] 'main' is invalid` over the whole function |
| **Diagnostics, after** | preprocessor, parser and semantic, `GLSL0001`–`GLSL0227`, on the construct | the same | the same, and every naga finding reproduced with a code and a tight span |
| **Preprocessor, before** | none. `#define SATURATE(x)` was a name; both sides of the `#ifdef` were walked as live | none | none |
| **Preprocessor, after** | expanded, with provenance back to source bytes; the inactive `#ifdef` branch is outlined and dimmed but not analysed | — | — |
| **Hover on `texture`, before** | one hand-written signature, the same string in all three files | the same string | the same string |
| **Hover on `texture`, after** | the ES 3.00 overloads only — no `sampler1D` — with the reference page's prose | the desktop 3.30 set | the desktop 4.50 set, the largest of the three |
| **Completion, before** | one flat list, no version or stage filter: `gl_ClipDistance` offered to a WebGL shader | the same list | the same list |
| **Completion, after** | filtered by version *and* stage; `gl_FragCoord` yes, `gl_ClipDistance` no, `gl_ClipVertex` no | filtered to desktop 3.30, which has `gl_ClipDistance` and not the 1.10–1.50 compatibility names | filtered to 4.50 |
| **Signature help, before** | one signature | one signature | one signature |
| **Signature help, after** | the whole overload set in `genType`/`gsampler2D` notation, arity-matched | the same | the same |
| **Types** | an expression was never typed; now every expression has a type, and `material.tint` resolves through the block | `mat3(t, b, n) * sampled` is checked as a constructor and a product | `sampler2D(albedo, albedo_sampler)` is checked as a constructor |
| **`wgsl/shaderInfo`, before** | `skipped`, with `dialect.rs`'s reason: naga's front end accepts `#version 440`, `450` and `460 core` and this file declares `300 es` | `skipped` the same way on `330` | `ok`, and no version field existed to report |
| **now** | `ok`, `version: "3.00 es"`, `stage: fragment`, not guessed | `ok`, `version: "3.30"` | `ok`, `version: "4.50"` |

Two of the three files did not exist before this phase, which is itself the
point: there was nothing to *show* for ES or desktop OpenGL, because the answer
for both was "highlighted, not analysed". `test.frag` is byte-for-byte the file
0.5.1 shipped, but for a two-line comment rewritten to the same length so that
the P3-08 outline-parity spans stay valid.

### 8.5 Test counts at release

| Crate / suite | Tests | At end of Phase 5 |
| --- | --- | --- |
| `glsl-spec` | 28 (+1 doctest) | 28 |
| `glsl-spec-gen` | 23 | 23 (not previously totalled) |
| `glsl-syntax` | 245 (+1 doctest) | 245 |
| `glsl-analysis` | 117 (+1 doctest) | 117 |
| `wgsl-syntax` | 36 (+1 doctest) | 36 |
| `wgsl-lsp-core` | **172** — 64 unit, 56 `features`, **49** `glsl_dialects`, 3 `budgets` | 171 |
| **Rust total** | **621**, plus 4 doctests | 597 counted, 620 including `glsl-spec-gen` |
| `extensions/wgsl-shader` (vitest) | 69 | 69 |

The one test Phase 6 added is
`glsl_dialects::each_shipped_example_is_analysed_as_the_dialect_it_declares`:
the ES, OpenGL and Vulkan examples report `3.00 es`, `3.30` and `4.50`, a known
stage, and nothing skipped. `the_shipped_examples_produce_no_errors` grew from
three files to five in the same change.

### 8.6 The artifact

| | |
| --- | --- |
| VSIX | `extensions/wgsl-shader/wx-vsce-wgsl-shader-0.6.0.vsix` |
| Size | 833,062 bytes, 20 files (0.5.1 was 813,770) |
| Largest entries | `wasm/wgsl_lsp_wasm_bg.wasm` 1.75 MB, `dist/extension.js` 350 KB, `dist/server.js` 164 KB, `THIRD-PARTY-NOTICES.md` 90.8 KB |
| Built from | `rm -rf wasm/`, then the command list at the top of this section |

The +19 KB over 0.5.1 is almost entirely the third-party notices, which became a
generated per-crate enumeration in P6-03. The wasm did not move at all.
