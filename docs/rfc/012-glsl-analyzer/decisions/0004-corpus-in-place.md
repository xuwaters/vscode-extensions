# 0004 — The glslang Test corpus is read in place, never copied

**Status:** Accepted (at RFC acceptance)

## Decision

Corpus tests (lexer, preprocessor, parser gauntlets; the false-positive gate) read
`temp/glslang/Test/` directly and **skip with a visible message** when the directory is
absent. No corpus file is ever copied into this repository.

## Why

- ~2,000 real shaders across every version, stage and deliberate breakage — the best
  free gauntlet there is, and copying it would both bloat the repo and entangle us with
  glslang's licensing for content we only need to *read*.
- RFC 011 proved the pattern (its `temp/fast-analyzer` gates): tests that skip loudly
  keep CI honest without making an external checkout a build dependency.

## Consequences

- A one-line helper per test crate resolves the corpus root from the repo root and
  produces the skip.
- Files under `Test/` that are *supposed* to fail (glslang keeps `.out` expectations) do
  not fail our gates — our gates assert **no panics** and, for the curated
  false-positive list, **no error-severity diagnostics on files we vetted as valid**.
  We never diff against glslang's own `.out` files.
