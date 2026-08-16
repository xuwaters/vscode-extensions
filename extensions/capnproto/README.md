# Cap'n Proto Ultra

Cap'n Proto schema support for VS Code: syntax highlighting, an outline, hover,
go-to-definition, completion and diagnostics. The analysis comes from a Rust
lexer, parser and resolver compiled to WebAssembly and running in-process — no
language server, and `capnp` does not need to be installed.

## Features

- **Syntax highlighting** for `.capnp`: `#` comments; the file id `@0x…;` (the
  `@`, the hex constant and the terminating `;` are scoped separately);
  declaration keywords `struct`, `enum`, `interface`, `const`, `annotation`,
  `using`, `import`, `union`, `group` and `extends`; field, enumerant and
  method ordinals `@0`, `@1`, …; annotation uses like `$Cxx.namespace`; the
  built-in types `Void`, `Bool`, `Int8`–`Int64`, `UInt8`–`UInt64`, `Float32`,
  `Float64`, `Text`, `Data`, `List`, `AnyPointer` and `Capability`, with any
  other capitalised identifier treated as a user type; the constants `true`,
  `false`, `void`, `inf` and `nan`; the `stream` keyword and the `->` arrow;
  double-quoted strings with backslash escapes; and hex, integer and float
  literals.

- **Diagnostics** from the analyzer, published under the source `capnp`, each
  with a code you can search for. Errors: parse errors (`CAPNP0001`), a missing
  top-level file id (`CAPNP0002`), and an ordinal used twice in the same scope
  (`CAPNP0010`) — including fields hoisted from unions and groups, which share
  the enclosing struct's ordinal space. Warnings: an `import "…"` that resolves
  to nothing (`CAPNP0020`), a type that resolves nowhere (`CAPNP0030`), and a
  type that does exist in the workspace but in a file this one never imports
  (`CAPNP0031`). They refresh when a file is opened and on every save, and —
  unless you turn `capnp.diagnostics.onType` off — 150 ms after you stop
  typing.

- **Outline and breadcrumbs**: the document symbol tree nests fields, named and
  anonymous unions, groups, enumerants, methods, constants, annotations,
  `using` aliases and nested declarations under their parent. Fields,
  enumerants and methods are labelled with their ordinal (`x @0`), and the
  detail column carries the type — including generic arguments, so a
  `List(Point)` field reads as such. Generic structs show their parameters as
  `struct(T, U)`.

- **Hover**: over a type reference you get the declaration's kind and fully
  qualified name plus its doc comment; over a field name you get
  `name @N :Type` and which struct it belongs to. Built-in types say so, import
  aliases show the file they point at, and an unresolved name lists the
  candidate paths that were tried. If the type exists but its file is not
  imported here, the hover says so and tells you to add a `using`.

- **Go to definition** on a type reference jumps to the declaration, following
  `using X = import "…".Y;` aliases, bare `using import "…".Y;`, and inline
  `:import "…".Y` types. Putting the cursor inside the quoted path of an import
  jumps to that file.

- **Go to symbol in workspace** (`Ctrl+T` / `Cmd+T`) matches your query against
  the fully qualified name of every declaration in every loaded schema, so
  `Canvas.draw` finds the method directly.

- **Completion** is context-sensitive. After a `:`, inside `List(` or after a
  comma you get the seventeen built-in types plus every type visible from the
  current file, each labelled with its FQN and inserted using the shortest form
  that works from where you are. At declaration position you get the schema
  keywords. `.` and `:` trigger it, and the item kinds mean structs, enums and
  interfaces are distinguishable in the list.

- **Folding** for `struct`, `enum` and `interface` bodies and their nested
  members, plus `# region` / `# endregion` markers.

- **Editing niceties** from the language configuration: `#` comment toggling,
  bracket matching and auto-closing for `{}`, `[]`, `()` and `"` (quotes are
  skipped inside strings and comments), indent after a trailing `{`, and dedent
  on `}`.

The whole workspace is indexed up front: on activation the extension scans for
`**/*.capnp` (skipping `node_modules`, `target`, `dist` and `build`, up to 2000
files) and feeds them to the analyzer, so cross-file hover, go-to-definition and
symbol search work without opening every file first.

```capnp
@0xbf5147cbbecf40c1;

using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("myproject::schema");

struct Point {
  # A location in the plane.
  x @0 :Float64;
  y @1 :Float64;
}

enum Shape {
  circle @0;
  square @1;
}

interface Canvas {
  draw @0 (shape :Shape, at :Point) -> (ok :Bool);
  history @1 () -> (points :List(Point));
}
```

## Imports

Every workspace root is searched for imports automatically, so
`using X = import "sibling.capnp";` resolves with no configuration. Relative
paths are also tried against the importing file's own directory. For schemas
that live outside the workspace — the `/capnp/c++.capnp` above, or a vendored
schema directory — add the containing directory to `capnp.includePaths`;
relative entries there are resolved against the first workspace root. When
several loaded files could match an import path, the one nearest the importer
wins, so a vendored copy buried deep in the tree loses to a sibling.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `capnp.includePaths` | `[]` | Additional include paths for `using X = import "…"` resolution. Workspace-relative or absolute. Workspace roots are always searched in addition to this list. |
| `capnp.diagnostics.enabled` | `true` | Emit diagnostics for parse errors, missing file ids, and duplicate ordinals. |
| `capnp.diagnostics.onType` | `true` | Recompute diagnostics as you type (debounced). Turn off to only recompute on save. |

Changing `capnp.includePaths` re-resolves imports and refreshes every open
schema immediately.

## Commands

- **Cap'n Proto: Restart Analyzer** (`capnp.restart`) — clear all diagnostics,
  drop the analyzer's copy of the open schemas, re-read the include paths,
  rescan the workspace and recompute. Useful after files change outside the
  editor.
