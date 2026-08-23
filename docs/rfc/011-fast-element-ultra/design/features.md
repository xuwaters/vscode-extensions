# IDE features

**Status**: design, nothing built.

Per-feature contract: what it does, where it is computed, and what it does beyond
[fast-analyzer's version](../research/parity.md#2-ide-features). Settings are in §13.

Everything here is reached through a decorated `ts.LanguageService` method
([architecture.md §1](architecture.md#1-process-and-module-model)), except colour decorators and the
workspace command, which need `vscode`.

---

## 1. Diagnostics

`getSemanticDiagnostics` · Rust + oracle

The 26 rules of [rules.md](rules.md), merged with `no-invalid-css` from the CSS service, mapped back
to source-file spans and given a severity from the config.

Two behaviours differ from fast-analyzer:

- **No silent truncation.** If a budget is ever needed, exceeding it produces a visible diagnostic
  saying the analysis was incomplete, not a quietly shorter list
  ([research/parity.md §6](../research/parity.md#6-the-150-ms-budget)).
- **Diagnostics carry their origin.** A message about `<csv-grid>` says whether the tag is known from
  a declaration, from custom data, or from `globalTags`, because "unknown attribute on a tag we only
  half-know" is a different problem from "unknown attribute on a tag we fully know".

## 2. Quick info

`getQuickInfoAtPosition` · Rust

Hovering a tag name, attribute, property, event, slot name, or directive shows: the declared type,
where it is declared, its JSDoc, and — for an attribute — its `mode` and converter. For a tag, the
component's JSDoc and its slot/part/custom-property lists.

Beyond fast-analyzer: hovering `${ref('findInput')}` shows the member it names.

## 3. Completion

`getCompletionsAtPosition` + `getCompletionEntryDetails` · Rust

| Context | Offered |
| --- | --- |
| `<` | Tag names: declared components first, then built-ins, with import status shown |
| Inside a tag, at attribute position | Attributes not already used, plus `:`, `?`, `@` prefixed forms |
| After `:` | Properties of the tag |
| After `?` | Boolean-ish attributes of the tag |
| After `@` | Events the component emits, plus DOM events |
| Inside an attribute value with a fixed value set | The values, from HTML data or a union type |
| `slot="` | Slot names declared on the parent component |
| `part="` / `exportparts="` | CSS parts |
| **`${ref('` / `${slotted('` / `${children('`** | **Member names of the template's `TSource`** — new |
| Content position, after `${` | Snippets for `when`, `repeat`, `render`, and an `x => x.` skeleton |

The `ref('…')` completions are the ones TypeScript structurally cannot offer: the argument is a
string literal, so there is no symbol to complete. We have the template's `TSource`
([component-model.md §6](component-model.md#6-template-source-types)) and can.

Completion details resolve lazily — documentation, type, and the auto-import edit for an
unimported component.

## 4. Definition

`getDefinitionAndBoundSpan` · Rust resolves, TypeScript builds the span

| Cursor on | Goes to |
| --- | --- |
| A tag name | The component class declaration |
| An attribute | The `@attr` member, or the `attributes:` entry, or the JSDoc tag |
| A property | The `@observable`/`@volatile` member |
| An event | The `this.$emit("…")` call that emits it, or the `@fires` JSDoc |
| A slot name | The `@slot` JSDoc |
| **`ref('name')`** | **The member `name`** — new |
| A CSS part or custom property | Its JSDoc declaration |

## 5. References

`findReferences` · Rust

Cursor on a component class, its `@customElement` decorator, or the tag-name string → every use of
the tag in every template in the workspace, opening and closing tags both.

Beyond fast-analyzer: cursor on a **member** finds its template bindings too — `:prop`, `?attr`,
`@event`, and `${ref('…')}`.

fast-analyzer implements this by iterating every tsserver project and calling `getSourceFiles()` on
each, per invocation ([research/parity.md §5](../research/parity.md#5-where-fast-analyzers-answer-is-not-the-answer-we-want)).
The registry already indexes tag and member occurrences by document, so the query is a lookup. The
cross-project reach is kept — it is genuinely useful in this monorepo — but it is fed by
`upsertFile`, not by rescanning.

## 6. Rename

`getRenameInfo` + `findRenameLocations` · Rust

| Renaming | Also updates |
| --- | --- |
| A component class or its tag string | Every `<tag>` and `</tag>` in every template |
| An `@attr` member | Its `attr=` and `?attr=` bindings; the `attribute:` option if it names the same string |
| An `@observable` member | Its `:prop=` bindings, and `${ref('…')}` / `${slotted('…')}` / `${children('…')}` strings that name it |
| An event name in `$emit("…")` | Its `@event=` bindings |
| A slot name in `@slot` JSDoc | Its `slot="…"` uses |

TypeScript renames the declaration and its TypeScript references; we contribute the template
locations. The `${ref('…')}` case is new and is the one that currently breaks silently, because
renaming a member leaves a string that still compiles and no longer resolves.

## 7. Code fixes

`getCodeFixesAtPosition` · Rust

Every fix listed in [rules.md](rules.md#quick-fixes). Nearest-name suggestions use `strsim`.

## 8. Closing tags

`getJsxClosingTagAtPosition` · Rust

Typing `>` after `<my-element` inserts `</my-element>`. Suppressed for void elements and inside
foreign content where self-closing is legal.

## 9. Folding

`getOutliningSpans` · Rust

One span per element with children, plus one per `` html` ` `` and `` css` ` `` template.

**fast-analyzer implements this and then leaves the wiring commented out**
(`decorate-language-service.ts:19`). We wire it.

## 10. Format edits

`getFormattingEditsForRange` · TypeScript

Also commented out in fast-analyzer (line 20), and here the decision goes the other way: we forward
TypeScript's own format edits for the template range and write no HTML formatter of our own
([proposal.md §2](../proposal.md#2-goals-and-non-goals), non-goals). Reformatting HTML inside a
template is a large problem with strong opinions attached, and Prettier already solves it for people
who want it.

## 11. Colour decorators

`vscode.DocumentColorProvider` · extension host

Swatches for colours inside `` html` ` `` and `` css` ` ``.

fast-analyzer finds them with a regex over the raw file
(`/(css|html)`([\s\S]*?)`/gi` then `/#[0-9a-fA-F]+/gi`), which matches "colours" inside expressions,
comments and unrelated strings. The extension host has no engine, so it re-derives template ranges
with the substitution rules from [architecture.md §3](architecture.md#3-the-virtual-document) and
only looks inside literal parts — ~40 lines of duplication, and correct.

## 12. Workspace analysis

`fastElementUltra.analyze` command · extension host + plugin

Analyse every FAST template in the workspace and report into a `DiagnosticCollection` with a progress
notification.

fast-analyzer's version opens a terminal and runs `npx lit-analyzer <glob>`, which requires the
package to be installed, prints text, and cannot be clicked through. Running in-process gives
clickable results and reuses the program tsserver already has.

## 13. Settings

Namespace `fastElementUltra.*`, matching the repo's convention (`typstUltra.*`, `csvUltra.*`).

| Setting | Type | Default |
| --- | --- | --- |
| `disable` | boolean | `false` |
| `strict` | boolean | `false` |
| `logging` | `off` \| `error` \| `warn` \| `debug` \| `verbose` | `off` |
| `dontShowSuggestions` | boolean | `false` |
| `htmlTemplateTags` | string[] | `["html"]` |
| `cssTemplateTags` | string[] | `["css"]` |
| `maxProjectImportDepth` | integer | `-1` |
| `maxNodeModuleImportDepth` | integer | `1` |
| `globalTags` | string[] | `[]` |
| `globalAttributes` | string[] | `[]` |
| `globalEvents` | string[] | `[]` |
| `customHtmlData` | string \| object \| array | — |
| `rules.<id>` | `default` \| `off` \| `warning` \| `error` | `default` |

Merged with `html.experimental.customData`, as fast-analyzer does.

Dropped relative to fast-analyzer: `securitySystem` (lit-html's sanitizer model), the 8 deprecated
`skip*` aliases, and `externalHtmlTag*` — all of which exist to keep lit-plugin's history working
([research/parity.md §4](../research/parity.md#4-configuration-surface)).

`htmlTemplateTags` drops `"raw"` from the default, which is lit's raw-text tag. FAST's escape hatch
is `html.partial(…)` and is handled in the parser, not by a tag name.

## 14. Syntax highlighting

TextMate injection grammar · Phase 5

Highlights HTML inside `` html` ` `` and CSS inside `` css` ` ``, with FAST's binding prefixes
(`:`, `?`, `@`) marked distinctly so that a `:prop` reads as a property binding rather than as a
malformed attribute.

fast-analyzer ships the `vscode-lit-html` and `vscode-styled-components` grammars, both MIT, with one
line changed. Vendoring them the same way is the cheap path and requires carrying their licences;
whether to do that or write a FAST-specific grammar is task P5-01.
