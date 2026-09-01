# Phase 2 — Lexer & preprocessor

**Goal:** `glsl-syntax`'s token layer and the real preprocessor, per
[decision 0003](../decisions/0003-preprocessor-provenance.md) and the contract in
[design/architecture.md](../design/architecture.md#glsl-syntax-wasm).

**Exit criterion:** every `Test/` corpus shader lexes and preprocesses with zero panics;
the expansion/conditional/provenance fixtures are green.

**Crates touched:** `crates/wgsl-shader/glsl-syntax` only. (Skeleton and workspace wiring
already exist — do not edit the root `Cargo.toml`.)

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P2-01 | Token model + lexer per GLSL 4.60 §3: identifiers, all numeric literal forms (hex/octal/float suffixes `u lf LF`), operators, line continuation `\`, comments as trivia. Every byte owned by exactly one token/trivia; lexing never fails. | exhaustive lexer unit tests incl. pathological literals | ☑ | `lexer.rs`: lossless token stream (whitespace/newlines/splices are tokens too), splice handled at character level so `FO\`+NL+`O` is one `FOO`; 26 tests in `tests/lexer.rs`. |
| P2-02 | Directive recognition & record: `#version` (number + profile), `#extension` (name + behaviour), `#pragma`, `#line`, `#error`; malformed directives become diagnostics, not panics. | directive fixtures | ☑ | `Directives` records version/extensions/pragmas/`#line`/`#include`; `#error` becomes a diagnostic and stays quiet in dead branches; `#line N` numbers the *next* line (glslang `preprocessor.line.vert` is the oracle); 24 tests. |
| P2-03 | Macro table: `#define` object- and function-like (parameters, no varargs in GLSL), `#undef`, redefinition rules, predefined `__LINE__ __FILE__ __VERSION__ GL_ES`. | macro-table unit tests | ☑ | `MacroTable` keeps retired definitions with their `#undef` span for go-to-def; §3.3 identical-redefinition rule incl. whitespace separation; reserved names split error (`GL_`, `defined`, the dynamic three) from warning (`__`); 22 tests. |
| P2-04 | Conditionals: `#if/#ifdef/#ifndef/#elif/#else/#endif` with the §3.3 constant-expression evaluator (`defined`, integer ops); inactive regions recorded with spans, nested correctly. | conditional fixtures incl. nesting + `#elif` chains | ☑ | 32-bit wrapping evaluator with glslang's operator table and short-circuiting (no `?:`, none in glslang either); dead branches are never evaluated, so a dead `#error`/`1/0` stays quiet; one `InactiveRegion` per dead branch, nested dead branches folded into the outer one; 32 tests. |
| P2-05 | Expansion with provenance: argument substitution, re-scan, self-reference guard; body tokens map to invocation span, argument tokens keep their own. | provenance assertions on every expansion fixture | ☑ | Prosser hide-set expansion (work list, not a call stack) so a name from one expansion can take its `(` from the source; body/pasted/stringified tokens carry the invocation span, argument tokens their own; `##`/`#` implemented; step budget + depth cap make runaways diagnose instead of hang; 33 tests, all asserting spans via `attributed()`. |
| P2-06 | The `Preprocessed` output type: expanded stream + directive record + macro table (with definition spans, for go-to-def later) + inactive regions + diagnostics. | API round-trip tests | ☑ | `Preprocessed { tokens, directives, macros, inactive, diagnostics }` plus `version()/profile()/is_es()/is_inactive()`; `PreprocessOptions.predefines` is the host `-D` hook; codes are `GLSL0001`–`GLSL0025`, parser takes `GLSL0100+`; 9 tests. |
| P2-07 | Corpus gate: lex + preprocess everything under `temp/glslang/Test/` with shader-like extensions; zero panics; count snapshot recorded in the Notes column. Skips when `temp/` absent ([0004](../decisions/0004-corpus-in-place.md)). | `corpus_preprocess` test | ☑ | **1677 files / 3.2 MiB lexed + preprocessed, 0 panics**, 2.1 s. Each file is isolated in `catch_unwind` so a break names every offender; also asserts the token stream partitions the source and every span (token/region/diagnostic) stays in bounds. Skips loudly when the checkout is absent; `GLSL_CORPUS` overrides the root, which is how the skip path is exercised. |
