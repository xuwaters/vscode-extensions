# What `temp/fast-analyzer` actually does

**Source of record**: [`temp/fast-analyzer`](../../../../temp/fast-analyzer) at `b3d66a1`, a fork of
[lit-analyzer](https://github.com/runem/lit-analyzer) whose last upstream commit is `a518bf7`.
Everything on this page was counted from that tree on 2026-08-22.

This is the definition of "parity" in [proposal.md §2](../proposal.md#2-goals-and-non-goals). If a
capability is listed here as ✅, RFC 011 has to provide it or explain in
[decisions/](../decisions/README.md) why not.

---

## 1. Shape of the fork

Three packages:

| Package | Role | Published as |
| --- | --- | --- |
| `lit-analyzer` | The analysis library and a CLI | `lit-analyzer` (unchanged name) |
| `ts-lit-plugin` | TypeScript server plugin that decorates the language service | `ts-fast-plugin` |
| `vscode-lit-plugin` | The VS Code extension: settings bridge, colour provider, grammars | `fast-plugin` |

### 1.1 Size

`packages/lit-analyzer/src/lib`, excluding tests:

| Area | Files | Lines |
| --- | ---: | ---: |
| `analyze/parse` | 22 | 2,037 |
| `analyze/store` | 10 | 972 |
| `analyze/document-analyzer` | 17 | 1,242 |
| `analyze/types` | 22 | 492 |
| `analyze/data` | 3 | 737 |
| `analyze/util` | 16 | 850 |
| `analyze/component-analyzer` | 1 | 41 |
| `analyze/flavors` | 1 | 250 |
| `analyze/*.ts` (top level) | — | 1,350 |
| `rules` | 32 | 2,775 |
| `cli` | 13 | 943 |
| **Total** | **~137** | **~11,689** |

Plus `ts-lit-plugin/src` (~1,500 lines across 19 files) and `vscode-lit-plugin/src` (~500 lines
across 2 non-test files).

### 1.2 How much of it is FAST

`git diff a518bf7..HEAD -- packages/` — the entire FAST contribution:

```
46 files changed, 1479 insertions(+), 502 deletions(-)
```

Of which the substantive analysis code is:

| File | Lines added | What it does |
| --- | ---: | --- |
| `analyze/flavors/fast-element-analyzer.ts` | 250 | The whole FAST component model |
| `ts-lit-plugin/ts-lit-plugin.ts` | 233 | `findReferences` for tag names, incl. cross-project |
| `analyze/lit-analyzer.ts` | 90 | Member-property rename locations |
| `analyze/default-lit-analyzer-context.ts` | 52 | Merging FAST tags into the WCA collection |
| `document-analyzer/html/rename-locations/rename-locations-for-member-property.ts` | 75 | New file |
| `rules/util/type/extract-binding-types.ts` | 40 | Unwrapping `${x => …}` return types |
| `rules/util/type/is-assignable-in-attribute-binding.ts` | 24 | `stripNullAndUndefined` |
| `rules/util/directive/is-lit-directive.ts` | 21 | `isFastElementDirective` |
| `analyze/constants.ts` | 10 | The `:` modifier |
| Everything else | ~684 | package metadata, README, icons, grammar tweak, tests |

### 1.3 Runtime dependencies inherited

| Dependency | Size | What we do with it |
| --- | --- | --- |
| `web-component-analyzer` ^2.0.0 | 2.9 MB | Component discovery. Written for LitElement; does not recognise FAST |
| `parse5` 5.1.0 | — | HTML fragment parsing with `sourceCodeLocationInfo` |
| `vscode-html-languageservice` 5.5.1 | — | Built-in HTML tag/attribute data, and HTML completion data |
| `vscode-css-languageservice` 6.3.7 | — | `` css` ` `` validation, completion, hover, folding, colour |
| `@vscode/web-custom-data` ^0.4.2 | — | Browser-compat HTML data |
| `ts-simple-type` ~2.0.0-next.0 | — | Assignability for the type rules |
| `didyoumean2` 4.1.0 | — | "Did you mean …?" suggestions |
| `fast-glob` ^3.2.11 | — | CLI file discovery |
| `chalk` ^2.4.2 | — | CLI colours |

---

## 2. IDE features

`LitAnalyzer`'s public API (`analyze/lit-analyzer.ts`) and what the plugin wires it to.

| Feature | Analyzer method | Decorated LS method | RFC 011 |
| --- | --- | --- | --- |
| Diagnostics | `getDiagnosticsInFile` | `getSemanticDiagnostics` | ✅ Phase 3 |
| Completion | `getCompletionsAtPosition` | `getCompletionsAtPosition` | ✅ Phase 4 |
| Completion details | `getCompletionDetailsAtPosition` | `getCompletionEntryDetails` | ✅ Phase 4 |
| Quick info / hover | `getQuickInfoAtPosition` | `getQuickInfoAtPosition` | ✅ Phase 4 |
| Go to definition | `getDefinitionAtPosition` | `getDefinitionAndBoundSpan` | ✅ Phase 4 |
| Find all references | — (lives in the plugin) | `findReferences` | ✅ Phase 4 |
| Rename info | `getRenameInfoAtPosition` | `getRenameInfo` | ✅ Phase 4 |
| Rename locations | `getRenameLocationsAtPosition` | `findRenameLocations` | ✅ Phase 4 |
| Code fixes | `getCodeFixesAtPositionRange` | `getCodeFixesAtPosition` | ✅ Phase 4 |
| Closing-tag completion | `getClosingTagAtPosition` | `getJsxClosingTagAtPosition` | ✅ Phase 4 |
| Signature help | — | `getSignatureHelpItems` | ✅ pass-through, Phase 4 |
| Folding / outlining | `getOutliningSpansInFile` | **commented out** in `decorate-language-service.ts:19` | ✅ Phase 4 — and actually wired |
| Format edits | `getFormatEditsInFile` | **commented out** at line 20 | ⚠️ see §5 |
| Colour decorators | — (extension-side regex) | — | ✅ Phase 4, on the parsed tree instead of a regex |
| Workspace analysis | CLI | `fast-plugin.analyze` command → `npx lit-analyzer` in a terminal | ✅ Phase 4, in-process |

Two of these are worth calling out: **folding and formatting are implemented in the analyzer and then
not connected**. `decorate-language-service.ts` has both lines commented out. RFC 011 treats folding
as a real feature and formatting as a decision to re-take — see §5.

---

## 3. The rule set

23 rule ids in `LitAnalyzerRuleId`; **21** have a rule module in `ALL_RULES`; `no-invalid-css` is
produced by the CSS service rather than a rule module; and `no-invalid-boolean-binding` has a default
severity, appears in the extension's settings schema, and **is referenced by no code at all**.

Severities below are `[normal, strict]` from `DEFAULT_RULES_SEVERITY`.

| Rule | Default | Needs the type checker | Fate in RFC 011 |
| --- | --- | :---: | --- |
| `no-unknown-tag-name` | off / warn | | ✅ carry |
| `no-missing-import` | off / warn | | ✅ carry |
| `no-unclosed-tag` | warn / error | | ✅ carry |
| `no-unknown-attribute` | off / warn | | ✅ carry |
| `no-unknown-property` | off / warn | | ✅ carry, FAST `:` semantics |
| `no-unknown-event` | off / off | | ✅ carry |
| `no-unknown-slot` | off / warn | | ✅ carry |
| `no-unintended-mixed-binding` | warn / warn | | ✅ carry |
| `no-expressionless-property-binding` | error / error | | ✅ carry |
| `no-invalid-attribute-name` | error / error | | ✅ carry |
| `no-invalid-tag-name` | error / error | | ✅ carry |
| `no-missing-element-type-definition` | off / off | | ✅ carry |
| `no-invalid-css` | warn / error | | ✅ carry (CSS service) |
| `no-noncallable-event-binding` | error / error | ● | ✅ carry |
| `no-boolean-in-attribute-binding` | error / error | ● | ✅ carry |
| `no-complex-attribute-binding` | error / error | ● | ✅ carry |
| `no-incompatible-type-binding` | error / error | ● | ✅ carry |
| `no-invalid-directive-binding` | error / error | ● | ⚙️ rewrite — FAST's directive set |
| `no-incompatible-property-type` | warn / error | ● | ⚙️ rewrite → `no-incompatible-attr-config` |
| `no-property-visibility-mismatch` | off / warning | | ⚙️ rewrite → `no-attr-visibility-mismatch` |
| `no-nullable-attribute-binding` | **off / off** | ● | ❌ drop — wrong for FAST |
| `no-legacy-attribute` | off / off | | ❌ drop — Polymer `foo$=` |
| `no-invalid-boolean-binding` | error / error | | ❌ drop — dead id |

**17 carried, 3 rewritten, 3 dropped**, plus 6 new FAST-native rules — 26 in total. One carried rule
changes its default (`no-unknown-event`, from `off` to `warn`). The full catalogue with messages,
quick fixes and rationale is [design/rules.md](../design/rules.md).

### 3.1 The seven type rules

These are the ones that force TypeScript to stay in the design
([proposal.md §3](../proposal.md#3-the-constraint-that-shapes-everything)). Identified by grepping
for `ts-simple-type`, `isAssignableTo`, `SimpleType` and `getType()`:

`no-boolean-in-attribute-binding`, `no-complex-attribute-binding`, `no-incompatible-property-type`,
`no-incompatible-type-binding`, `no-invalid-directive-binding`, `no-noncallable-event-binding`,
`no-nullable-attribute-binding`.

The shared helpers they route through are `rules/util/type/*` (10 files): `extract-binding-types`,
`is-assignable-to-type`, `is-assignable-in-{attribute,boolean,property,element}-binding`,
`is-assignable-binding-under-security-system`, `remove-undefined-from-type`.

---

## 4. Configuration surface

`fast-plugin.*` in the extension's `contributes.configuration`, mapped onto `LitAnalyzerConfig`:

| Setting | Type | Default | RFC 011 |
| --- | --- | --- | --- |
| `disable` | boolean | `false` | ✅ |
| `strict` | boolean | `false` | ✅ |
| `logging` | enum off/error/warn/debug/verbose | `off` | ✅ |
| `dontShowSuggestions` | boolean | `false` | ✅ |
| `maxProjectImportDepth` | integer | `-1` | ✅ |
| `maxNodeModuleImportDepth` | integer | `1` | ✅ |
| `securitySystem` | enum off/ClosureSafeTypes | `off` | ❌ lit-html-specific |
| `htmlTemplateTags` | string[] | `["html","raw"]` | ✅ default becomes `["html"]` |
| `cssTemplateTags` | string[] | `["css"]` | ✅ |
| `globalTags` | string[] | — | ✅ |
| `globalAttributes` | string[] | — | ✅ |
| `globalEvents` | string[] | — | ✅ |
| `customHtmlData` | path/object[] | — | ✅ (+ `html.experimental.customData` merge) |
| `rules.<id>` | enum default/off/warning/error | `default` | ✅ per rule |
| 8 deprecated aliases | | | ❌ new extension, no history to keep |

The deprecated aliases (`skipSuggestions`, `checkUnknownEvents`, `skipUnknownTags`,
`skipUnknownAttributes`, `skipUnknownProperties`, `skipUnknownSlots`, `skipMissingImports`,
`skipTypeChecking`) and `externalHtmlTagNames`/`externalHtmlTags`/`externalHtmlAttributes` exist
only to keep lit-plugin's old users working. A new extension starts clean.

`securitySystem` is dropped because it models `lit-html`'s `ClosureSafeTypes` sanitizer. FAST has
`DOMPolicy` instead ([research/fast-element.md §6](fast-element.md#6-dom-policy)), which is a
different mechanism; adding a rule for it would be new work, not parity, and it is listed under
[proposal.md §10](../proposal.md#10-deliberately-deferred).

---

## 5. Where fast-analyzer's answer is not the answer we want

Not everything on this page should be reproduced. These are the places where parity is the wrong
target, each with its own record:

| Behaviour | Why it is not carried over |
| --- | --- |
| Folding + formatting implemented then commented out | We wire folding. Formatting is re-decided in [design/features.md §10](../design/features.md#10-format-edits) — forwarding tsserver's own edits over the template range is the parity behaviour and it is what the commented-out code did |
| Colour decorators found by regex over the whole file (`/(css\|html)`([\s\S]*?)`/gi`) | We already have a parsed tree; a regex over unparsed source will find "colours" inside expressions and comments |
| `fast-plugin.analyze` shells out to `npx lit-analyzer` in a terminal | Requires the package to be installed, prints to a terminal, and cannot be clicked through. We run it in-process into the Problems panel |
| `findReferences` walks *every* tsserver project's `getSourceFiles()` on every call | Correct results, unbounded cost. The registry should be indexed, not re-scanned |
| `web-component-analyzer` runs on every source file, then FAST results are merged over the top | Two component models with a merge step between them. One model, [design/component-model.md](../design/component-model.md) |
| `MAX_RUNNING_TIME_PER_OPERATION = 150` | See §6 |

---

## 6. The 150 ms budget

`DefaultLitAnalyzerContext.isCancellationRequested` returns `true` once
`Date.now() - startTime > 150`, and the analyzer's loops check it and stop. When that happens it
calls `this.logger.error(…)` — but `logging` defaults to `off`, so **the log line goes nowhere and
the user sees a shorter list of diagnostics with no indication that anything was abandoned.** In the
CLI there is no cancellation token, so the wall-clock branch is the only one that fires.

This is cited in [proposal.md §1.3](../proposal.md#13-why-now-and-why-here) as evidence that the
workload is under time pressure, and it is the one behaviour RFC 011 should not reproduce as-is. If a
budget is needed at all, exceeding it has to be visible: a diagnostic on the template saying the
analysis was truncated, not silence.
