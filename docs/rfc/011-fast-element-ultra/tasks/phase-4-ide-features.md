# Phase 4 — IDE features

**Goal**: everything that is not a diagnostic.
**Exit criterion**: `${ref('` completes with member names, and renaming a member updates its
template bindings and its `ref('…')` strings across files.
**Status**: ☑ 15 / 17, ◐ 2 — exit test green (`test/features.test.ts`, including both criteria on
csv-ultra's real files)

| # | Task | Status |
| --- | --- | --- |
| P4-01 | Position resolution in Rust: offset → node → meaning | ☑ — `resolve_position` in `ide.rs`; the plugin-side twin lesson was `templateAt` choosing the **innermost** template, because `when(…, html\`…\`)` nests whole documents inside an outer expression |
| P4-02 | Completion: tag names, import status, auto-import edit in details | ☑ |
| P4-03 | Completion: attributes, `:` properties, `?` booleans, `@` events, filtered by what is present | ☑ |
| P4-04 | Completion: attribute values from HTML data and union types; `slot=`, `part=`, `exportparts=` | ☑ — component attribute values come from string-literal unions extracted at discovery (`MemberFact.values`) |
| P4-05 | **Completion inside `ref('` / `slotted('` / `children('`** | ☑ — computed by the **plugin** (the string lives inside a placeholder; the engine's `documentInfoAt` routes the position, `sourceMembers` supplies the members). Deviation recorded in features.md |
| P4-06 | Content-position snippets for `when`, `repeat`, `render`, binding skeleton | ☑ |
| P4-07 | `getCompletionEntryDetails`: lazy documentation, type, import edit | ☑ |
| P4-08 | Nearest-name suggestions with `strsim` | ◐ — jaro-winkler ≥ 0.84, pinned by fixtures (`findInpt`→`findInput`, `aria-pressd`→`aria-pressed`, unrelated names get nothing). The didyoumean2 side-by-side the task asked for was not run — `temp/fast-analyzer` is absent from the checkout; our own fixtures are the guard until it returns |
| P4-09 | Quick info for tags, attributes, properties, events, slots, directives, and `ref('…')` | ☑ |
| P4-10 | Definition for all targets including `ref('name')` → the member | ☑ |
| P4-11 | References: tag occurrences from the engine's documents | ◐ — within-project complete, implemented as a walk over the engine's parsed documents (already parsed, kilobytes each; never rescans tsserver files — the cost the design was rejecting). **Cross-project reach is not implemented**: engines are per-project and share nothing, so a reference in a project that has not loaded is not found. fast-analyzer's every-project rescan had this reach; ours trades it for boundedness. Recorded in features.md |
| P4-12 | References: member occurrences — `:prop`, `?attr`, `@event`, `ref('…')` | ☑ |
| P4-13 | Rename info + locations for every row of features.md §6, with the corpus integration test | ☑ — `findInput` renamed from the `ref('…')` string reaches `element.ts`, and from the declaration reaches the template string; every produced span is exactly the member name. (Undecorated members work through `sourceMembers` and the class-body fallback.) The "re-typechecks after applying" step asserts span-exactness rather than applying edits and re-running the checker |
| P4-14 | Code fixes: every fix in rules.md | ☑ — except `no-missing-element-type-definition`'s map-entry insertion, which is not shipped (rule defaults off; the message says what to add) |
| P4-15 | Closing tags and folding — wired | ☑ |
| P4-16 | CSS through `vscode-css-languageservice`: validation, completion, hover, folding | ☑ — via the package's ESM build (the UMD `main` breaks under bundling); placeholder-touching diagnostics filtered; placeholder-only stylesheets (typst's `css\`${sheet}\``) skip validation entirely |
| P4-17 | Extension host: colour decorators over template ranges, the analyze command, the status item | ☑ — analyze and status reach the plugin over custom tsserver protocol handlers via `typescript.tsserverRequest`, so workspace analysis genuinely runs against tsserver's own program |

## Notes

**P4-05 and P4-12/13 — the two features that justify the project — are the exit test's subject**,
run against csv-ultra's real template, not fixtures.

**P4-11's gap is the honest one to keep**: cross-project references need either a shared registry
across `Engine` instances or a plugin-level fan-out over projects; both are additive and neither
blocks the monorepo case where the projects that matter are open.
