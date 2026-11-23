# RON Ultra

Language support for [RON (Rusty Object Notation)](https://github.com/ron-rs/ron):
syntax highlighting, diagnostics, a comment-preserving formatter, a nested
outline, hovers that tell you where a value sits, and smart select. Name the
Rust type a file deserializes into, and it is also **checked against that type**,
with its fields and variants offered as completions and one click away from
their declarations. The analysis comes from a hand-written RON parser and a Rust
declaration reader, compiled to WebAssembly and run as a language server inside
VS Code. There's nothing to install, and you don't need rust-analyzer.

```ron
/* @[crate::config::GameConfig] */
#![enable(implicit_some)]

GameConfig( // the root struct
    window_size: (800, 600),
    window_title: "PAC-MAN",
    fullscreen: false,

    key_bindings: {
        "up": Up,
        "down": Down,
    },
    difficulty_options: (
        start_difficulty: Easy,
        adaptive: false,
    ),
    motd: r#"He said "hi""#,
    save_slot: None,
)
```

## Files it applies to

The `ron` language is bound to files ending in `.ron`. To use it for anything
else, run **Change Language Mode** or add a `files.associations` entry.

## Features

- **Syntax highlighting.** Struct and variant names, field names, `Some` and
  `None`, `true` and `false` each get their own scope. So do every number form
  (decimal, `0x`, `0o` and `0b` integers, underscores, type suffixes such as
  `255u8` and `1.0f32`, `inf` and `NaN`) and strings: escapes are picked out,
  invalid escapes are flagged, and raw strings (`r#"..."#`), byte strings
  (`b"..."`) and char literals are covered too. Block comments nest, as they do
  in RON. In an `#![enable(...)]` attribute the four known extensions are
  highlighted, and any other name is marked invalid.

- **Diagnostics** for the syntax problems listed below, and for mismatches with
  the annotated Rust type. They are computed when a RON file is opened and
  again as you type.

- **Formatting** with **Format Document**, and therefore with format-on-save.
  Every comment is kept, scalars are reproduced exactly as written, and only
  layout changes. See [Formatting](#formatting).

- **Outline and breadcrumbs.** Struct fields and map entries are listed by name,
  nested as deep as the document goes. A named root such as `GameConfig(...)`
  is the single top-level symbol. List and tuple items appear as `[0]`, `[1]`,
  … only when they hold structure of their own, so a list of a thousand numbers
  does not flood the outline. Go to Symbol in File, the Outline view and
  breadcrumbs all work.

- **Hover.** Hovering a field name, a map key, a scalar, a struct name or a
  bracket shows the value's path from the root and what it is. For example,
  `` `difficulty_options.start_difficulty` · unit struct or enum variant `Easy` ``
  or `` `key_bindings` · map · 2 entries ``. Hovering an extension inside
  `#![enable(...)]` explains what it does. In an annotated file the hover
  also shows the Rust field, struct or variant behind what you are pointing at,
  with its type and doc comment.

- **Smart select.** **Expand Selection** (<kbd>Shift</kbd>+<kbd>Alt</kbd>+<kbd>→</kbd>
  on Windows and Linux, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Cmd</kbd>+<kbd>→</kbd>
  on macOS) grows from the token under the cursor to the value, to its whole
  `field: value` entry, to the enclosing container, and so on out to the root.

- **Folding** of multi-line structs, tuples, lists and maps (the closing bracket
  stays visible), runs of two or more `//` comment lines, multi-line block
  comments and multi-line strings.

- **Checking against your Rust types.** A comment such as
  `/* @[crate::config::GameConfig] */` ties the file to a Rust struct or enum.
  The file is then checked the way `ron` and `serde` would read it, and you get
  completion, richer hovers and go to definition. See
  [Checking against your Rust types](#checking-against-your-rust-types).

- **Editing niceties.** `//` and `/* */` comment toggling, auto-closing and
  surrounding pairs for brackets and quotes, and a word pattern that treats
  `r#type` and `-1.5` as single words.

## Checking against your Rust types

Put a comment naming the type anywhere before the top-level value. It can be a
block or line comment, and it can go before or after `#![enable(...)]`:

```ron
/* @[crate::config::GameConfig] */
GameConfig(...)
```

```ron
// schema: @[Vec<crate::level::Level>]
[...]
```

What sits between `@[` and `]` is a Rust type. A path is resolved the way
`rustc` would resolve it from the crate root:

- `crate` is the package whose `Cargo.toml` is nearest above the RON file. Its
  library (`src/lib.rs`, or `[lib] path`) is tried first, then `src/main.rs`.
- Modules are followed into `foo.rs` or `foo/mod.rs`, into inline
  `mod foo { ... }` blocks, and through `#[path = "..."]`.
- Names imported with `use`, including `pub use` re-exports, globs and
  `as` renames, resolve to the item they import.
- Other crates in your workspace can be named too (`shared::Color`) when they
  are path dependencies, or workspace dependencies with a path.

Rust files are read from disk as needed and re-read when they change. Save a
Rust file and every open RON file that depends on it is checked again.

### What is checked

| Code | Severity | Reported for |
| --- | --- | --- |
| `RON012` | Warning | An annotation that names nothing the extension can find, or a file with no `Cargo.toml` above it |
| `RON013` | Error | A value of the wrong kind: a string where the field is a `u32`, a list where it is a struct, `5` for an `Option<u8>` without `implicit_some` |
| `RON014` | Warning | A field the struct does not declare, with a "did you mean" hint. It's an error under `#[serde(deny_unknown_fields)]` |
| `RON015` | Error | Required fields left out (fields that are not `Option` and not `#[serde(default)]`) |
| `RON016` | Error | A name that is not a variant of the enum |
| `RON017` | Error | A tuple, tuple struct, tuple variant or `[T; N]` array with the wrong number of values |
| `RON018` | Error | A struct written with another struct's name, or without its name under `explicit_struct_names` |
| `RON019` | Error | An integer that does not fit its type, such as `300` for a `u8` or `-1` for a `u32` |

These follow serde's derive attributes: `rename`, `alias`, `rename_all`,
`rename_all_fields`, `default`, `skip` / `skip_deserializing`, `flatten`,
`transparent`, `from` / `try_from`, `deny_unknown_fields`, `other`, and the
`untagged`, internally tagged (`tag`) and adjacently tagged (`tag` + `content`)
enum representations. Attributes inside `#[cfg_attr(..., serde(...))]` count.
The file's `#![enable(...)]` extensions are honored too: `implicit_some`,
`unwrap_newtypes`, `unwrap_variant_newtypes` and `explicit_struct_names`.

Generic structs and type aliases are followed (`@[Wrapper<crate::Item>]` works).
Standard containers are understood by name: `Vec`, `HashMap`, `BTreeMap`,
`HashSet`, `Option`, `Box`, `Arc`, `Cow`, `PathBuf` and their friends, plus
common crate types such as `IndexMap`, `SmallVec` and `Uuid`.

When the extension can't see into a type, it accepts any value there. That
covers types from crates outside your workspace, `serde_json::Value`, fields
with `#[serde(with = "...")]`, and anything generated by a macro.

### Completion, hover and go to definition

- **Inside `@[...]`**, completion offers `crate`, your workspace dependencies,
  modules and types, one path segment at a time.
- **Where a field name goes**, completion offers the fields not written yet,
  with their types and doc comments. Required fields come first. Picking one
  inserts `field: ` and opens completion again for its value.
- **Where a value goes**, completion offers what the type accepts: enum
  variants, `Some(…)` and `None`, `true` and `false`, or a struct skeleton with
  every required field filled in as a tab stop.
- **Hover** a field, a struct name or a variant to see its Rust declaration and
  documentation, or a value to see the type it must have.
- **Go to Definition** (<kbd>F12</kbd>, or <kbd>Ctrl</kbd>/<kbd>Cmd</kbd>+click)
  on a field, a struct or variant name, or the annotation itself jumps to the
  declaration in the Rust source.

### Limits

The extension reads declarations; it doesn't compile your crate. These cases
are out of reach:

- Items produced by macros, and `#[cfg]`, which is not evaluated. Every
  declaration counts.
- Edits to a Rust file that has not been saved. Changes are picked up when the
  file is written to disk.
- Visibility. A private struct can be named, since only its shape matters.

## Diagnostics

| Code | Severity | Reported for |
| --- | --- | --- |
| `RON001` | Error | A syntax error: an unexpected token, a missing `:` or `,`, a missing value, an unclosed bracket |
| `RON002` | Error | A string or raw string that is never closed |
| `RON003` | Error | A block comment that is never closed |
| `RON004` | Error | A malformed number, such as `0b102`, `1e` or `1.5u8` |
| `RON005` | Error | An escape RON does not have, such as `\q`, or a `\x` above `7F` outside a byte string |
| `RON006` | Error | A character literal that is empty, unterminated or holds more than one character |
| `RON007` | Error | Anything after the top-level value, since a RON file holds exactly one |
| `RON008` | Error | Named fields and positional values mixed in one `( ... )` |
| `RON009` | Error | The same field twice in one struct |
| `RON010` | Warning | The same key twice in one map (when deserialized, the last one wins) |
| `RON011` | Error | An unknown extension in `#![enable(...)]` |

The codes for checking against Rust types, `RON012` to `RON019`, are listed
under [What is checked](#what-is-checked).

Duplicate string keys are compared by their decoded text, so `"a"` and `r"a"`
count as the same key. A file that is empty or holds only comments gets no
diagnostic, because that is what every new file looks like.

## Formatting

Running **Format Document** on a RON file does the following:

- Puts a container on one line, as in `(x: 1, y: 2)` or `[1, 2, 3]`, when it
  holds no comments, fits within `ronUltra.format.printWidth`, and you did not
  break the line right after its opening bracket.
- Otherwise writes it one entry per line with a comma after every entry,
  including the last.
- Indents with the editor's settings for the file (`editor.tabSize` and
  `editor.insertSpaces`).
- Normalizes spacing to `field: value`, `a, b` and `Name(...)`, with no space
  before the parenthesis.
- Collapses runs of blank lines between entries to one, and removes blank lines
  just inside brackets and at the start and end of the file.
- Ends the file with a single newline, following `files.insertFinalNewline`.
- Keeps the file's line endings, so a CRLF file stays CRLF.

The newline after an opening bracket is how you choose a layout. Put one there
and the container stays expanded however short it is. Take it out and the
formatter collapses the container again whenever it fits:

```ron
// Stays expanded:          // Collapses:
Point(                      Point(x: 1,
    x: 1,                       y: 2)
    y: 2,
)                           // → Point(x: 1, y: 2)
```

Some things are never changed:

- **Scalars**, byte for byte: `0xFF_u8` stays `0xFF_u8` and `1_000` stays
  `1_000`, and every escape and raw string stays as written.
- **Order**: fields and keys are never reordered.
- **Comments**: each one stays with the entry it annotates, whether it sits on
  the line above, at the end of the line, or between a field name and its value.
- **Attributes**: `#![enable(...)]` lines are kept verbatim, one per line.

If the file has a syntax-level error (`RON001` to `RON008`), formatting is
refused outright and VS Code reports that there are no formatting edits.
Format-on-save often fires while you are still typing, and rewriting a
half-typed document around a hole would do more harm than good. Duplicate
fields or keys, unknown extensions and type mismatches do not block formatting.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `ronUltra.diagnostics.enabled` | `true` | Report the diagnostics listed above |
| `ronUltra.schema.enabled` | `true` | Check files that name their Rust type in an `@[...]` comment, and offer that type's fields and variants as completions |
| `ronUltra.format.enabled` | `true` | Enable the formatter. When off, no formatting edits are produced |
| `ronUltra.format.printWidth` | `80` | Widest line a container may be collapsed onto. `0` expands every non-empty container |

## Commands

| Command | What it does |
| --- | --- |
| **RON Ultra: Restart Language Server** | Restart the server, forgetting every Rust file it has read |
