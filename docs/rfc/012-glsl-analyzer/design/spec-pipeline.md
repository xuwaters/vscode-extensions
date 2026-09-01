# Design: the builtin spec pipeline

Owner phase: 1. Governing decision: [0002](../decisions/0002-docs-gl-as-spec-source.md).
This document specifies intent and interfaces; P1-01's survey
([research/docs-gl.md](../research/docs-gl.md)) may amend details — amendments are noted
here, not silently diverged from.

## Source layout

`temp/docs.gl` (clone of github.com/BSVino/docs.gl):

- `sl4/*.xhtml` — one page per desktop-GLSL builtin (functions, and `gl_*.xhtml` for
  builtin variables). Each page has: one or more `funcprototype` blocks (or
  `fieldsynopsis` for variables), a parameters section, a `description` section, and a
  `versions` table whose columns are GLSL versions.
- `el3/*.xhtml` — the same shape for GLSL ES.
- Other directories (`gl2/ gl3/ gl4/ es*/`) are the *API* (C) pages — ignored.

P1-01 must verify this inventory against the real checkout: page count per directory,
pages missing a description or versions table, prototype quirks (vararg-looking
constructs, `void` parameter lists, overload groups split across pages), and how ES
pages express versions. Everything the generator excludes gets listed with a reason.

## Data model (`glsl-spec`, hand-written)

```rust
pub struct BuiltinFunction {
    pub name: &'static str,
    pub overloads: &'static [Overload],
    pub doc: Doc,                     // description + per-parameter docs
    pub desktop: VersionMask,         // GLSL 1.10 … 4.60
    pub es: VersionMask,              // GLSL ES 1.00 … 3.20
}
pub struct Overload { pub ret: TypeRef, pub params: &'static [Param] }
pub struct Param { pub name: &'static str, pub ty: TypeRef, pub flow: Flow } // in/out/inout
pub enum TypeRef { Concrete(&'static str), Family(Family) } // genType, gvec, gsampler2D …
pub struct BuiltinVariable {
    pub name: &'static str, pub ty: &'static str,
    pub stages: StageMask, pub flow: Flow,     // in/out from the shader's viewpoint
    pub doc: Doc, pub desktop: VersionMask, pub es: VersionMask,
}
```

Exact shapes may evolve during P1-02; the invariants that may not:

- **Generic families are symbolic data**, not pre-expanded text, so `glsl-analysis` can
  expand them during overload resolution and hover can print the spec's own notation.
- **All prose lives in one contiguous string pool** with `(offset, len)` slices in the
  entries — one big `&'static str` compresses better in wasm and avoids per-entry
  pointer bloat.
- Lookup is by binary search over a sorted-by-name table (the tables are `static`, the
  generator sorts).

Hand-written alongside (not generated): keyword list with the version/profile each
arrived in, basic type names, precision defaults per stage — small closed sets from the
spec text.

## Generator (`glsl-spec-gen`, native bin)

- Rust, honest XHTML parsing (an XML crate such as `roxmltree` on the XHTML — these
  pages are well-formed; fall back per-page with a loud error if one is not).
- Pipeline per page: prototypes → structured overloads; parameters section → per-param
  doc; description → markdown (see below); versions table → mask.
- Merge: same builtin appearing in `sl4/` and `el3/` becomes one entry with both masks;
  ES-only builtins keep an empty desktop mask and vice versa.
- Markdown conversion: strip the boilerplate lead-ins, keep the first N meaningful
  paragraphs within a per-entry byte budget, convert the refpages' entity math and
  `<code>` spans, drop tables/images with a "see reference" ellipsis.
- Output: `crates/glsl/glsl-spec/src/generated/functions.rs`, `variables.rs`,
  `doc_pool.rs` (or one file — generator's choice, recorded here) with a header naming
  the tool, the docs.gl commit hash, and the attribution (q4).
- **Determinism**: stable ordering, no timestamps, no HashMap iteration order leaks.
  `cargo run -p glsl-spec-gen` twice → identical bytes. A test in `glsl-spec` re-runs
  the generator when `temp/docs.gl` exists and asserts the committed file matches.

## Known hazards (from studying gen_spec.py's behaviour, not its code)

- `gl_Position` needed special-casing upstream — its page's `fieldsynopsis` is atypical.
- Some pages document several functions (`textureSize` variants) in one file; the
  per-prototype loop must attach the right versions row (the versions table has one row
  per prototype group).
- Operators/keywords are *not* on docs.gl — that is why they are hand-written here,
  where glsl_analyzer scraped the spec HTML instead.
