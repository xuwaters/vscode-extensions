# The corpus: this repository's own FAST code

**Measured**: 2026-08-22, over `extensions/*/` excluding `node_modules/` and `dist/`.

Three extensions in this repo write their webviews in FAST Element. That makes them the natural
regression corpus for RFC 011 — real templates, written without a linter, by someone who will notice
a false positive the same week it appears.

---

## 1. What is there

| | csv-ultra | pdf-ultra | typst-ultra | total |
| --- | ---: | ---: | ---: | ---: |
| `template.ts` lines | 371 | 445 | 145 | 961 |
| Files importing `@microsoft/fast-element` | 3 | 6 | 3 | **16** (incl. tests) |

Counted across all 16 files:

| Construct | Count |
| --- | ---: |
| `html<T>`…`` — **typed** tagged templates | **26** |
| ``html`…` `` — untyped tagged templates | **0** |
| ``css`…` `` | 3 |
| `@event="${…}"` bindings | 74 |
| `attr="${…}"` bindings | 29 |
| `${…}` in content position | 31 |
| `:prop="${…}"` bindings | 7 |
| `?bool="${…}"` bindings | 5 |
| `${ref('…')}` | 17 |
| `${when(…)}` | 15 |
| `${repeat(…)}` | 2 |
| `${slotted(…)}` / `${children(…)}` / `${render(…)}` | 0 |
| `@customElement(…)` | 5 |
| `@observable` members | 55 |
| `@attr` members | **0** |
| `@volatile` members | 0 |
| `this.$emit(…)` | 0 |

Two shapes of the distribution matter for design:

- **Every template is typed.** 26 of 26 use `html<T>`, so `x` in `x => x.foo` has a real type and
  TypeScript already checks the expression body. Our job is the *target* side: is what that
  expression returns valid for the attribute, property, boolean or event it is bound to.
  A rule that warns on untyped templates ([design/rules.md](../design/rules.md#no-untyped-template))
  would currently fire zero times here, which is the correct steady state and not a reason to skip it.
- **Bindings are overwhelmingly events and attributes**, not properties. `no-unknown-event` — which
  fast-analyzer defaults to `off` in both normal and strict mode — is the rule with the most
  surface here. Worth revisiting its default.

---

## 2. Why fast-analyzer finds none of it

All three extensions register their element the same way:

```ts
// extensions/csv-ultra/webview/viewer/element.ts:89,112
export const CSV_GRID_TAG = 'csv-grid';

@customElement({ name: CSV_GRID_TAG, template, styles })
export class CsvGrid extends FASTElement implements SheetView { … }
```

| File | Line | Registration |
| --- | ---: | --- |
| `extensions/csv-ultra/webview/viewer/element.ts` | 112 | `@customElement({ name: CSV_GRID_TAG, template, styles })` |
| `extensions/pdf-ultra/webview/viewer/element.ts` | 82 | `@customElement({ name: PDF_VIEWER_TAG, template, styles })` |
| `extensions/typst-ultra/webview/viewer/element.ts` | 72 | `@customElement({ name: TYPST_PREVIEW_TAG, template, styles, shadowOptions: null })` |

and `extractFastElementTagName` requires a string literal:

```ts
// temp/fast-analyzer/…/flavors/fast-element-analyzer.ts:91-98
if (ts.isObjectLiteralExpression(arg)) {
    const nameProp = arg.properties.find(p => … p.name.escapedText === "name");
    if (nameProp != null && ts.isStringLiteralLike(nameProp.initializer)) {
        return nameProp.initializer.text;   // ← never reached for an identifier
    }
}
return undefined;                            // ← what actually happens, 3 of 3
```

`analyzeClassDeclaration` returns `undefined`, no `HtmlTag` is produced, nothing is merged into the
store, and **every downstream feature is unavailable for all three elements**: no completion for
`<csv-grid>`'s attributes, no hover, no go-to-definition on the tag, no unknown-attribute checking,
no rename propagation.

`ts-lit-plugin`'s `extractTagNameFromClass` — used only by `findReferences` and the rename path —
has the same limitation on the object form, so find-all-references on `CsvGrid` also returns only
what TypeScript itself found.

The fix in the fork is two lines: ask the checker for the type of `nameProp.initializer` and read
its `StringLiteral` value. The reason this is in an RFC rather than a pull request is that it is
symptomatic — [design/component-model.md §5](../design/component-model.md#5-what-fast-analyzer-covers-today)
lists ten more constructs in the same state.

---

## 3. Constructs the corpus exercises that the design must handle

Pulled from reading the three templates, not from counting:

| Construct | Where | Why it is awkward |
| --- | --- | --- |
| Literal text and a placeholder in one attribute value | `class="btn ${x => …}"`, csv-ultra:63 | Mixed binding: the attribute has both a literal and an expression part |
| Two placeholders in one attribute value | `style="left: ${…}px; top: ${…}px"`, csv-ultra:329 | The mixed-binding path with an index per placeholder |
| Element expression between attributes | `<div class="table" ${ref('tableEl')} tabindex="0">`, csv-ultra:289 | A placeholder in *attribute-name* position — this is why the substitution has to be a legal attribute name, not just a legal value |
| **Nested templates in `when`** | csv-ultra:154, 164 (nested twice), 191, 309, 347; typst-ultra:125 | An inner tagged template inside an expression of the outer one. Two virtual documents; the inner one keeps the outer `TSource` |
| **Nested templates in `repeat`, with a changed source type** | `html<MenuItem, CsvGrid>` at csv-ultra:333; `html<OutlineRow, PdfViewer>` at pdf-ultra:51 | Inside a `repeat` the item becomes `TSource` and the outer component becomes `TParent`. Every binding in that template resolves members against `MenuItem`, not `CsvGrid` |
| `ExecutionContext.parent` access | `@click="${(item, c) => c.parent.pick(item)}"`, csv-ultra:334 | The second arrow parameter is `ExecutionContext<TParent>`; reaching the outer component goes through it |
| Inline SVG with self-closing tags | 12 `/>` in csv-ultra's template alone | Foreign content: self-closing is legal there and `no-unclosed-tag` must not fire |
| A binding whose expression is a call, not an arrow | `@keydown="${keys((x, e) => x.onKeydown(e))}"`, csv-ultra:289 | The arrow-unwrapping heuristic sees a `CallExpression` |
| Tag name from a `const` | `@customElement({ name: CSV_GRID_TAG … })` | §2 |
| `shadowOptions: null` | typst-ultra:72 | Light DOM: slot rules and `::part` do not apply |

Not present in the corpus but legal, so still owed a test: an unquoted placeholder attribute value
(`?disabled=${x => x.locked}`), `${slotted('…')}`, `${children('…')}`, `${render(…)}`, `@attr`
members, `@volatile`, and `this.$emit(…)`.

Two of these deserve extra weight in the design:

- **`repeat` rebinds `TSource`.** Half the value of a FAST analyzer is knowing which type `x` is at
  a given point in the tree, and `repeat(…, html<Item, Parent>`…`)` moves it. Any feature that
  resolves a member name — completion inside `ref('…')`, `no-non-reactive-binding`, rename — has to
  track the source type per *template*, not per file. [design/component-model.md §6](../design/component-model.md#6-template-source-types)
  makes this explicit.
- **`keys(…)` breaks the arrow heuristic.** `extract-binding-types.ts`'s
  `unwrapArrowFunctionReturnType` calls `type.getCallSignatures()` and takes `[0]`. Here that works
  by luck: `keys` returns a function, so a call signature is found on the *result* type and the
  binding is treated as a handler. Nothing about the heuristic guarantees that in general — see task
  P3-08, which is to enumerate the cases before copying it.

---

## 4. How the corpus is used

Not as a benchmark — as a correctness gate. Three checks, run in CI from Phase 2 onward:

1. **Discovery**: all 5 elements found, with their `@observable` members, from all three files.
2. **Zero false positives**: the full rule set over all 26 templates produces no diagnostic. Any
   diagnostic here is either a real bug in our extensions (fix the extension, record it) or a bug in
   a rule (fix the rule). Both outcomes are useful; silence is the expected one.
3. **Round-trip**: renaming a member updates every `:prop` binding and every `${ref('…')}` that names
   it, and the file still typechecks.

The corpus is small — 961 lines of template. It is *not* a substitute for a scale test; see
[spikes.md](spikes.md) for what has to be measured on something larger, and note that no such
measurement exists yet.
