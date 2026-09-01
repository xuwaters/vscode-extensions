# docs.gl survey (P1-01)

The inventory the generator is designed against, measured on the checkout below.
Everything here is a *measurement*, not a plan; where it contradicts
[design/spec-pipeline.md](../design/spec-pipeline.md) the amendment is spelled out in
[§9](#9-amendments-to-designspec-pipelinemd).

| | |
| --- | --- |
| Repository | github.com/BSVino/docs.gl → `temp/docs.gl` (never committed) |
| Commit surveyed | `e94408a383941cb09df228a3c4bad4e7b799b302` (2026-04-12) |
| Directories used | `sl4/` (desktop GLSL), `el3/` (GLSL ES) |
| Directories ignored | `gl2/ gl3/ gl4/ es1/ es2/ es3/` — the C API pages, not the shading language |

## 1. Inventory

| | `sl4/` | `el3/` |
| --- | --- | --- |
| `.xhtml` files | 174 | 142 |
| …of which redirect stubs | 1 (`dFdy.xhtml`) | 1 (`dFdy.xhtml`) |
| Real reference pages | 173 | 141 |
| Distinct function names declared | 161 | 137 |
| `funcprototype` blocks (overloads) | 699 | 469 |
| `gl_*` variable pages | 31 | 14 |

The `el3/` page names are a strict subset of `sl4/`'s — no page exists only in ES — so the
merge is a left join on `sl4/` names plus nothing. ES-only *overloads* do exist inside
shared pages, which is why both directories are parsed rather than ES availability being
inferred from the desktop page.

### Section presence, all 314 real pages

| Section | Pages |
| --- | --- |
| `refsect1#description` | 314 (all) |
| `refsect1#versions` | 314 (all) |
| `refsect1#Copyright` | 314 (all) |
| `refsect1#parameters` | 253 |
| `refsect1#seealso` | 309 |

The 61 pages without a `parameters` section are the 45 variable pages plus 16 function
pages whose functions take no arguments (`barrier`, `memoryBarrier*`, `EmitVertex`,
`EndPrimitive`, `groupMemoryBarrier`). Missing `parameters` is therefore normal and not
an error condition.

## 2. Well-formedness

Every one of the 314 pages parses as standalone XML with a stock parser: a single root
`<div class="refentry" id="…">`, all tags closed, `<col/>`/`<br/>` self-closed. Entity
usage across both directories is 23 `&lt;`, 9 `&gt;`, 2 `&amp;` — all XML predefined, so
no DTD or entity catalogue is needed. `roxmltree` can read the files as-is.

Two caveats the reader must handle:

- Pages are HTML *fragments*, not documents: no XML declaration, no `<html>`, leading
  indentation before the root element.
- Uncompiled template tokens `{$pipelinestall}{$examples}` appear as bare text between
  the description and versions sections on all 314 pages (they are substituted by
  docs.gl's `compile.py` at site-build time). They are text nodes, not markup, so they do
  not break parsing — but any code that concatenates the page's text must drop them.

`sl4/dFdy.xhtml` and `el3/dFdy.xhtml` are not pages at all:

```
<script>window.location.replace("dFdx");</script>
```

That is also not well-formed as XML in isolation (it is, but it carries no refentry), so
the generator detects the `<script>` prefix and skips it as a *known redirect*, not a
failure. `dFdy` is still generated — it is declared on `dFdx.xhtml` (see §4).

## 3. Prototypes

Prototypes live in `<table class="funcprototype-table">`. The first cell is
`<code class="funcdef">RET <strong class="fsfunc">name</strong>(</code>`; every later
cell is one parameter, `TYPE <var class="pdparam">name</var>`, with the last carrying the
closing `)` and `;`.

Across all 1,168 prototypes in both directories the flattened cell text matched
`^(ret) (name)\($` + `TYPE NAME` parameters with **zero** exceptions. The variance is
confined to four documented forms:

| Form | Meaning | Where |
| --- | --- | --- |
| `void EmitVertex(void)` | sole `void` parameter = empty parameter list | 16 no-arg functions |
| `out genIType exp` / `inout uint data` | parameter flow qualifier | 18 `out`, 32 `inout` |
| `[float bias]` | *optional* parameter, brackets included in the cell text | 67 prototypes on 7 pages: `texture`, `textureGather`, `textureGatherOffset`, `textureGatherOffsets`, `textureOffset`, `textureProj`, `textureProjOffset` |
| `gvec4`, `genType`, `gsampler2D`, `mat`, `vec` … | a generic *family* name, kept symbolic | everywhere |

No prototype uses an array suffix, a default value, a struct type or a vararg. The
`in` qualifier never appears explicitly (it is the default).

### 3.1 Type vocabulary

40 distinct return-type spellings and 87 distinct parameter-type spellings. Three of
those 87 are typos in docs.gl and must be normalised by the generator:

| Page(s) | As written | Correct |
| --- | --- | --- |
| `sl4/texelFetch.xhtml` | `sample sample` (2 prototypes) | `int sample` |
| `sl4/textureQueryLevels.xhtml`, `sl4/textureQueryLod.xhtml` | `gsampler2DDArray` | `gsampler2DArray` |
| `sl4/textureSize.xhtml` | `gsamplerRect`, `gsamplerRectShadow` | `gsampler2DRect`, `gsampler2DRectShadow` |

The last is not strictly a typo but an inconsistency: `texture.xhtml` spells the same
family `gsampler2DRect`. The generator normalises to the `2DRect` spelling so the family
table has one entry per real family. `gbufferImage` (11 `image*` pages) and `gimageRect`
are likewise normalised to `gimageBuffer` and `gimage2DRect`.

These five substitutions are the whole fixup table; it lives in the generator next to the
family table with this section as its citation.

### 3.2 Pages that declare more than one function

Eight `sl4/` pages and six `el3/` pages declare prototypes whose `fsfunc` name differs
from the page name. They are grouped, not aliased — each name becomes its own entry:

| Page | Functions declared |
| --- | --- |
| `dFdx` | `dFdx dFdy` (+ `dFdxCoarse dFdxFine dFdyCoarse dFdyFine` in `sl4/`) |
| `floatBitsToInt` | `floatBitsToInt floatBitsToUint` |
| `intBitsToFloat` | `intBitsToFloat uintBitsToFloat` |
| `packUnorm` | `packUnorm2x16 packSnorm2x16 packUnorm4x8 packSnorm4x8` |
| `unpackUnorm` | `unpackUnorm2x16 unpackSnorm2x16 unpackUnorm4x8 unpackSnorm4x8` |
| `umulExtended` | `umulExtended imulExtended` |
| `fwidth` (`sl4/` only) | `fwidth fwidthCoarse fwidthFine` |
| `noise` (`sl4/` only) | `noise1 noise2 noise3 noise4` |

So the generator keys entries by the **prototype's own name**, never by the file name;
the file name is only provenance. This is also how `dFdy` survives its redirect stub.

## 4. Variables

45 `gl_*` pages (31 desktop, 14 ES). The declaration lives in `refsynopsisdiv` in one of
two shapes:

**(a) `fieldsynopsis`** — 43 pages, `29 + 14`:

```html
<code class="fieldsynopsis"><span class="modifier">in </span><span class="type">vec4 </span><span class="varname">gl_FragCoord </span>;</code>
```

Five `sl4/` pages carry **two** `fieldsynopsis` blocks, one per stage, each preceded by a
`<pre class="programlisting">// In tessellation control shaders</pre>` comment naming the
stage: `gl_Layer`, `gl_PrimitiveID`, `gl_TessLevelInner`, `gl_TessLevelOuter`,
`gl_ViewportIndex`. The two blocks differ in their `modifier` (`in` vs `out`), which is
exactly the per-stage flow the data model wants — so the generator keeps one variable
entry with a stage mask and the union of flows, and does *not* emit two entries.

The `varname` span carries the array suffix inline: `gl_TessLevelOuter[4]`,
`gl_ClipDistance[]`, `gl_SampleMask[]`. The generator splits the suffix off the name and
folds it into the type (`float` + `[4]` → `float[4]`) so lookup by bare name works.

**(b) No `fieldsynopsis` at all** — `sl4/gl_Position.xhtml` and `sl4/gl_PointSize.xhtml`.
Both document a member of the `gl_PerVertex` interface block and give it as a
`<pre class="programlisting">` code listing:

```
out gl_PerVertex {
    vec4 gl_Position;
    float gl_PointSize;
    float gl_ClipDistance[];
};
```

This is the `gl_Position` quirk [design/spec-pipeline.md](../design/spec-pipeline.md)
warned about, and it is the *only* structural special case in the whole corpus. The
generator handles it by scanning the `programlisting` for a line declaring the page's own
variable name and reading the flow from the block's own `out`/`in` keyword. `el3/`'s
copies of both pages use the ordinary `fieldsynopsis` shape, so ES needs no special case.

### 4.1 Stage association

docs.gl does not tag a variable with its stage in machine-readable form. Two signals are
available and the generator uses both:

1. The versions-table row labels, which for multi-stage variables read
   `gl_Layer (geometry stage)`, `gl_PointSize (vertex shader)`,
   `gl_PrimitiveID (Tessellation Control and Evaluation Languages)`.
2. The description's opening clause, which is boilerplate-uniform:
   *"Available only in the fragment language, …"*, *"Available only in the tessellation
   control and evaluation languages, …"*.

Both are matched against a fixed stage-word table (`vertex`, `tessellation control`,
`tessellation evaluation`, `geometry`, `fragment`, `compute`). Where neither yields a
stage, the variable gets an **all-stages** mask rather than an empty one — a permissive
default, because an empty mask would make the analyzer reject a legal name.

## 5. Version tables

Uniform to a degree that makes this the easiest part of the pipeline. Every page's
versions table has a two-row `thead`; the second row's cells are the column labels, and
there is exactly **one** distinct label set per directory:

| Directory | Columns | Count |
| --- | --- | --- |
| `sl4/` | `1.10 1.20 1.30 1.40 1.50 3.30 4.00 4.10 4.20 4.30 4.40 4.50` | 173/173 pages |
| `el3/` | `1.00 3.00 3.10` | 141/141 pages |

The first header cell is `Function Name` (151 `sl4/`, 133 `el3/`) or `Variable Name`
(22 / 8). Body cells are exactly two glyphs: `✔` (2,437 total) and `-` (1,304 total).
Nothing else — no footnote markers, no blank cells, no `colspan` in the body.

**Two versions are missing from docs.gl entirely: desktop 4.60 and ES 3.20.** The
checkout predates neither, it simply never added the columns. Since a `#version 460` file
must not have every builtin reported as unavailable, the generator **extrapolates**: the
4.60 bit copies the 4.50 bit and the ES 3.20 bit copies the ES 3.10 bit. This is stated in
the generated header and is the single place the tables assert something docs.gl does not.
It is safe in the permissive direction — no builtin was removed between 4.50 and 4.60 or
between ES 3.10 and 3.20.

`el3/` has no ES 3.20 column *and* its 1.00 column is close to useless: ES 1.00's texture
builtins (`texture2D`, `textureCube`, …) have no `el3/` pages at all, so the ES 1.00 mask
is honest about what it covers and silent about the rest. Phase 4 must not treat "ES 1.00
bit clear" as "does not exist in ES 1.00" for names outside this corpus; the legacy ES /
compatibility-profile surface is q3's problem, not the generator's.

### 5.1 Rows do not map cleanly onto prototypes

This is the one place the design's assumption ("the versions table has one row per
prototype group") does not survive contact.

| Body rows | `sl4/` pages | `el3/` pages |
| --- | --- | --- |
| 1 | 108 | 122 |
| 2 | 42 | 8 |
| 3 | 16 | 7 |
| 4 | 7 | 4 |

The 65 `sl4/` and 19 `el3/` multi-row tables label their rows in **six mutually
inconsistent prose styles**:

| Style | Example labels |
| --- | --- |
| family in parens | `abs (genType)` / `abs (genIType)` / `abs (genDType)` |
| family, no space | `mix(genType)` / `mix(genDType)` / `mix(genIType), mix(genUType), mix(genBType)` |
| vector class | `lessThan (vec)` / `lessThan (ivec)` / `lessThan (uvec)` |
| scalar class | `determinant (float)` / `determinant (double)` |
| sibling function names | `dFdx` / `dFdy` / `dFdxCoarse, dFdxFine, dFdyCoarse, dFdyFine` |
| opaque-type set, sometimes with brace notation and the `gsamplerRect` spelling | `texture` / `texture (gsampler2DRect{Shadow})` / `texture (gsampler2DMS, gsampler2DMSArray)` / `texture (gsamplerCubeArray{Shadow})` |
| stage | `gl_Layer (geometry stage)` / `gl_Layer (fragment stage)` |

A label is therefore parsed as *(one or more function names) + (a set of qualifier
tokens)*, tokens being the comma-separated contents of the parentheses with `{…}` expanded
(`gsampler2DRect{Shadow}` → `gsampler2DRect`, `gsampler2DRectShadow`) and run through the
§3.1 fixup table. An overload is then matched to the row with the **most** qualifier
tokens that all appear among the overload's own type spellings, falling back to the
unqualified row for that name. This is a heuristic, so:

- **Function-level masks are the union** of every row naming that function. That mask is
  the authoritative one, and it is what §7's exit criterion is asserted against.
- **Per-overload masks** carry the matched row's mask, and when no row matches they carry
  the function-level union — i.e. the fallback is permissive and can never manufacture a
  spurious "not available in this version" diagnostic.

The generator prints its per-overload match rate at the end of a run and fails loudly if a
*function-level* mask comes out empty (a row that names no known function), because that
would mean a parse failure rather than a missing row.

## 6. Prose

Descriptions total 181 KB of text across both directories (median page 417 bytes, longest
`sl4/gl_Layer.xhtml` at 3.8 KB), which is comfortably inside the §8 budget even before the
per-entry cap. The markup inside `description` and `parameters` is a small closed set:

| Markup | Count | Markdown mapping |
| --- | --- | --- |
| `<em class="parameter"><code>x</code></em>` | 1,384 | `` `x` `` |
| `<code class="function">` | 406 | `` `…` `` |
| `<code class="varname">` | 221 | `` `…` `` |
| `<code class="constant">` / `<code class="code">` | 106 | `` `…` `` |
| `<span class="emphasis">` | 99 | `*…*` |
| `<a class="citerefentry">` → `<span class="refentrytitle">` | 59 | `` `…` `` (the link target is another docs.gl page, useless offline) |
| `<ul class="itemizedlist">` / `<li>` | 8 / 33 | `- …` |
| `<pre class="programlisting">` | 12 | fenced ```` ```glsl ```` block |
| MathML (`<math>` … `<mfrac>`, `<msup>`, `<mtable>`) | 161 `<math>` roots | dropped, replaced with `(see the reference page)` |
| `$x \times (1 - a) + y \times a$` TeX spans | 5, all `sl4/` | `` `x * (1 - a) + y * a` `` after entity-free literal transcription |
| `<div class="informaltable">` inside a description | 1 | dropped with the same ellipsis |

Paragraph text is heavily line-wrapped with leading indentation; whitespace must be
collapsed per paragraph. No page uses `<b>`, `<i>`, `<table>` inside `parameters`, or
nested lists.

## 7. Exclusion list

Everything the generator refuses to read, with the reason. This is the complete list; a
page not on it that fails to parse is a hard error and stops the run.

| Excluded | Reason |
| --- | --- |
| `gl2/ gl3/ gl4/ es1/ es2/ es3/` | OpenGL C API pages, not the shading language |
| `sl4/dFdy.xhtml`, `el3/dFdy.xhtml` | `<script>` redirect stubs; the functions are declared on `dFdx.xhtml` |
| everything in `sl4/`/`el3/` that is not `*.xhtml` | none exist today; the filter is defensive |

No page is excluded for bad content. All 314 real pages contribute.

## 8. What docs.gl does *not* cover

Recorded so later phases do not go looking:

- **Keywords, operators, precision qualifiers, layout qualifiers, storage qualifiers.**
  Nothing. Hand-written in `glsl-spec` per [decision 0002](../decisions/0002-docs-gl-as-spec-source.md) (task P1-06).
- **Constructors and the implicit-conversion table.** Not documented as builtins. Phase 4.
- **Builtin constants** (`gl_MaxVertexAttribs`, `gl_MaxDrawBuffers`, …). No pages. Phase 4
  if they are wanted at all.
- **The compatibility profile** — `gl_FragColor`, `gl_TexCoord[]`, `gl_ModelViewMatrix`,
  `attribute`, `varying`, `texture2D`, `textureCube`. Not one page. This is the evidence
  q3 was waiting on: **docs.gl documents none of the legacy surface**, so "whatever
  docs.gl documents and no more" would leave WebGL1-era shaders unanalysable. Phase 4 must
  decide from the corpus instead.
- **Extension-gated builtins** (`GL_ARB_*`, `GL_EXT_*`). Version columns only. Matches N3.
- **Desktop 4.60 and ES 3.20 columns** — see §5.

## 9. Amendments to design/spec-pipeline.md

The survey confirms the design except in three places. These are the amendments; the
generator implements this document where the two differ.

1. **§"Known hazards" — "the versions table has one row per prototype group" is wrong.**
   Row labels are prose in six styles and 108 of 173 `sl4/` pages have a single row for
   every prototype. Replaced by §5.1: authoritative masks are per *function* (row union),
   per-overload masks are best-effort with a permissive fallback.
2. **Version coverage.** The design assumed "GLSL 1.10 … 4.60" and "GLSL ES 1.00 … 3.20"
   are readable from the tables. 4.60 and ES 3.20 columns do not exist; they are
   extrapolated from 4.50 / ES 3.10 and the extrapolation is declared in the generated
   header (§5).
3. **The `gl_Position` quirk generalises.** It is `gl_Position` *and* `gl_PointSize`, and
   only in `sl4/`; five further pages carry two `fieldsynopsis` blocks that must merge into
   one entry rather than special-case (§4).

Nothing else in the design needed changing: the pages really are uniform, really are
well-formed XML, and the merge really is a name-keyed left join.

## 10. Licensing (feeds P1-10)

`temp/docs.gl/readme.md`: *"docs.gl is a public domain web scaffolding for the OpenGL
documentation."* The repository carries no `LICENSE` file.

Every one of the 314 pages ends with a `refsect1#Copyright` section, in three variants
that differ only in the year:

- 309 pages — "Copyright © 2011-2014 Khronos Group."
- 4 pages — "Copyright © 2014 Khronos Group."
- 1 page — "Copyright © 2012-2014 Khronos Group."

all continuing: *"This material may be distributed subject to the terms and conditions set
forth in the Open Publication License, v 1.0, 8 June 1999.
https://opencontent.org/openpub/"*.

So the prose we embed is Khronos material under **OPL v1.0**, and the scaffolding that
reshaped it is public domain. [Decision 0005](../decisions/0005-refpage-attribution.md)
settles the wording.
