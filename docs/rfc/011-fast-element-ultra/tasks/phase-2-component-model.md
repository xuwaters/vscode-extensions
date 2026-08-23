# Phase 2 — Component model

**Goal**: know what elements exist, what members they have, and where they are reachable from.
**Exit criterion**: all 5 corpus elements discovered with their `@observable` members, from
`@customElement({ name: CONST })` — the case fast-analyzer misses entirely.
**Status**: ☐ 0 / 12
**Gated by**: [P1-05](phase-1-foundation.md)

Specified by [design/component-model.md](../design/component-model.md).

| # | Task | Status |
| --- | --- | --- |
| P2-01 | Tagged-template discovery: find `html`/`css` templates by **resolved symbol**, not by tag spelling, honouring `htmlTemplateTags`/`cssTemplateTags`. Nested templates become their own virtual documents. [0009](../decisions/0009-no-lit-compatibility.md) | ☐ |
| P2-02 | Registration form A + B: `@customElement("tag")` and `@customElement({ name })`, with the name resolved **through the checker** so a `const`, an imported `const` and an `as const` member access all work. [component-model.md §2.1](../design/component-model.md#21-resolving-the-name) | ☐ |
| P2-03 | Registration form C: `MyEl.define(…)` and `FASTElement.define(Type, …)`, all three overloads | ☐ |
| P2-04 | Members from decorators: `@attr` (bare and with `attribute`/`mode`/`converter`), `@observable`, `@volatile` — on property declarations **and get/set accessors** | ☐ |
| P2-05 | Members from the definition: the `attributes: (AttributeConfiguration \| string)[]` option, which needs no decorator | ☐ |
| P2-06 | Inheritance: walk `checker.getBaseTypes()` to `FASTElement`, collect members, resolve shadowing. Mixin members from the type where no declaration exists, marked as such. [component-model.md §3.1](../design/component-model.md#31-inheritance) | ☐ |
| P2-07 | Events: `this.$emit("name", detail)` — name from arg 0, `detail` type from arg 1. Also `@fires` JSDoc | ☐ |
| P2-08 | JSDoc facts: `@slot`, `@csspart`, `@cssprop`, `@attr`, `@prop`. `@slot` is the only source of slot names, so `no-unknown-slot` depends entirely on this task | ☐ |
| P2-09 | Template source types: read `TSource`/`TParent` from the tag's type arguments per virtual document; link a component to the template named in its definition, both ways. [component-model.md §6](../design/component-model.md#6-template-source-types) | ☐ |
| P2-10 | `fast-html-data`: build-time generator from `vscode-html-languageservice` and `@vscode/web-custom-data` JSON into a committed `phf` table; runtime loader for user custom-data files; `globalTags`/`globalAttributes`/`globalEvents` | ☐ |
| P2-11 | The registry: per-file contributions, the five-level merge order, `origin` provenance, duplicate-tag detection, file-granular invalidation. [component-model.md §4](../design/component-model.md#4-what-rust-does-with-the-facts) | ☐ |
| P2-12 | Dependency store: the import graph, `maxProjectImportDepth` and `maxNodeModuleImportDepth`, answering "is this component reachable from this module?" — the input to `no-missing-import` | ☐ |

## Exit test

`extensions/fast-element-ultra/test/corpus.test.ts`, run in CI
([research/corpus.md §4](../research/corpus.md#4-how-the-corpus-is-used)):

```
csv-grid       ← extensions/csv-ultra/webview/viewer/element.ts:112     + its @observable members
pdf-viewer     ← extensions/pdf-ultra/webview/viewer/element.ts:82      + its @observable members
typst-preview  ← extensions/typst-ultra/webview/viewer/element.ts:72    + shadowOptions: null recorded
```

plus a fixture project exercising every row of
[component-model.md §5](../design/component-model.md#5-what-fast-analyzer-covers-today) — including
the rows fast-analyzer marks ❌, which is the point of the phase.

## Notes

**P2-02 is the two-line fix that motivated the RFC.** It should be one of the first things that
works, and the corpus test should be written before it so it starts red.

**P2-06 is the largest task here** and the one most likely to surface checker subtleties. Mixins
resolve to synthesised types with no declaration; the design accepts half a fact
([component-model.md §3.1](../design/component-model.md#31-inheritance)) rather than dropping the
member, and the `origin` field is how a diagnostic knows the difference.

**P2-10's generated file is committed**, so a clean build does not need the npm packages present.
Same arrangement as typst-ultra's embedded-language grammar.
