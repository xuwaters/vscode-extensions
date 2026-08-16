# Protocol Buffers (proto3)

Protocol Buffers support for VS Code: syntax highlighting for `.proto` and for
protobuf text-format files, plus real language features — diagnostics, outline,
go to definition, references, rename, completion, hover, inlay hints and
formatting. All of it comes from a Rust analyzer compiled to WebAssembly and
running in the extension host. No language server, no `protoc`, no `buf`, no
toolchain to install.

## Features

- **Syntax highlighting** for `.proto`. `syntax`, `package`, `import`
  (including the `public` and `weak` modifiers) and `option`; `message`,
  `enum`, `service`, `rpc` (with `stream` and `returns`) and `oneof`, each with
  its declared name scoped as a type or function; the field modifiers
  `repeated`, `optional` and `required`; `map<K, V>`; `reserved` and
  `extensions` ranges with their `to` / `max` keywords, and `extend` blocks.
  Field numbers get their own scope, separate from ordinary numbers, so you can
  colour them distinctly. All fifteen scalar types are recognised, as are
  dotted and leading-dot type references — the namespace segments and the type
  segment are scoped separately. Option names are highlighted both in `option`
  statements and in `[…]` field options, with custom options in parentheses
  (`[(my.pkg.custom) = true]`) picked out as well. Line and block comments,
  single- and double-quoted strings with hex, octal, unicode and simple escapes
  (invalid escapes marked as such), decimal, hex, octal and float literals, and
  the `true` / `false` / `inf` / `nan` constants round it out. Both proto2 and
  proto3 spellings are covered — `required`, `optional`, `group`, `extend` and
  `extensions` are all in the grammar.

- **Diagnostics** as you type (150 ms debounce) and on open and save. The
  analyzer reports lexical and parse errors (unterminated strings and comments,
  invalid escapes, unexpected tokens), unresolved, unused and circular imports,
  unknown types and non-type references, duplicate field numbers, field numbers
  out of range or inside the reserved 19000–19999 block, uses of reserved
  numbers and reserved names, duplicate declaration names, duplicate enum
  values, the proto3 rule that the first enum value must be zero, `oneof`
  members that are `repeated` or `map` or otherwise invalid, invalid map key
  types, and `required` in a proto3 file. Every
  message carries its code, so `PROTO0030: …` tells you which check fired.

- **Naming-convention warnings**, off by default. Turning
  `proto3.diagnostics.style` on adds UpperCamelCase, lower_snake_case and
  SCREAMING_SNAKE_CASE checks plus an empty-message warning (`PROTO0070` and
  up). They ship off so they do not duplicate what `buf lint` already tells
  you.

- **Cross-file navigation.** On activation the extension scans the workspace
  for `.proto` files (up to 2000, skipping `node_modules`, `.git` and whatever
  your root `.gitignore` excludes) and feeds them to the analyzer, so Go to
  Definition, Find All References and Go to Symbol in Workspace work across
  files you have not opened. Rename updates every use site in the workspace.

- **Outline, breadcrumbs and folding.** Messages, enums and their values,
  fields, `oneof`s, services and methods are published as nested
  document symbols. Folding ranges come from the parser rather than from
  indentation, and comment blocks fold as comments.

- **Hover and inlay hints.** Hovering a type reference shows its
  fully-qualified name, kind and doc comment; hovering a field shows
  `label type name = number` plus its doc comment. Unresolved references show
  the candidate names that were tried. Inlay hints append the resolved FQN
  after short type references that come from another file, so you can see
  `Timestamp: .google.protobuf.Timestamp` without jumping away.

- **Completion** of keywords and scalar types, plus every message, enum, enum
  value, field, service and method visible from the file you are editing,
  labelled by kind.

- **Quick fixes.** When a type resolves to a file you have not imported, an
  **Add import "…"** action inserts the import in the right place. **Organize
  imports** sorts them (plain, then `public`, then `weak`, alphabetical within
  each) and drops duplicates; it is offered only when the order is not already
  canonical.

- **Formatting** with a built-in pretty-printer: two-space indentation, one
  declaration per line, blank lines between top-level items, leading doc
  comments preserved. It is opinionated and does not preserve manual column
  alignment. If the file has parse errors the formatter declines rather than
  produce garbled output, so you see the error instead.

- **Semantic highlighting** on top of the grammar, marking resolved messages
  and enums as types, enum values, field names and package segments — so
  colours follow what the analyzer actually resolved, not just what the regex
  matched.

- **Well-known types are bundled.** `any.proto`, `api.proto`,
  `descriptor.proto`, `duration.proto`, `empty.proto`, `field_mask.proto`,
  `source_context.proto`, `struct.proto`, `timestamp.proto`, `type.proto` and
  `wrappers.proto` are compiled into the analyzer, so
  `import "google/protobuf/timestamp.proto";` resolves — and Go to Definition
  on `Timestamp` works — without any include path configuration.

- **Editing niceties** from the language configuration: `//` and `/* */`
  comment toggling, bracket matching and auto-closing for `{}`, `[]`, `()` and
  quotes, indent on `{`, dedent on `}`, and `// region` / `// endregion`
  folding markers.

```proto
syntax = "proto3";

package example.v1;

import "google/protobuf/timestamp.proto";

option java_package = "com.example.v1";

// An order placed by a customer.
message Order {
  reserved 4, 7 to 9;
  reserved "legacy_total";

  string id = 1;
  repeated LineItem items = 2;
  map<string, string> labels = 3 [deprecated = true];
  google.protobuf.Timestamp created_at = 5;

  oneof payment {
    Card card = 10;
    Invoice invoice = 11;
  }

  enum Status {
    STATUS_UNSPECIFIED = 0;
    STATUS_SHIPPED = 1;
  }
}

service Orders {
  rpc Get(GetOrderRequest) returns (Order);
  rpc Watch(WatchRequest) returns (stream Order);
}
```

## Text format files

`.txtpb`, `.textproto`, `.textpb`, `.pbtxt` and `.prototext` are registered as
a separate language with their own grammar: `#` comments, field names before
`:` and before `{` or `<`, nested message blocks in both brace and angle form,
repeated-value lists, extension field names and `google.protobuf.Any` type URLs
in `[…]`, string escapes, numbers (including the `f` suffix and signed forms)
and the text-format spellings of booleans and floats (`t`, `f`, `True`,
`infinity`, `nan`).

Text-format files get parse diagnostics, an outline and folding on their own.
Schema-aware checks switch on when the file carries the conventional header
annotations:

```textproto
# proto-file: example/v1/order.proto
# proto-message: example.v1.Order

id: "ord-1"
labels: [{ key: "tier" value: "gold" }]
```

With a `# proto-message:` header the analyzer resolves the message in your
workspace and flags unknown fields, value kinds that do not match the declared
field type, unknown enum values, singular fields set twice and conflicting
`oneof` arms; a `# proto-file:` header additionally scopes the lookup so a
wrong-file mistake is caught. Hover, Go to Definition and field-name completion
work in text-format files under the same conditions. Without the header,
structure features still work and the schema-aware ones stay quiet. Unknown
`# proto-*:` keys and duplicated headers are reported.

Find All References, rename, formatting, inlay hints, quick fixes and semantic
tokens apply to `.proto` files only.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `proto3.includePaths` | `[]` | Additional `-I` include paths for `import "…"` resolution. Absolute, or relative to the first workspace folder. Workspace folder roots are always searched. |
| `proto3.diagnostics.onType` | `true` | Recompute diagnostics while you type, debounced by 150 ms. Turn off to recompute only on open and save. |
| `proto3.diagnostics.style` | `off` | Set to `on` to emit the naming-convention warnings (`PROTO0070`+). |
| `proto3.diagnostics.enabled` | `true` | Declared, but not read by this version — diagnostics are always produced. |
| `proto3.inlayHints.enabled` | `true` | Declared, but not read by this version — inlay hints follow the editor's own `editor.inlayHints.enabled` setting. |
| `proto3.trace.server` | `off` | Declared, but not read by this version — no trace output is written. |

Changing `proto3.includePaths` or `proto3.diagnostics.style` re-runs the
analyzer over every open protobuf file immediately.

## Commands

- **Proto3: Restart Analyzer** (`proto3.restart`) — re-read the include paths,
  rescan the workspace for `.proto` files and recompute diagnostics. Useful
  after files change outside the editor or the index looks stale.

- **Proto3: Show Workspace Symbols** (`proto3.showSymbolTree`) — open a
  scratch document listing every indexed symbol as `kind  fully.qualified.name
  (file)`. Handy for checking what the analyzer actually indexed.
