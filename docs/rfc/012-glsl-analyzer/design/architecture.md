# Design: crate architecture and data flow

Owner: architect (this document is normative for crate boundaries). The diagram of
record is [proposal.md §3](../proposal.md#3-architecture).

## Crate contracts

### `glsl-spec` (wasm)
Pure data + lookup. No I/O, no allocation at query time beyond what formatting needs.
Public surface: `functions()`, `variables()`, `keywords()`, `types()` lookups plus the
model types. Depends on nothing but core (and `analyzer-core` only if span types are
genuinely needed — prefer not).

### `glsl-syntax` (wasm)
`source: &str` → `Preprocessed` → `SyntaxTree`.

- `tokenize` never fails; every byte belongs to exactly one token or trivia.
- `preprocess(tokens, predefines) -> Preprocessed`: expanded token stream with
  provenance ([decision 0003](../decisions/0003-preprocessor-provenance.md)), the
  directive record (`version`, `extensions`, `pragmas`), macro table with definition
  spans, inactive regions, and preprocessor diagnostics.
- `parse(preprocessed) -> SyntaxTree`: lossless (every source byte reachable), full
  GLSL 4.60 grammar + ES/legacy variances, recovery at `;`/`}` boundaries, parser
  diagnostics. Tree shape settled by P3-01 (q2).
- No dependency on `glsl-spec` — syntax does not know what `mix` is.

### `glsl-analysis` (wasm)
`(&SyntaxTree, &Preprocessed, &glsl-spec) -> Analysis`.

- Symbol tables per scope; resolved reference for every identifier occurrence (or a
  recorded unresolved).
- Types for every expression node that has one; call sites carry the chosen overload.
- The diagnostics catalogue: each diagnostic has a stable code, a severity, a span, and
  a message written for the editor, not the compiler.
- Stage and version context: from `#version`/file extension, threaded through builtin
  availability and rule selection.

### `glsl-spec-gen` (native only, never a wasm dependency)
See [spec-pipeline.md](spec-pipeline.md).

## Integration into `wgsl-lsp-core`

`state.rs`'s `Document` currently holds `wgsl_syntax::Parsed` + the naga `Analysis` for
both languages. After Phase 5 it holds, for `Language::Glsl`, the `glsl-syntax` tree and
`glsl-analysis` result instead; features branch on language at the *data* level (a small
adapter producing the shapes features already consume — symbols, references, scopes —
plus richer GLSL-only answers where the feature can use them: typed hovers, real
overloads in signature help, availability-filtered completion).

Rules of engagement:

- WGSL paths are not edited except where a shared type forces a mechanical touch.
- The old heuristic GLSL walk and `analysis/dialect.rs` are deleted only in their own
  tasks (P5-01/P5-02), after parity gates, never as a side effect.
- The wasm crate (`wgsl-lsp-wasm`) should need nothing but new Cargo deps.

## Performance posture

Whole-file reparse per edit (matching the current server's model) with the budgets in
[proposal.md §8](../proposal.md#8-budgets). No incremental parsing in this RFC; the CST
and analysis are rebuilt per keystroke and must simply be fast enough. If P5-10's
measurements break the budget, the recorded fallback order is: memoise preprocessing
when no `#` appears in the edit range; then cheapen analysis, never features.

**Outcome:** the budget was broken and neither fallback was needed. A 1,000-line
shader rebuilds in 3.4 ms native and 4.9 ms in wasm, against 5 ms and 25 ms; the
gap closed on linear table scans, per-character lexing and per-candidate work in
overload resolution, none of which changed an answer. Nothing was memoised and no
feature was cheapened, so the "rebuild everything per keystroke" model above still
holds exactly as written. See
[research/measurements.md §5](../research/measurements.md#5-closing-the-native-miss).
