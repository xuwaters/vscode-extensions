# Phase 2 — Lexer & preprocessor

**Goal:** `glsl-syntax`'s token layer and the real preprocessor, per
[decision 0003](../decisions/0003-preprocessor-provenance.md) and the contract in
[design/architecture.md](../design/architecture.md#glsl-syntax-wasm).

**Exit criterion:** every `Test/` corpus shader lexes and preprocesses with zero panics;
the expansion/conditional/provenance fixtures are green.

**Crates touched:** `crates/glsl/glsl-syntax` only. (Skeleton and workspace wiring
already exist — do not edit the root `Cargo.toml`.)

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P2-01 | Token model + lexer per GLSL 4.60 §3: identifiers, all numeric literal forms (hex/octal/float suffixes `u lf LF`), operators, line continuation `\`, comments as trivia. Every byte owned by exactly one token/trivia; lexing never fails. | exhaustive lexer unit tests incl. pathological literals | ☐ | |
| P2-02 | Directive recognition & record: `#version` (number + profile), `#extension` (name + behaviour), `#pragma`, `#line`, `#error`; malformed directives become diagnostics, not panics. | directive fixtures | ☐ | |
| P2-03 | Macro table: `#define` object- and function-like (parameters, no varargs in GLSL), `#undef`, redefinition rules, predefined `__LINE__ __FILE__ __VERSION__ GL_ES`. | macro-table unit tests | ☐ | |
| P2-04 | Conditionals: `#if/#ifdef/#ifndef/#elif/#else/#endif` with the §3.3 constant-expression evaluator (`defined`, integer ops); inactive regions recorded with spans, nested correctly. | conditional fixtures incl. nesting + `#elif` chains | ☐ | |
| P2-05 | Expansion with provenance: argument substitution, re-scan, self-reference guard; body tokens map to invocation span, argument tokens keep their own. | provenance assertions on every expansion fixture | ☐ | |
| P2-06 | The `Preprocessed` output type: expanded stream + directive record + macro table (with definition spans, for go-to-def later) + inactive regions + diagnostics. | API round-trip tests | ☐ | |
| P2-07 | Corpus gate: lex + preprocess everything under `temp/glslang/Test/` with shader-like extensions; zero panics; count snapshot recorded in the Notes column. Skips when `temp/` absent ([0004](../decisions/0004-corpus-in-place.md)). | `corpus_preprocess` test | ☐ | |
