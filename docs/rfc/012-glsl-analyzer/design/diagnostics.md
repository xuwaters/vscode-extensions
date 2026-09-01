# Design: the semantic diagnostics catalogue

Owner: Phase 4 (P4-09). Normative for what `glsl-analysis` reports, with which
code, at which severity. The parser's and preprocessor's ranges are
[design/cst.md §6](cst.md#6-diagnostics); this document owns `GLSL0200` onward.

## 1. The rule behind every entry

**A false error in an editor is worse than a missed one.** The analyzer is read
by someone mid-keystroke, not by a build server, and the cost of the two
mistakes is not symmetric: a missed error costs a bug found later, a false one
costs trust in every other squiggle on the screen. Every rule below is written
to fail towards silence.

Four mechanisms enforce that, and they are worth naming because they are what
the rules *are*:

| Mechanism | What it silences |
| --- | --- |
| **`Type::Unknown` is contagious and quiet.** Every rule returns early on an operand it could not type. | Extension types, unmodelled builtins, anything downstream of a name we could not place |
| **Untrustworthy input reports nothing.** A file whose parse or preprocessing produced an error, or which `#include`s a file we never followed, gets resolution and types and **no error-severity diagnostic at all**. | Half-typed files, files whose other half is missing |
| **An `#extension` line switches the name and availability rules off.** RFC 012 §2 N3 models no extension, so a file that enables one may legally use names and overloads no table here carries. | Every `GL_ARB_*`/`GL_EXT_*`/`GL_NV_*` shader in the corpus |
| **Names the language reserves are never unknown.** A `gl_` prefix, a `__` prefix, or a vendor suffix (`EXT`, `NV`, `ARB`, `KHR`, …) means "not this analyzer's business". | The ray-tracing, mesh, subgroup and cooperative-matrix vocabularies |

Two more are narrower and were each forced by a specific false positive the
corpus produced:

- **A profile the tables say nothing about is not a profile the name is absent
  from.** docs.gl has one set of pages per profile; where a page is missing the
  mask comes back empty. `gl_SampleMask` has no ES page and is in ES 3.20 all
  the same, so an availability error is only raised against a profile the entry
  has *some* version in.
- **A stage error needs a builtin that belongs to exactly one stage**, and that
  stage must be fragment or compute. `gl_ClipDistance`'s page names four stages
  and forgets that 4.30 made it readable in the fragment shader.

## 2. The catalogue

Severity is **error** except where the table says otherwise. Every code has a
fixture in `tests::catalogue` that produces *exactly* it, and a corrected twin
that produces nothing.

| Code | Meaning | Fires when |
| --- | --- | --- |
| `GLSL0200` | a name that is declared nowhere in scope | the name is not a symbol, not a builtin, not a type, not `gl_`/vendor-shaped, and the file enables no extension |
| `GLSL0201` | a call to something that is not a function | same, for a callee — or a callee that resolves to a variable |
| `GLSL0202` | a type specifier that names no type | same, for a type position |
| `GLSL0203` | a name declared twice in one scope | two declarations in one frame. Never for functions (that is overloading), for interface-block names (`in Primitive {…}` and `out Primitive {…}` is one interface), or for reserved-looking names |
| `GLSL0204` | a member the struct or block does not have | the owner's type is a known struct and the name is not one of its fields |
| `GLSL0205` | a swizzle the vector cannot answer | an unknown letter, mixed component sets, more than four, or a component past the vector's size |
| `GLSL0206` | `.member` on something with no members | a scalar, matrix, array or opaque base. `.length` is never this — it is the method |
| `GLSL0207` | `[]` on something that cannot be indexed | a scalar, struct or opaque base |
| `GLSL0208` | a constant index outside the bounds | the index folds to a constant *and* the bound is known. An unsized array says nothing |
| `GLSL0209` | a constructor these arguments cannot build | §5.4: too few components, an argument that contributes none, a struct with the wrong arity, an array with the wrong length |
| `GLSL0210` | no overload accepts these arguments | after retrying against *every* overload, not just this version's. Silent when an argument is opaque and some overload had the right arity — docs.gl's sampler pages are incomplete — and silent when the file enables an extension |
| `GLSL0211` | several overloads accept these arguments equally | two user declarations tied at a *non-zero* cost with different return types. A tie at zero is the same call written twice |
| `GLSL0212` | the wrong number of arguments | one declaration in scope, wrong arity |
| `GLSL0213` | an argument the parameter cannot accept | one declaration in scope, right arity, a parameter that does not take it |
| `GLSL0214` | a write to something that cannot be assigned | assignment, `++`/`--`, or an `out` argument, applied to a value — a literal, a call's result, a repeated swizzle |
| `GLSL0215` | a write to a `const`, `uniform` or shader input | the same, where the target *is* storage but read-only. `varying` never counts (it is an input or an output depending on the stage); a parameter never counts (it is a copy) |
| `GLSL0216` | an operator applied to types it has no meaning for | `&&` on a non-`bool`, `%` on a float, `!` on a number, arithmetic on a `bool`, a comparison on a vector |
| `GLSL0217` | two operands whose types do not agree | shapes that do not combine, a `?:` whose branches share no type, an assignment or initialiser whose source does not convert |
| `GLSL0218` | a condition that is not a `bool` | `if`, `while`, `for`, `?:`. Not `switch`, which selects on an integer |
| `GLSL0219` | a `return` that does not match the function | a value that does not convert, a missing value, a value in a `void` function |
| `GLSL0220` | `discard` outside a fragment shader | **only when the host named the stage.** A guessed stage never reports |
| `GLSL0221` | `break` or `continue` with nothing to leave | outside every loop, and outside every `switch` for `break` |
| `GLSL0222` | a `const` without a constant initialiser | no initialiser at all, or one that is *certainly* not constant — it reads a variable or calls a declared function. An initialiser this crate merely cannot fold is left alone |
| `GLSL0223` | a name this `#version` does not have | the declared version is one we recognise, the file enables no extension, the entry covers this profile, and the compatibility profile does not keep it. Carries a hint naming the other spelling where there is one |
| `GLSL0224` | a builtin this shader stage does not have | **only when the host named the stage**, and only for a builtin that belongs to exactly one stage, fragment or compute |
| `GLSL0225` | an array size that is not a positive constant | folds to zero or less, or reads storage |
| `GLSL0226` | code that can never run — **warning** | a statement after a `return`/`break`/`continue`/`discard` in the same block. Once per block |
| `GLSL0227` | a non-`void` function that can end without a value — **warning** | the body contains no `return` *anywhere*. Deliberately the weakest form of the check: a flow analysis would report the file being typed |

`GLSL0226` and `GLSL0227` are warnings for the same reason `GLSL0105` and
`GLSL0109` are in the parser: they describe a file mid-edit, and an editor
should not paint it red between two keystrokes.

## 3. Hints

Where a diagnostic knows the answer, it says it. `GLSL0223` carries the
replacement spelling in both directions — `texture` in a 1.10 shader suggests
`texture2D`, `texture2D` in a 3.30 core shader suggests `texture`, and
`gl_FragColor` suggests declaring an `out` variable — because "does not exist in
GLSL 1.10" without the "use `texture2D`" is a diagnostic that tells a user they
are stuck rather than what to type.

## 4. What is deliberately not reported

Recorded so nobody adds them later thinking they were missed:

- **Precision qualifiers.** ES requires a default precision for `float` in a
  fragment shader; the rule is real but the diagnostic would fire on every
  fragment of a shader being assembled from `#include`s we do not follow.
- **Initialiser-list shapes.** 4.20's braced initialisers have subtle rules and
  the payoff is a diagnostic nobody asks for.
- **Layout qualifier validity** (`location` ranges, `binding` collisions,
  `local_size` limits). Explicitly deferred by RFC 012 §10.
- **Interface matching between stages.** Cross-file, and there is no second file.
- **Constant folding beyond integers.** §2 N1: not a compiler.
- **`switch` exhaustiveness, fall-through, duplicate cases.** Style, not errors.

## 5. The gates

| Gate | What it holds | Where |
| --- | --- | --- |
| Fixture per code | every `GLSL02xx` is produced by a source, and its corrected twin is silent | `tests::catalogue` |
| Severity | only `GLSL0226`/`GLSL0227` are warnings | `tests::catalogue` |
| Spans | every diagnostic lands on real source bytes, on a character boundary, inside the file — a macro's error lands on the invocation | `tests::catalogue`, `tests::corpus` |
| No panic | the whole glslang corpus under `catch_unwind` | `tests::corpus::corpus_analyze` |
| No false positives | a curated list of valid corpus shaders reports zero errors | `tests::corpus::corpus_no_false_errors` |
