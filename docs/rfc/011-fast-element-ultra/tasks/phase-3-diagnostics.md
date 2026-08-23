# Phase 3 — Diagnostics

**Goal**: all 26 rules reporting, with severities, `strict`, and the type oracle.
**Exit criterion**: zero diagnostics over the corpus, and a seeded mistake of each rule's kind
reported at the right span with the right message.
**Status**: ☑ 13 / 14, ◐ 1 — both exit suites green (`test/corpus.test.ts` silence in strict mode;
`test/diagnostics.test.ts` seeded fixtures)

| # | Task | Status |
| --- | --- | --- |
| P3-01 | Rule engine: visitor, registration, severity resolution, `strict`, suppression, `dontShowSuggestions` | ☑ — severities exist once, in Rust; the plugin fetches the resolved table. Suppression (`@ts-ignore`/`@fast-ignore` on the previous line) is a plugin-side filter |
| P3-02 | Structural tag rules | ☑ — `no-unknown-tag-name`/`no-unclosed-tag` in the engine pass; `no-invalid-tag-name`/`no-duplicate-tag-name` at the registry level (spans point at registrations); `no-missing-element-type-definition` at discovery (needs `resolveName`) |
| P3-03 | Binding-name rules, `no-unknown-event` default → `warn` | ☑ — tested with strict *off* to pin the changed default |
| P3-04 | Binding-shape rules | ☑ |
| P3-05 | `no-missing-import` over the dependency graph, with the add-import quick fix | ☑ — the fix is a side-effect import synthesized by the plugin from the engine's `addImport` command |
| P3-06 | **`no-non-reactive-binding`** | ☑ — and running it over the corpus **narrowed it**: it fires on identifier/property-access reads only, so `${shortcut('a','b')}`-style one-time computed values are exempt by shape. Constness and callability travel with the placeholder, so the rule decides in Rust with no round trip. Default resolved: warn / strict error (open question 4 closed) |
| P3-07 | Directive rules: position by resolved symbol; `ref`/`slotted`/`children` targets against `TSource` with nearest-member fix | ☑ — `slotted` additionally requires a `<slot>` element and a shadow root |
| P3-08 | **Binding-type extraction**: the `${x => …}` unwrapping | ☑ — the enumeration became the rule: for value bindings, a type with call signatures unwraps to its **first signature's return type** (covers arrows, function expressions, and calls returning functions — the corpus's `keys(…)`); for `@event` bindings nothing unwraps, the raw type must be callable (or have `handleEvent`); `any`/`unknown` suppress rather than guess. Pinned by the oracle fixtures |
| P3-09 | The type oracle: batched facts, answered in TypeScript, merged back. **Measure the batch** | ☑ — **measured**: 32 facts/13 docs (csv), 60/11 (pdf); one crossing per document each way, no callbacks. [research/measurements.md](../research/measurements.md). Substance change from the design: the checker itself, not ts-simple-type ([0010](../decisions/0010-checker-not-ts-simple-type.md)) |
| P3-10 | Type rules with FAST's attribute-removal semantics in the helper | ☑ — `getNonNullableType` before every attribute comparison; `attr="${(x) => x.a ? 'yes' : null}"` is tested clean |
| P3-11 | `no-incompatible-attr-config` | ◐ — `mode: "boolean"` vs non-boolean and object-typed `@attr` without a converter are implemented and tested; the converter's own `fromView` return-type check is **not** (a converter's presence currently silences the rule). The remaining half needs signature resolution on the converter object |
| P3-12 | `no-attr-visibility-mismatch` and `no-untyped-template` with the type-argument fix | ☑ — the fix inserts `<ClassName>` at the tag's end, class name recovered through the registry (cross-file) |
| P3-13 | Shadow-DOM rules | ☑ — `<slot>` and `slotted('…')` in the engine (cross-file component resolution via the registry), `::part` in the css handler |
| P3-14 | `no-implicit-prevent-default`, default off/off | ☑ — fires when an opted-in key/input event handler's return type cannot be `true` |

## Notes

**The corpus silenced two rules the design would have shipped noisy.** `no-non-reactive-binding`'s
narrowing (P3-06) and the attr-config apparent-type bug (a `number` property's apparent type is the
`Number` interface, which read as "object-like" until the check used the type's own flags) were both
found by the zero-diagnostics gate, which is the gate doing exactly what
[research/corpus.md §4](../research/corpus.md#4-how-the-corpus-is-used) said it was for.

**Engine-decidable special case**: `@event="literal"` is `no-noncallable-event-binding` without the
oracle — a string is never callable.
