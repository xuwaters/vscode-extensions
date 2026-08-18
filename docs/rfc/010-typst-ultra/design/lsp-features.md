# RFC 010 — LSP Feature Surface

What the server implements, which upstream API each feature rests on, and how much of it is ours to write.
Crate structure is in [crates.md §3](crates.md#3-typst-lsp-core).

Legend for **Source**:
🟢 upstream `typst-ide` does the semantic work · 🟡 upstream primitives, our assembly · 🔵 entirely ours

---

## 1. Capabilities at a glance

| LSP method | Source | Phase | Notes |
| --- | --- | --- | --- |
| `textDocument/publishDiagnostics` | 🟡 | 1 | `typst::compile` warnings + errors, span-mapped |
| `textDocument/completion` | 🟢 | 2 | `typst_ide::autocomplete` — 181 items at a bare cursor |
| `textDocument/hover` | 🟢 | 2 | `typst_ide::tooltip` |
| `textDocument/definition` | 🟢 | 2 | `typst_ide::definition` |
| `textDocument/documentSymbol` | 🔵 | 2 | Syntax-tree walk |
| `workspace/symbol` | 🔵 | 2 | Union of per-file symbol walks |
| `textDocument/references` | 🟡 | 2 | `named_items` + `analyze_labels` + walk |
| `textDocument/rename`, `prepareRename` | 🟡 | 2 | Same primitives, guarded |
| `textDocument/semanticTokens/full`, `/full/delta` | 🟡 | 2 | `typst_syntax::highlight` → `Tag` |
| `textDocument/foldingRange` | 🔵 | 2 | Syntax-tree walk |
| `textDocument/selectionRange` | 🔵 | 2 | Ancestor chain of the leaf node |
| `textDocument/documentLink` | 🔵 | 2 | Import/include/image paths |
| `textDocument/formatting`, `/rangeFormatting` | 🟢 | 2 | `typstyle-core` (`partial` module for ranges) |
| `textDocument/inlayHint` | 🟡 | 4 | Parameter names at call sites |
| `textDocument/completion` (postfix) | 🔵 | 4 | `x.rect` → `rect(x)`, sorted under upstream's items |
| `textDocument/signatureHelp` | 🟡 | 4 | From `Func` metadata; see [proposal.md §10](../proposal.md#10-known-limitations-from-the-no-fork-constraint) |
| `textDocument/codeAction` | 🔵 | 4 | Quick fixes for a small set of diagnostics |
| `textDocument/codeLens` | 🔵 | 4 | "Preview" / "Export" above the document |
| `typst/*` (preview, export, status) | 🔵 | 3 | [architecture.md §7.2](architecture.md#72-extension-host-and-server-lsp) |
| BibTeX, in `.bib` files and citations | 🔵 | 6 | Same methods, routed by extension — [§8](#8-bibtex-bibliographies) |

Not implemented, deliberately: call hierarchy, type hierarchy, type definition, implementation, moniker,
linked editing, color provider, on-type formatting.

---

## 2. Diagnostics (Phase 1)

The whole of Phase 1, and the feature that proves the architecture end to end.

```rust
let warned: Warned<SourceResult<PagedDocument>> = typst::compile::<PagedDocument>(&world);
comemo::evict(evict_age);   // AFTER. See architecture.md §6.
```

`warned.warnings` is always populated; `warned.output` is `Err(EcoVec<SourceDiagnostic>)` on failure. Each
`SourceDiagnostic` gives us:

| Field | Maps to |
| --- | --- |
| `severity` | `DiagnosticSeverity::{Error, Warning}` |
| `span` | `WorldExt::range(span)` → byte range → UTF-16 `Range` |
| `message` | `Diagnostic::message` |
| `hints` | Appended to the message as `hint: …` lines (VSCode renders multi-line messages in the hover) |
| `trace` | `DiagnosticRelatedInformation[]` — the `#import`/`#include` chain that reached the error |

Three behaviours worth specifying because they are the difference between usable and irritating:

1. **Diagnostics are published for every file in the compile graph**, not just the open one. An error
   inside an imported `theme.typ` appears in the Problems panel with the right URI, and its `trace`
   points back to the import site.
2. **A file that had diagnostics and now has none must be published as an empty array**, or stale
   squiggles persist. The server tracks the previous diagnostic URI set per compile and clears the
   difference.
3. **Compile results for superseded document versions are dropped.** Every compile carries the version it
   started from ([architecture.md §5](architecture.md#5-concurrency-model-one-thread-two-clocks)).

Controlled by `typstUltra.diagnostics.enabled` and `typstUltra.compile.when` (`onType` | `onSave` |
`never`).

---

## 3. The `typst-ide` trio (Phase 2)

These three are nearly free, which is the single biggest reason this project is affordable.

### 3.1 Completion

```rust
// temp/typst/crates/typst-ide/src/complete.rs:38
pub fn autocomplete(
    world: &dyn IdeWorld,
    output: Option<impl AsOutput>,
    source: &Source,
    cursor: usize,
    explicit: bool,
) -> Option<(usize, Vec<Completion>)>
```

Returns the replacement start offset and the items. `Completion` carries `kind`, `label`, `apply`
(snippet syntax like `${lhs} + ${rhs}`), and `detail`.

Mapping `CompletionKind` → LSP:

| `CompletionKind` | `CompletionItemKind` | Note |
| --- | --- | --- |
| `Syntax` | `Snippet` | |
| `Func` | `Function` | |
| `Type` | `Class` | |
| `Param` | `Field` | |
| `Constant` | `Constant` | |
| `Path` | `File` | Needs `IdeWorld::files()` — we implement it |
| `Package` | `Module` | Needs `IdeWorld::packages()` — fed by the host's Universe index |
| `Label` | `Reference` | Only produced when a compiled document is available |
| `Font` | `Text` | Sourced from our `FontBook` |
| `Symbol(EcoString)` | `Text` with the glyph as `detail` | The `sym.*` / `emoji.*` namespaces |

`apply` uses `${…}` placeholders, so items with `apply` get `insertTextFormat: Snippet`; everything else is
plain text. The replacement start offset becomes the `textEdit` range — never `insertText`, because typst
completions frequently replace back past the cursor (e.g. `#` triggers a whole-expression completion).

Two things we add on top:
- **`triggerCharacters`**: `#`, `.`, `@`, `/`, `"`, `:`, and `$`.
- **`explicit`** is `true` when the request came from an explicit invoke, `false` for automatic triggering —
  upstream uses it to decide how aggressive to be.

Two things upstream does not give us — and which [P4-13](../tasks/phase-4-polish.md) rebuilt as a pure
syntax feature, as predicted: postfix completions (`x.rect` → `rect(x)`) and UFCS variants.

The subtlety is *when* to offer them. In markup, `#value.` parses as a value followed by a full stop —
because that is what people usually mean — so the dot is a `Text` node, not a `FieldAccess`. Upstream's
own `complete_field_accesses` distinguishes the two shapes, and so do we. Postfix items sort under a `z`
prefix, so a real field on the value always wins.

One more thing we add: **`sortText` on every item**, derived from upstream's returned order. Without it a
client re-sorts alphabetically and throws away the relevance ranking.

### 3.2 Hover

```rust
// temp/typst/crates/typst-ide/src/tooltip.rs:24
pub fn tooltip(world, output, source, cursor, side: Side) -> Option<Tooltip>
pub enum Tooltip { Text(EcoString), Code(EcoString) }
```

Covers named-parameter docs, font info, label previews, and import targets. `Text` becomes a plain markdown
string; `Code` becomes a fenced ` ```typst ` block. `side` comes from whether the cursor sits at a token
boundary — we pass `Side::Before` and retry with `Side::After` on `None`, matching how upstream's own tests
probe.

We extend the hover with one thing of ours: when the hovered node is a label or reference and a compiled
document exists, we append the page number it resolves to.

### 3.3 Goto-definition

```rust
// temp/typst/crates/typst-ide/src/definition.rs:27
pub enum Definition { Span(Span), File(FileId), Std(Value) }
```

- `Span` → `WorldExt::range` → a `Location` in the owning file.
- `File` → the whole imported/included file, range `0:0`.
- `Std` → no location exists. We respond with `null` and instead surface the standard-library
  documentation through hover. (Tinymist opens a generated docs page here; that needs a docs pipeline we
  are not building in Phase 2.)

---

## 4. Features we assemble (Phase 2)

### 4.1 Semantic tokens

The real syntax highlighting. `typst_syntax::highlight(&LinkedNode) -> Option<Tag>` gives 22 tags from the
actual parser, so coloring is correct by construction rather than by regex approximation.

| `Tag` | LSP token type | `semanticTokenScopes` fallback |
| --- | --- | --- |
| `Comment` | `comment` | `comment` |
| `Keyword` | `keyword` | `keyword.control` |
| `Operator`, `MathOperator` | `operator` | `keyword.operator` |
| `Number` | `number` | `constant.numeric` |
| `String` | `string` | `string.quoted` |
| `Function` | `function` | `entity.name.function` |
| `Punctuation`, `MathGroupingParens` | `punct`* | `punctuation` |
| `Escape` | `escape`* | `constant.character.escape` |
| `Strong` | `strong`* | `markup.bold` |
| `Emph` | `emph`* | `markup.italic` |
| `Link` | `link`* | `markup.underline.link` |
| `Raw` | `raw`* | `markup.raw` |
| `Label` | `label`* | `entity.name.label` |
| `Ref` | `ref`* | `markup.other.reference` |
| `Heading` | `heading`* | `markup.heading` |
| `ListMarker` | `listMarker`* | `punctuation.definition.list.begin` |
| `ListTerm` | `listTerm`* | `markup.bold` |
| `MathDelimiter` | `mathDelimiter`* | `punctuation.definition.math` |
| `Interpolated` | `interpolated`* | `meta.interpolation` |
| `Error` | `error`* | `invalid.illegal` |

`*` = custom token type, declared in the server's legend and given a TextMate scope fallback via
`contributes.semanticTokenScopes` so themes without explicit support still color it sensibly.

Both `full` and `full/delta` are implemented — delta matters here because a document produces thousands of
tokens and re-sending them on every keystroke is wasteful. The server keeps the previous token array per
document and emits `SemanticTokensDelta` edits.

Two things the implementation had to work out, neither obvious from upstream's signature:

- **Tags nest and overlap.** `typst_syntax::highlight` reports `= *AB*` as `0..6 Heading` **and**
  `2..6 Strong`; LSP tokens may not overlap. Resolved by tagging **leaves only**, carrying the innermost
  tag from the ancestor chain — so bold text inside a heading is bold, not heading-coloured.
- **A token may not span lines**, which a raw block or block comment does. Those are split per line.

And one trap worth naming: the token cache holds what the **client** has, so `didChange` must *not* clear
it. Clearing it turns every delta request into a full resend and silently deletes the feature. Caught by
`deltas_round_trip_over_an_edit_sequence`, which applies the emitted edits and compares.

Because the TextMate grammar is deliberately minimal ([proposal.md §11.1](../proposal.md#11-open-questions)),
semantic tokens are not a garnish — they are the primary coloring mechanism, and
`typstUltra.semanticTokens: "disable"` should be understood as "fall back to approximate coloring".

### 4.2 References and rename

`typst-ide` gives the resolution primitives; the search is ours.

- **Labels and references** (`<intro>` / `@intro`): `analyze_labels` on the compiled document plus a
  syntax walk for `Ref` nodes. This is the common case and it is reliable.
- **Local bindings** (`#let x = …`): `named_items` walks the scope chain from a node. We invert it —
  resolve the definition, then walk the file's tree collecting identifiers that resolve to the same
  definition span.
- **Imported items**: resolvable within the compile graph. Files outside it are not searched.

Rename applies the same set as a `WorkspaceEdit`. `prepareRename` **refuses** in three cases, each with an
explicit message rather than a silent no-op:

| Refuse when | Message |
| --- | --- |
| The symbol is defined in a package (read-only) | "Cannot rename an item defined in package `@preview/…`" |
| The symbol is a standard-library item | "Cannot rename a standard library item" |
| The definition is outside the compile root's file graph | "Cannot rename: `…` is not reachable from the current main file" |

### 4.3 Document and workspace symbols

A syntax-tree walk producing a `DocumentSymbol` hierarchy:

| Node | Symbol kind | Nesting |
| --- | --- | --- |
| `Heading` | `String` | Nested by level, so `=` contains `==` — the outline users actually want |
| `#let f(..) = ..` | `Function` | Under the enclosing heading |
| `#let x = ..` | `Variable` | " |
| `#show`/`#set` rules | `Event` | " |
| `<label>` | `Key` | " |
| `#import`/`#include` | `Module` | Top level |

`workspace/symbol` runs the same walk over every `.typ` file the host reports, with a substring/fuzzy
filter. Files are parsed on demand and their trees cached, keyed by mtime.

### 4.4 Folding, selection ranges, document links

All three are direct consequences of having a real syntax tree, and all three are cheap:

- **Folding**: headings (to the start of the next heading of equal or lower level), code blocks, content
  blocks, arrays/dicts, and consecutive line comments.
- **Selection ranges**: the ancestor chain of `LinkedNode::leaf_at(offset)` — expand-selection that
  respects typst's actual grammar, including math.
- **Document links**: `#import "…"`, `#include "…"`, `image("…")`, `read("…")`, `bibliography("…")`, and
  `link("https://…")`. Relative paths resolve through the same `VirtualPath` logic the compiler uses, so
  a link is only offered when the target actually resolves.

### 4.5 Formatting

`typstyle-core` 0.15.1, the same version as the compiler:

```rust
let cfg = typstyle_core::Config::default()
    .with_width(print_width)
    .with_tab_spaces(indent_size);
typstyle_core::Typstyle::new(cfg).format_source(source.clone()).render()
```

`render()` returns `Err(Error::SyntaxError)` when the document has parse errors — we respond with `null`
rather than mangling the file. Range formatting uses `typstyle_core::partial`. Settings:
`typstUltra.formatter.mode` (`typstyle` | `off`), `.printWidth` (default 80), `.indentSize` (default 2).

---

## 5. Phase 4 features

- **Inlay hints**: parameter names at call sites for positional arguments, derived from the callee's `Func`
  metadata. Off by default for markup-heavy files, where they add noise.
- **Signature help**: from `Func` params. Without tinymist's compiler patches we cannot show *evaluated*
  argument values, only declared parameters, types, and defaults — enough for the overwhelming majority of
  uses.
- **Code actions**: a curated set tied to specific diagnostics — "add missing import", "wrap in
  `#{…}`", "convert `\"…\"` to a content block", "add `<label>` for this heading".
- **Code lenses**: "Preview" and "Export as…" above the first line, mirroring tinymist's affordance.

---

## 6. What the extension does instead of the server

Some things are genuinely better in TypeScript, and putting them in the server would be dogma:

| Behaviour | Why host-side |
| --- | --- |
| On-enter list/comment continuation | Needs to run before the LSP round-trip to feel instant. An `onEnterRules` in `language-configuration.json` plus one command |
| Bracket/quote auto-closing, `$…$` pairing | `language-configuration.json` |
| Status bar (compile state, page count, main file) | Fed by `typst/compileStatus` notifications |
| Export save dialogs, "reveal in Finder" | VSCode APIs |
| System font discovery | Needs `fs` and platform-specific directories |
| Package download | Needs the network |

---

## 7. Commands and Settings

### Commands (12)

| Command | Title | Default keybinding |
| --- | --- | --- |
| `typstUltra.showPreview` | Typst: Show Preview | `ctrl+shift+v` / `cmd+shift+v` |
| `typstUltra.showPreviewToSide` | Typst: Show Preview to the Side | `ctrl+k v` / `cmd+k v` |
| `typstUltra.syncPreviewToCursor` | Typst: Sync Preview to Cursor | `ctrl+k ctrl+j` |
| `typstUltra.pinMain` | Typst: Pin This File as Compile Root | |
| `typstUltra.unpinMain` | Typst: Unpin Compile Root | |
| `typstUltra.export` | Typst: Export… (QuickPick) | |
| `typstUltra.exportPdf` | Typst: Export PDF | |
| `typstUltra.toggleInvertColors` | Typst: Toggle Preview Color Inversion | |
| `typstUltra.restartServer` | Typst: Restart Language Server | |
| `typstUltra.showLog` | Typst: Show Log | |
| `typstUltra.clearPackageCache` | Typst: Clear Package Cache | |
| `typstUltra.newFromTemplate` | Typst: New Project from Template… | |

### Settings (24, all under `typstUltra.`)

All present as specified. `preview.renderMode` is live rather than deferred —
[P4-05](../tasks/phase-4-polish.md) shipped in the same pass.

| Setting | Default | Purpose |
| --- | --- | --- |
| `rootPath` | `""` | Compile root; empty = workspace folder. Determines what `/absolute.typ` means |
| `mainFile` | `""` | Compile entry. Empty = follow the focused editor. Outranked by the `pinMain` command, outranks nothing — see [0008](../decisions/0008-compile-root.md) |
| `compile.when` | `"onType"` | `onType` \| `onSave` \| `never` |
| `compile.debounce` | `150` | ms after the last keystroke before compiling |
| `diagnostics.enabled` | `true` | |
| `semanticTokens` | `"enable"` | `enable` \| `disable` |
| `formatter.mode` | `"typstyle"` | `typstyle` \| `off` |
| `formatter.printWidth` | `80` | |
| `formatter.indentSize` | `2` | |
| `inlayHints.enabled` | `false` | Parameter-name hints (Phase 4) |
| `fonts.system` | `true` | Index installed system fonts |
| `fonts.paths` | `[]` | Extra font directories, workspace-relative or absolute |
| `packages.enabled` | `true` | Allow downloading from the registry |
| `packages.registry` | `"https://packages.typst.org"` | |
| `packages.cachePath` | `""` | Empty = typst's standard cache dir, shared with `typst-cli` |
| `preview.scrollSync` | `"both"` | `both` \| `editorToPreview` \| `previewToEditor` \| `off` |
| `preview.cursorIndicator` | `true` | Show a marker at the cursor's page position |
| `preview.invertColors` | `"never"` | `never` \| `always` \| `auto` (follow the VSCode theme) |
| `preview.background` | `"editor"` | `editor` \| `white` \| `gray` |
| `preview.renderMode` | `"svg"` | `svg` \| `png` \| `auto`. PNG is a low-memory mode that loses zoom fidelity and find-in-preview; `auto` switches per page above ~1 MB of SVG. Phase 4 — see [0006](../decisions/0006-preview-rendering.md) |
| `export.outputPath` | `"$dir/$name"` | Supports `$dir`, `$name`, `$root` |
| `memory.evictAge` | `1` | comemo cache age. Lower is both faster and smaller — measured, counter-intuitive, and explained in [0005](../decisions/0005-cache-eviction-policy.md). Deviates from typst-cli's `10` on purpose |
| `memory.restartThresholdMb` | `1024` | Offer a server restart above this heap; `0` disables |
| `trace.server` | `"off"` | `off` \| `messages` \| `verbose` |

Changing `fonts.*`, `packages.*`, `rootPath`, or `mainFile` restarts the session; everything else is
applied through `workspace/didChangeConfiguration` and triggers at most a recompile.

### Compile-root resolution

`mainFile` is one of three inputs, not the whole story. The entry file is the first of:

1. **Session pin** — `typstUltra.pinMain`, stored in `workspaceState`
2. **`typstUltra.mainFile`** — workspace or folder setting, checked in by a team
3. **The focused `.typ` editor** — the zero-configuration default

A status-bar item shows which mode is active (`$(eye) chapter-03.typ` following, `$(pin) main.typ` pinned)
and opens a QuickPick to switch. Full behaviour, including the one-shot "pin `main.typ`?" suggestion and
what happens when the focused file is outside the pinned project, is in
[0008](../decisions/0008-compile-root.md).

---

## 8. BibTeX bibliographies (Phase 6)

A `.bib` file is part of a typst project — `bibliography("refs.bib")` reads it and `@knuth1984` cites into
it — so it is part of the server. The *why*, including why neither `biblatex` nor `hayagriva` could be the
parser, is [0011](../decisions/0011-bibtex-support.md). This is the surface.

### 8.1 Routing

There is no second server and no second dispatch table. Every handler asks `Server::bib_of(uri)` first;
`Some` means the document is a `.bib` and the BibTeX path answers it. The document overlay, the URI map,
the token cache, and `typst/*` are untouched.

| In a `.bib` file | What it answers |
| --- | --- |
| `publishDiagnostics` | Syntax errors, duplicate keys and fields (error); missing required fields, unknown entry types (warning) |
| `documentSymbol` | One symbol per entry, keyed by citation key, fields nested underneath |
| `workspace/symbol` | Citation keys, searched with the same fuzzy filter as typst symbols |
| `hover` | The entry as a reference; what a field means; a `crossref` target; a `@string` expansion |
| `completion` | Entry types as fill-in skeletons, field names (required ones first), `crossref` keys, `@string` names |
| `definition` | `crossref` → the entry it names, in this file or another; a bare value → its `@string` |
| `documentLink` | `url` fields, and `doi` fields behind `https://doi.org/` |
| `foldingRange` | One region per entry |
| `selectionRange` | value → field → entry → file |
| `semanticTokens/full`, `/full/delta` | From the BibTeX parse: entry type, key, field name, value, number, `@string` reference, punctuation, and the ignored text between entries |
| `formatting` | The canonical layout — one field per line, trailing commas, one blank line between entries. Refuses on a file with syntax errors, as `typstyle` does |
| `rangeFormatting`, `codeAction`, `codeLens`, `inlayHint`, `signatureHelp` | Nothing. These are typst notions |

| In a `.typ` file | What changes |
| --- | --- |
| `hover` on `@key` | Falls back to the bibliography when `typst-ide` has no tooltip |
| `definition` on `@key` | Falls back to the entry's key range in the `.bib` file |
| `completion` after `@` | Appends citation keys upstream did not offer, sorted after everything it did |
| `references`, `rename` on `@key` | Span both languages: one rename rewrites the entry and every citation |

### 8.2 Two clocks, again

[architecture.md §5](architecture.md#5-concurrency-model-one-thread-two-clocks) has the syntax tree and the
compiled document on separate clocks. Bibliography diagnostics are a third: they come straight off the edit, because parsing is
instant and a `.bib` file that no `bibliography()` call names would otherwise never be checked at all.

The consequence is in the publishing. Both publishers clear by difference — "these URIs had diagnostics
last time and do not now" — so they keep separate sets (`published`, `bib_published`) and the compile skips
`.bib` files entirely. Without that, each would clear the other's squiggles on its way past, and which one
you saw would depend on your typing speed.

### 8.3 What the compiler already did

`Vfs::file` prefers the open-document overlay, so an unsaved edit to a `.bib` already reached the compiler
before any of this existed: the bibliography in the preview is the one in the editor, not the one on disk.
`an_unsaved_bibliography_edit_reaches_the_compiler` pins it.

What did *not* work, and now does: a `.bib` can no longer become the compile root — not by being focused,
not by `typst/setMain` — so editing one recompiles the document that cites it.
