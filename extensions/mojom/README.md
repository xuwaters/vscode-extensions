# Mojom IDL Ultra

Mojom is Chromium's interface definition language: `.mojom` files declare the
structs, enums, unions and interfaces that get code-generated into the C++,
Java, TypeScript and Rust bindings used for inter-process communication. This
extension gives those files syntax highlighting, an outline, hover, go-to-
definition, completion and diagnostics, all from a Rust analyzer compiled to
WebAssembly and running in-process. No language server, no toolchain to
install.

## Features

- **Syntax highlighting** for `.mojom`: `//` and `/* */` comments; `module`
  declarations, with the dotted namespace name scoped separately from the
  keyword; `import "…";` paths; `const` declarations; `struct`, `union`,
  `enum` and `interface` declarations (including the forward-declaration forms
  `struct Foo;` and `enum Bar;`); attributes in `[Brackets]`, where every
  identifier inside — the `Sync` in `[Sync]`, and both halves of
  `[EnableIf=is_win]` — is scoped as an attribute name, with string and
  numeric attribute values getting their own scopes; method declarations, with
  the method name, its `@3` ordinal, the `=>` response arrow, parameter types
  and parameter names each distinguished; ordinals on struct fields and union
  fields; the primitive types `bool`, `int8`–`uint64`, `float`, `double` and
  `string`; the composite and handle-ish types `array`, `map`, `associated`,
  `pending_remote`, `pending_receiver`, `pending_associated_remote`,
  `pending_associated_receiver`, `handle`, `message_pipe`, `shared_buffer`,
  `data_pipe_consumer`, `data_pipe_producer` and `platform`, with types nested
  inside `<…>` highlighted recursively; user-defined and module-qualified type
  names; enum members, struct and union members; hex, float and decimal
  literals; and the `true` / `false` / `default` constants.

- **Diagnostics** in five flavours. Parse errors (`MOJOM0001`) and duplicate
  ordinals within one scope (`MOJOM0010`) are reported as errors — the ordinal
  check runs separately over struct fields, union fields, interface methods,
  a method's request parameters, and its response parameters, so `@1` may
  legitimately appear once in each. Three warnings need the rest of the
  workspace: an `import "…";` that resolves to no known file (`MOJOM0020`), a
  type reference that matches nothing anywhere (`MOJOM0030`), and a type that
  does exist but lives in a file this one never imports (`MOJOM0031`, whose
  message names the defining file). They refresh when a file is opened, on
  every save, and — unless you turn `mojom.diagnostics.onType` off — 150 ms
  after you stop typing.

- **Hover** returns a fenced `mojom` block with the symbol's kind, fully
  qualified name and signature: a field shows its type label, a method shows
  `(params) => (response)`. `//` comment lines immediately above a declaration
  are picked up as its documentation and appended. Hovering a type that is
  defined but not imported here adds a note telling you to add the `import`;
  hovering a type that resolves to nothing lists the fully qualified names
  that were tried, which is usually enough to spot the wrong module prefix.

- **Go to definition** on a type reference jumps to its declaration, whether
  it is in the same file or reached through an import and written
  module-qualified as `other.module.Foo`. Do it on the string inside
  `import "…";` and you jump to the imported file itself.

- **Completion**, offered as you type and re-triggered on `.` and `<`. Just
  after a `(`, `,` or `<` you are in type position, so you get the 20 builtin
  type names plus every struct, union, interface and enum visible from the
  current file — each labelled with its fully qualified name, and inserted
  bare when it lives in this file's own module or module-qualified when it
  does not. At the start of a file or after a `;` you get the seven
  declaration keywords `module`, `import`, `struct`, `union`, `interface`,
  `enum` and `const`. Anywhere ambiguous, both sets.

- **Outline, breadcrumbs and folding**. The `module` line and every top-level
  declaration become document symbols, with fields, methods, constants and
  nested enums as children, and enum values under their enum. Fields and
  methods that carry an ordinal are shown as `name @3`. Folding ranges come
  from the same parse, covering struct, union, interface and enum bodies
  including enums nested inside a struct or interface.

- **Workspace symbols**. Press Ctrl+T / Cmd+T and search across every loaded
  `.mojom` file by fully qualified name; the match is a case-insensitive
  substring of the FQN, and an empty query lists everything.

- **The whole workspace is indexed up front.** On activation the extension
  scans for `**/*.mojom` — skipping `node_modules`, `target`, `dist`, `build`
  and `out`, up to 5000 files — and feeds them to the analyzer, so cross-file
  diagnostics, definitions and completion work without you having opened the
  other files first. Files already open in the editor keep their unsaved
  contents.

- **Editing niceties** from the language configuration: `//` and `/* */`
  comment toggling, bracket matching and auto-closing for `{}`, `[]`, `()` and
  `"` (quotes are skipped inside strings and comments), `<>` available as a
  surrounding pair, indent after a trailing `{`, dedent on a leading `}`, and
  `// region` / `// endregion` folding markers.

```mojom
module chrome.mojom;

import "url/mojom/url.mojom";

// Details of a single download.
struct DownloadItem {
  string id@0;
  url.mojom.Url source@1;
  array<uint8> checksum@2;
  map<string, string> headers@3;
};

enum DownloadState {
  kPending,
  kActive,
  kDone,
};

interface DownloadObserver {
  OnStateChanged(DownloadState state);
};

[Sync]
interface DownloadManager {
  // Starts a download and reports the item it created.
  Start@0(DownloadItem item) => (bool accepted);

  [EnableIf=is_win]
  Cancel@1(string id);

  AddObserver(pending_remote<DownloadObserver> observer);
  BindReceiver(pending_receiver<DownloadManager> receiver);
};
```

## Resolving imports

Mojom import paths are conventionally written relative to a source root, not
to the importing file, so the analyzer tries several strategies in order: each
configured include path joined with the import path; then, for paths starting
`./` or `../`, the importing file's own directory; then any loaded file whose
path ends with the import path, preferring the one sharing the most directory
segments with the importer and breaking ties toward the shallower path. That
last rule is what keeps `import "dep.mojom"` pointing at the sibling next to
you rather than a vendored copy buried elsewhere in the tree.

Every workspace root is searched automatically. Set `mojom.includePaths` only
when your source roots live outside the workspace, or when a root deeper in
the tree — Chromium's `src/`, say — is what your import paths are written
relative to.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `mojom.includePaths` | `[]` | Additional source roots for resolving `import "…";` paths. Workspace-relative or absolute. Workspace roots are always searched in addition to this list. |
| `mojom.diagnostics.enabled` | `true` | Emit diagnostics for parse errors, duplicate ordinals, unresolved imports, and unknown type references. |
| `mojom.diagnostics.onType` | `true` | Recompute diagnostics as you type (debounced). Turn off to only recompute on save. |

Changing `mojom.includePaths`, or adding and removing workspace folders,
re-resolves imports and re-runs diagnostics on every open file straight away.

## Commands

- **Mojom: Restart Analyzer** (`mojom.restart`) — clear all diagnostics, drop
  the open documents from the analyzer, re-read the include paths, re-scan the
  workspace for `.mojom` files, and re-diagnose. Useful after files change on
  disk outside the editor.

## Note

Language features are registered for files on disk, so an unsaved untitled
buffer gets syntax highlighting but not diagnostics, hover, definitions or
completion until it is saved with a `.mojom` extension.
