# 0004 — Drop `web-component-analyzer` and write a FAST component model

**Status**: Accepted · **Date**: 2026-08-22

## Context

fast-analyzer discovers components in two passes and merges them:

```ts
// default-lit-analyzer-context.ts
const htmlCollection = analyzeSourceFile(sourceFile, { … });      // web-component-analyzer
const fastCollection = analyzeFastElementComponents(sourceFile, ts, checker);  // 250 lines
// …then a hand-written merge of properties, attributes and events, name by name
```

`web-component-analyzer` is 2.9 MB and was written to discover LitElement, `customElements.define`,
and JSDoc-annotated plain custom elements. It does not recognise FAST. The FAST pass exists because
of that, and the merge exists because both passes might have found the same tag.

## Decision

One component model, written for FAST, in the plugin. `web-component-analyzer` is not a dependency.

## Consequences

**One source of truth.** No merge step, so no question about which pass wins when they disagree —
which is a real question today, since the merge only adds members whose names are not already
present, meaning the WCA version of a member silently wins on type.

**A model that covers FAST.** WCA's absence is not a loss, because
[component-model.md §5](../design/component-model.md#5-what-fast-analyzer-covers-today) shows the
combined system covers about a third of FAST's declaration forms. What WCA *did* provide and we now
owe ourselves:

| From WCA | Our replacement |
| --- | --- |
| JSDoc `@slot` / `@fires` / `@csspart` / `@cssprop` / `@attr` / `@prop` | Read directly. It is a JSDoc tag walk, not a framework |
| Inheritance-chain member collection | `checker.getBaseTypes()` walk — and WCA's version never applied to FAST classes anyway |
| `customElements.define("x", Class)` | Not FAST. `FASTElement.define` is, and WCA does not know it |
| Analysis of `HTMLElement` subclasses in `node_modules` | Genuinely lost. Third-party components that are not FAST no longer light up |

**That last row is the real cost.** A project mixing FAST with a non-FAST web-component library gets
less from us than from fast-analyzer today. The intended answer is
`custom-elements.json` ingestion — the standard interchange format for exactly this — which is listed
in [proposal.md §10](../proposal.md#10-deliberately-deferred) rather than Phase 1. Until then, the
mitigations are `customHtmlData` and `globalTags`, both of which fast-analyzer also offers and both
of which are worse than reading a manifest.

**~2.9 MB and a transitive dependency tree leave the VSIX.**

## Alternatives

**Keep WCA and add a FAST flavour to it.** WCA has a flavour API, and this is the architecturally
tidy answer. Rejected: it means a pull request against an upstream we do not control (last publish
2.0.0), for a framework its author does not use, to get a model we would then still have to merge
with. The 250-line bolt-on exists because this route was already not taken.

**Keep WCA only for `node_modules` scanning.** Attractive — it is the one thing we lose — but it
means shipping 2.9 MB and running a second analyzer over every external file to serve a case
`custom-elements.json` serves better. Revisit if the mixed-library case turns out to be common.

## Revisit if

- Users report the mixed-framework case often enough that `customHtmlData` is not an answer. The fix
  is CEM ingestion, not WCA.
- WCA gains real FAST support upstream.
