# Phase 4 — IDE features

**Goal**: everything that is not a diagnostic.
**Exit criterion**: `${ref('` completes with member names, and renaming a member updates its
template bindings and its `ref('…')` strings across files.
**Status**: ☐ 0 / 17

Specified by [design/features.md](../design/features.md).

| # | Task | Status |
| --- | --- | --- |
| P4-01 | Position resolution in Rust: offset → node → meaning (tag name, attribute name, modifier, attribute value, placeholder, directive argument, text). Every other task in this phase is a consumer of this one | ☐ |
| P4-02 | Completion: tag names, with import status and the auto-import edit in the details | ☐ |
| P4-03 | Completion: attributes, `:` properties, `?` booleans, `@` events, filtered by what is already used on the node | ☐ |
| P4-04 | Completion: attribute values from HTML data and from union types; `slot=`, `part=`, `exportparts=` | ☐ |
| P4-05 | **Completion inside `ref('` / `slotted('` / `children('`** — member names of the template's `TSource`. The feature TypeScript structurally cannot provide. [features.md §3](../design/features.md#3-completion) | ☐ |
| P4-06 | Completion: content-position snippets for `when`, `repeat`, `render`, and an `x => x.` skeleton | ☐ |
| P4-07 | `getCompletionEntryDetails`: lazy documentation, type, and the import edit | ☐ |
| P4-08 | Nearest-name suggestions with `strsim`. **Compare against `didyoumean2` on the fixtures** before adopting the parameters — a wrong suggestion is worse than none | ☐ |
| P4-09 | Quick info for tags, attributes, properties, events, slots, directives, and `ref('…')` | ☐ |
| P4-10 | Definition for all seven targets in [features.md §4](../design/features.md#4-definition), including `ref('name')` → the member | ☐ |
| P4-11 | References: tag occurrences from the registry index, including across tsserver projects — fed by `upsertFile`, not by rescanning every project's `getSourceFiles()` | ☐ |
| P4-12 | References: **member** occurrences — `:prop`, `?attr`, `@event`, `ref('…')` | ☐ |
| P4-13 | Rename info and locations for every row of [features.md §6](../design/features.md#6-rename), with an integration test that renames `CsvGrid.hasHeader` in the real corpus and re-typechecks | ☐ |
| P4-14 | Code fixes: every fix in [rules.md](../design/rules.md#quick-fixes) | ☐ |
| P4-15 | Closing tags and folding ranges. Folding is implemented-and-unwired in fast-analyzer; wire it | ☐ |
| P4-16 | CSS: `css` documents through `vscode-css-languageservice` — `no-invalid-css`, completion, hover, folding, colour. [0005](../decisions/0005-css-stays-in-typescript.md) | ☐ |
| P4-17 | Extension host: colour decorators over parsed template ranges (not a regex over the file), the `fastElementUltra.analyze` command reporting into a `DiagnosticCollection`, and a status item showing engine state — on, disabled, poisoned, TS version out of range | ☐ |

## Exit test

`extensions/fast-element-ultra/test/features.test.ts`, driving the real `.wasm` through the plugin's
own code paths against the corpus:

- Completion at `${ref('` in `csv-ultra/webview/viewer/template.ts:200` offers `findInput`.
- Definition on `csv-grid` in a template lands on `element.ts:112`.
- Rename of `CsvGrid.hasHeader` produces edits in `element.ts` **and** `template.ts`, and the result
  typechecks.
- Hover on `@click` shows the handler's expected signature.

## Notes

**P4-01 is a prerequisite for fourteen of the other sixteen tasks.** It is worth over-testing.

**P4-05 and P4-12 are the two features that justify the project to a user.** Everything else in this
phase is parity; these two are things no tool currently does, and they exist because we track the
template's source type ([component-model.md §6](../design/component-model.md#6-template-source-types)).

**P4-11's index is a behaviour change, not just a speed one.** fast-analyzer's version re-walks every
project on every invocation, which is correct and unbounded; an index can be stale in ways a rescan
cannot. Invalidation is per file via `upsertFile`, and the test for it is a cross-project rename that
runs after editing the *other* project.
