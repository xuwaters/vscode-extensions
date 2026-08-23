# Phase 3 — Diagnostics

**Goal**: all 26 rules reporting, with severities, `strict`, and the type oracle.
**Exit criterion**: zero diagnostics over the corpus, and a seeded mistake of each rule's kind
reported at the right span with the right message.
**Status**: ☐ 0 / 14

Specified by [design/rules.md](../design/rules.md) and
[architecture.md §4.3](../design/architecture.md#43-rust--ts-binding-facts).

| # | Task | Status |
| --- | --- | --- |
| P3-01 | Rule engine: a visitor over the tree with a shared context, rule registration, severity resolution, `strict`, `// @ts-ignore` suppression, `dontShowSuggestions` | ☐ |
| P3-02 | Structural tag rules: `no-unknown-tag-name`, `no-unclosed-tag`, `no-invalid-tag-name`, `no-missing-element-type-definition`, `no-duplicate-tag-name` | ☐ |
| P3-03 | Binding-name rules: `no-unknown-attribute`, `no-unknown-property`, `no-unknown-event`, `no-unknown-slot`, `no-invalid-attribute-name`. Note `no-unknown-event`'s default moves to `warn` — [0007](../decisions/0007-fast-rule-semantics.md) | ☐ |
| P3-04 | Binding-shape rules: `no-expressionless-property-binding`, `no-unintended-mixed-binding` | ☐ |
| P3-05 | `no-missing-import`, over P2-12's dependency graph, with the add-import quick fix | ☐ |
| P3-06 | **`no-non-reactive-binding`** — the new rule that matters. Needs the constness test from the oracle so `${SOME_CONST}` does not fire. Run over the corpus and over a codebase with deliberate one-time bindings before choosing the default; closes [open question 4](../decisions/README.md#open-questions). [rules.md](../design/rules.md#no-non-reactive-binding) | ☐ |
| P3-07 | Directive rules: `no-invalid-directive-binding` (position, by resolved symbol) and `no-invalid-directive-target` (`ref`/`slotted`/`children` string against `TSource`, with a nearest-member quick fix) | ☐ |
| P3-08 | **Binding-type extraction**: the `${x => …}` unwrapping. Enumerate the cases before porting the fork's `getCallSignatures()[0]` heuristic — arrows, function expressions, calls returning functions (the corpus's `keys(…)`), overloads, generics, `this` parameters, and the aspects where unwrapping must *not* happen (`@event`). Regression tests from the fork's known-good cases. [research/corpus.md §3](../research/corpus.md#3-constructs-the-corpus-exercises-that-the-design-must-handle) | ☐ |
| P3-09 | The type oracle: binding facts out of Rust, batch answering in TypeScript with `ts-simple-type`, results merged back. **Measure the batch size and the per-fact cost** — [budget 3](../research/spikes.md#budget-3--type-oracle-round-trip) | ☐ |
| P3-10 | Type rules: `no-noncallable-event-binding`, `no-boolean-in-attribute-binding`, `no-complex-attribute-binding`, `no-incompatible-type-binding` — with FAST's attribute-removal semantics modelled in the helper rather than patched around it | ☐ |
| P3-11 | `no-incompatible-attr-config`: `@attr({ mode })` and `@attr({ converter })` against the declared type. Replaces lit's `no-incompatible-property-type` | ☐ |
| P3-12 | `no-attr-visibility-mismatch` and `no-untyped-template`, the latter with a quick fix that inserts the type argument inferred from the referencing component | ☐ |
| P3-13 | Shadow-DOM rules: `no-slot-without-shadow-root`, covering `<slot>`, `slot=`, `::part` in the component's CSS, and `slotted('…')` on a `shadowOptions: null` component | ☐ |
| P3-14 | `no-implicit-prevent-default`, default `off`/`off`. Grounded in the csv-ultra incident — [research/fast-element.md §10](../research/fast-element.md#10-event-handlers-and-preventdefault) | ☐ |

## Exit test

Two suites:

1. **Silence over the corpus.** The full rule set, `strict` on, over all 26 templates → zero
   diagnostics. A diagnostic here is either a real bug in one of our extensions (fix the extension,
   record it in [research/corpus.md](../research/corpus.md)) or a false positive (fix the rule).
2. **A seeded fixture per rule**, with the expected span and message, in
   `crates/fast/fast-analyzer-core/tests/`, replayed from recorded `upsertFile` + `analyze` payloads
   so the engine tests need no compiler ([crates.md](../design/crates.md#testing)).

## Notes

**P3-08 is the riskiest task in the phase.** The fork's heuristic works on the corpus partly by luck
— `keys(…)` returns a function, so a call signature is found on the result type. Copying it without
enumerating the cases would import a subtle bug we then cannot explain. Do the enumeration first,
then write the code against it.

**P3-06's default is a real decision, not a formality.** `error` matches how bad the bug is;
`warning` is what a rule should be when it has a legitimate-use exception it detects heuristically.
The evidence to decide is a run over code that uses one-time bindings on purpose, and the corpus does
not contain any.

**P3-09 is where [0002](../decisions/0002-rust-engine-typescript-oracle.md) is either confirmed or
not.** One crossing per file is a design claim; the measurement either supports it or sends more rule
logic into the plugin.
