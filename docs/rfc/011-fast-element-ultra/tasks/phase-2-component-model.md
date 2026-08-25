# Phase 2 — Component model

**Goal**: know what elements exist, what members they have, and where they are reachable from.
**Exit criterion**: all corpus elements discovered with their `@observable` members, from
`@customElement({ name: CONST })` — the case fast-analyzer misses entirely.
**Status**: ☑ 12 / 12 — exit test green (`test/corpus.test.ts`, `test/discovery.test.ts`)

Specified by [design/component-model.md](../design/component-model.md).

| # | Task | Status |
| --- | --- | --- |
| P2-01 | Tagged-template discovery by **resolved symbol**, honouring `htmlTemplateTags`/`cssTemplateTags`; nested templates become their own virtual documents | ☑ — `when(…)` fragments inherit the enclosing `TSource`; `repeat` item templates deliberately do not (only an explicit type argument can state the item type) |
| P2-02 | Registration forms A + B, name resolved **through the checker** | ☑ — string literal, same-file `const`, imported `const`, and the unresolvable case registering with `tagName: null`. The corpus test was written first and started red, as the note asked |
| P2-03 | Registration form C: `MyEl.define(…)` and `FASTElement.define(Type, …)` | ☑ |
| P2-04 | Members from decorators: `@attr` (bare/`attribute`/`mode`/`converter`), `@observable`, `@volatile` — on properties **and accessors** | ☑ — default attribute name confirmed from fast-element source: property name **lowercased** |
| P2-05 | `attributes: (AttributeConfiguration \| string)[]` in the definition | ☑ |
| P2-06 | Inheritance: walk to `FASTElement`, collect members, resolve shadowing; mixin members where the checker gives them | ☑ — explicit class chains (capped at 16), shadowing resolved, `origin: 'inherited'`. Mixins whose base the checker cannot hand back as a class declaration contribute nothing — narrower than the designed half-fact, recorded in component-model.md |
| P2-07 | Events: `this.$emit("name", detail)` with the `detail` type; `@fires` JSDoc | ☑ — then widened, because the class-body walk found nothing in the ordinary case: events are raised from the *template* (`x.$emit`, `c.parent.$emit` inside a `repeat` item), so the file is indexed by the **type of the receiver** instead. `@fires {Type}` and a `declare $events` map are the two ways to state what the scan cannot see ([component-model.md §3.2](../design/component-model.md#32-events)) |
| P2-08 | JSDoc facts: `@slot`, `@csspart`, `@cssprop`, `@attr`, `@prop` | ☑ — with the `-` default-slot marker disambiguated from `--css-custom-property` names, a collision the design missed |
| P2-09 | Template source types per virtual document; component ↔ template linked both ways | ☑ — cross-file (`element.ts` → `template.ts`), which required `getShorthandAssignmentValueSymbol` for `{ template }` shorthand and an engine-side `component_for_document` fallback |
| P2-10 | `fast-html-data`: build-time generator into committed `phf` tables; custom-data loader; globals | ☑ — three namespace maps (HTML/SVG/MathML; `title` exists in two) + events; SVG/MathML curated in `generator/svg-data.json` because VS Code ships no SVG data and the corpus is full of inline SVG |
| P2-11 | The registry: per-file contributions, merge order, `origin` provenance, duplicate detection, file-granular invalidation | ☑ |
| P2-12 | Dependency store: import graph, depth limits, reachability | ☑ — `maxProjectImportDepth` enforced in the BFS; node-module deps are merged into the same graph, so `maxNodeModuleImportDepth` is accepted but not separately enforced (recorded; the corpus never exercises the distinction) |

## Exit test

`extensions/fast-element-ultra/test/corpus.test.ts` — green:

```
csv-grid       ← extensions/csv-ultra/webview/viewer/element.ts     + 21 @observable members
pdf-viewer     ← extensions/pdf-ultra/webview/viewer/element.ts     + its @observable members
typst-preview  ← extensions/typst-ultra/webview/viewer/element.ts   + shadowOptions: null recorded
```

(The count is **3** elements — the RFC's "5" was a grep artifact; see the correction in
[research/corpus.md](../research/corpus.md).)

`test/discovery.test.ts` exercises every row of component-model.md §5's table, including the rows
fast-analyzer marks ❌ — which was the point of the phase.
