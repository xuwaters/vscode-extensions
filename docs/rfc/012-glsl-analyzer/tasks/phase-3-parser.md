# Phase 3 — Parser & CST

**Goal:** the lossless, recovering GLSL parser in `glsl-syntax`.
**Needs:** Phase 2.

**Exit criterion:** every corpus shader parses with zero panics; the CST round-trips the
source byte-for-byte; the CST-derived outline covers everything the old heuristic walk
found on the extension's `examples/`. — **met**, see P3-07 and P3-08.

**Crates touched:** `crates/wgsl-shader/glsl-syntax`.

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P3-01 | CST shape decision (q2): green/red vs. flat event/span arrays, node-kind inventory for the 4.60 grammar, trivia and inactive-region attachment. Write [design/cst.md](../design/cst.md) and [decisions/0006](../decisions/0006-flat-cst-arrays.md). | `tests::cst`, 13 | ☑ | Flat preorder arena + one contiguous child array; leaves are `TokenId`s into `Preprocessed::tokens`, so the tree owns no tokens. Losslessness is `pieces()`, a coverage function over the written leaves, not stored trivia. 52 node kinds |
| P3-02 | Declarations: full qualifier sequences, `layout(…)` contents, `precision`, struct declarations, interface blocks (named/anonymous instance, arrays of blocks), arrays (sized/unsized, C and GLSL array syntax), initialisers. | `tests::declarations`, 16 | ☑ | Also `subroutine(…)`, `[[…]]` attributes, and the type-less `invariant a;` / `layout(…) in;` forms |
| P3-03 | Functions: prototypes vs. definitions, parameter lists with qualifiers/arrays, `subroutine` tolerance. | `tests::functions`, 9 | ☑ | Prototype and definition are one `FunctionDecl`; the `CompoundStmt` child is the difference |
| P3-04 | Statements & expressions: full precedence table (spec §5.1), selection/iteration/jump, `switch`, comma, ternary, call vs. constructor vs. array-index disambiguation. | `tests::expressions`, 17 | ☑ | Precedence asserted as one-line S-expressions, so an inverted table is visible. Call/constructor/array-constructor are deliberately one kind — Phase 4 tells them apart |
| P3-05 | Recovery: sync at `;`/`}`, unclosed groups at EOF, half-typed member lines — the "keeps working while the source does not parse" contract. Property: parsing never panics on any prefix of a valid file. | `tests::recovery`, 13 | ☑ | Prefix **and** suffix fuzz over 7 seed shaders (~2,600 parses). Two recoveries: gentle after a missing `;` (never eats the next declaration), whole-line for a line that cannot be read. Depth-capped at 64; a 2,000-deep nest reports `GLSL0110` instead of aborting. Every `GLSL01xx` code has a seed |
| P3-06 | Dialect variances: ES (`precision` statements everywhere, `attribute/varying` at 100), legacy desktop, `#version`-independent tolerance (parse everything, let analysis judge). | `tests::dialects`, 12 | ☑ | Includes the "same body under four `#version` lines gives the same tree" property, and the ray-tracing/mesh/`nonuniformEXT` storage qualifiers the corpus uses |
| P3-07 | Corpus gate: parse all of `Test/`; zero panics; byte-for-byte round-trip; error-count snapshot in Notes. | `tests::corpus_parse`, 1 | ☑ | **1,677 files, 3,217 KiB, 0 panics.** Round-trip and "every token is a leaf exactly once" asserted per file. Snapshot: 1,203 files with zero parse errors — **1,187 of the 1,293 that are GLSL** rather than glslang's HLSL front-end tests (91.8 %) — 3,566 parse errors total. The remainder is extension syntax core GLSL has no grammar for (`coopmat<T,…>`, `vector<T,N>`, C-style casts, `spirv_type`) plus glslang's deliberately broken files |
| P3-08 | Outline extraction from the CST (symbols, scopes, references — the shapes `wgsl-lsp-core` consumes) with parity tests against the old walk on `examples/*.{vert,frag,comp}`. The old walk is **not** deleted here (that is P5-01). | `tests::outline`, 17 | ☑ | `outline::{Outline, Symbol, SymbolKind, Reference}` in `glsl-syntax`; nothing in `wgsl-syntax` touched. Parity on all three examples for kind, name, `name_span`, `detail` and scope — expectations transcribed from the old walk's actual output, never imported. References now include macro invocation sites (at the name, not the replacement) and `#define` names, which the expanded stream alone cannot give |

## What landed

`crates/wgsl-shader/glsl-syntax` gained, all hand-formatted and clippy-clean:

| File | What |
| --- | --- |
| `src/cst.rs` | `SyntaxTree`, `Node`, `NodeKind` (52), `Child`, `NodeId`/`TokenId`, traversal, `pieces`/`reconstruct`, `dump`, and the event→arena build |
| `src/parser/mod.rs` | the `Parser`: cursor, event list with deferred `Open`s, both recoveries, the depth cap, the qualifier table |
| `src/parser/decl.rs` | declarations, qualifiers, `layout`, structs, blocks, arrays, `looks_like_declaration` |
| `src/parser/stmt.rs` | statements, conditions, the `for` header |
| `src/parser/expr.rs` | the §5.1 precedence ladder by climbing |
| `src/outline.rs` | symbols, scopes, references — the P5 hand-off shapes |
| `src/diagnostics.rs` | `ParseCode` / `SyntaxDiagnostic`, `GLSL0100`–`GLSL0110` |

**244 tests green** (`cargo test -p glsl-syntax`): the 146 Phase-2 tests untouched, 98 new.
`cargo clippy -p glsl-syntax --all-targets` is silent.

## Notes for Phase 4 and 5

- The tree holds token *indices*, not tokens: `glsl-analysis` needs the `Preprocessed`
  alongside it, exactly as [architecture.md](../design/architecture.md) already says.
- `NodeKind::CallExpr` covers calls, constructors and array constructors alike, and
  `IndexExpr` covers `a[i]` whether `a` is an array or a type. Both were left ambiguous
  on purpose; resolving them is Phase 4's job with a symbol table in hand.
- Semantic diagnostics start at `GLSL0200`. `GLSL0100`–`GLSL0199` is the parser's.
- The outline's `SymbolKind` mirrors `wgsl_syntax::tree::SymbolKind` name for name, minus
  `TypeAlias`, so the P5-01 adapter is a mapping and not a translation.
