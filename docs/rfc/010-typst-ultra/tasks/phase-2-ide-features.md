# Phase 2 — IDE features

**Goal**: the full language-server surface, plus the two systems that make real projects work — packages
and compile-root resolution.
**Exit criterion**: `#import "@preview/cetz:0.4.2"` completes, downloads, resolves, and jumps to definition.
**Status**: ☐ Not started — 0 / 17

## Tasks

### The `typst-ide` trio

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P2-01 | Completion: `typst_ide::autocomplete`, `CompletionKind` → `CompletionItemKind` mapping, `apply` → snippet `insertTextFormat`, replacement offset → `textEdit` range (never `insertText`), trigger characters `# . @ / " : $` | [lsp-features.md §3.1](../design/lsp-features.md#31-completion) | ☐ |
| P2-02 | Hover: `typst_ide::tooltip`, `Text` → markdown / `Code` → fenced `typst` block, `Side::Before` with `Side::After` retry, plus our page-number extension for labels | [lsp-features.md §3.2](../design/lsp-features.md#32-hover) | ☐ |
| P2-03 | Goto-definition: `Definition::{Span, File, Std}`; `Std` responds `null` with hover carrying the docs | [lsp-features.md §3.3](../design/lsp-features.md#33-goto-definition) | ☐ |

### Features we assemble

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P2-04 | Semantic tokens `full` **and** `full/delta`: all 22 `Tag` values, custom token types declared in the legend, `contributes.semanticTokenScopes` fallbacks. Delta is not optional — a document emits thousands of tokens | [lsp-features.md §4.1](../design/lsp-features.md#41-semantic-tokens), [0007](../decisions/0007-textmate-grammar.md) | ☐ |
| P2-05 | Document symbols, nested by heading level, including `#let` bindings, show/set rules, labels, imports | [lsp-features.md §4.3](../design/lsp-features.md#43-document-and-workspace-symbols) | ☐ |
| P2-06 | Workspace symbols: same walk over all workspace `.typ` files, trees cached by mtime, fuzzy filter | [lsp-features.md §4.3](../design/lsp-features.md#43-document-and-workspace-symbols) | ☐ |
| P2-07 | References: labels via `analyze_labels` + `Ref` walk; local bindings via `named_items` inverted | [lsp-features.md §4.2](../design/lsp-features.md#42-references-and-rename) | ☐ |
| P2-08 | Rename + `prepareRename`, with the three explicit refusals (package-defined, std-library, outside the compile graph) each carrying its own message | [lsp-features.md §4.2](../design/lsp-features.md#42-references-and-rename) | ☐ |
| P2-09 | Folding ranges: headings, code blocks, content blocks, arrays/dicts, comment runs | [lsp-features.md §4.4](../design/lsp-features.md#44-folding-selection-ranges-document-links) | ☐ |
| P2-10 | Selection ranges: ancestor chain of `LinkedNode::leaf_at` | [lsp-features.md §4.4](../design/lsp-features.md#44-folding-selection-ranges-document-links) | ☐ |
| P2-11 | Document links: `#import`, `#include`, `image()`, `read()`, `bibliography()`, `link()`; only offered when the target resolves | [lsp-features.md §4.4](../design/lsp-features.md#44-folding-selection-ranges-document-links) | ☐ |
| P2-12 | Formatting + range formatting via `typstyle-core` (`partial` for ranges); respond `null` on `Error::SyntaxError` rather than mangling the file | [lsp-features.md §4.5](../design/lsp-features.md#45-formatting) | ☐ |

### Systems

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P2-13 | System font discovery: platform font directories + `typstUltra.fonts.paths`, background indexing after bundled fonts are live, `typst/fontsChanged` → recompile. **Measure first-run cost on a large font directory** (research debt) | [0004](../decisions/0004-bundle-default-fonts.md) | ☐ |
| P2-14 | Packages: `PackageProvider` with `Ready`/`Pending`/`Failed`, Universe download, **path-traversal-checked** untar, cache in typst's standard directory (shared with `typst-cli`), `typst/packageStatus` notifications, and a diagnostic on the `#import` line while pending. Respect `packages.enabled` for air-gapped setups | [architecture.md §7.1](../design/architecture.md#71-host-and-wasm-inside-the-server-process), [proposal.md §9](../proposal.md#9-security) | ☐ |
| P2-15 | Compile root, both modes: `typstUltra.mainFile` setting, `pinMain`/`unpinMain` commands with `workspaceState`, status-bar item + QuickPick, the one-shot "pin `main.typ`?" suggestion, and the "not in project" status for unrelated files | [0008](../decisions/0008-compile-root.md) | ☐ |
| P2-16 | On-enter list and comment continuation, host-side via `onEnterRules` so it lands before the LSP round-trip | [lsp-features.md §6](../design/lsp-features.md#6-what-the-extension-does-instead-of-the-server) | ☐ |
| P2-17 | Per-feature snapshot tests over fixtures with a cursor marker, following `typst-ide`'s own test shape; dedicated unit tests for semantic-token delta encoding | [proposal.md §13](../proposal.md#13-testing) | ☐ |

## Settings added in Phase 2

`semanticTokens` · `formatter.mode` · `formatter.printWidth` · `formatter.indentSize` ·
`fonts.system` · `packages.enabled` · `packages.registry` · `packages.cachePath`

Plus `mainFile` becomes fully honoured (P2-15).

## Definition of done

- [ ] Every method in [lsp-features.md §1](../design/lsp-features.md#1-capabilities-at-a-glance) marked Phase 2 responds
- [ ] A cold-cache `#import "@preview/cetz:0.4.2"` downloads, resolves, and compiles without a restart
- [ ] Rename refuses on a package symbol with a message naming the package
- [ ] Editing `chapters/03.typ` with `main.typ` pinned produces whole-document diagnostics
- [ ] `packages.enabled: false` produces a clear diagnostic instead of a hang
- [ ] Semantic tokens survive `full/delta` round-trips over a scripted edit sequence
