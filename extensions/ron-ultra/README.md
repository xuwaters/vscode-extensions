# RON Ultra

Language support for [RON (Rusty Object Notation)](https://github.com/ron-rs/ron):
syntax highlighting, diagnostics, a comment-preserving formatter, a nested
outline, hovers that tell you where a value sits, and smart select. The analysis
comes from a hand-written RON parser in Rust, compiled to WebAssembly and run
in-process. There's no language server and nothing to install.

```ron
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

- **Diagnostics** for eleven problems, listed below. They are computed when a
  RON file is opened and again as you type, debounced by 150 ms.

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
  `#![enable(...)]` explains what it does.

- **Smart select.** **Expand Selection** (<kbd>Shift</kbd>+<kbd>Alt</kbd>+<kbd>→</kbd>
  on Windows and Linux, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Cmd</kbd>+<kbd>→</kbd>
  on macOS) grows from the token under the cursor to the value, to its whole
  `field: value` entry, to the enclosing container, and so on out to the root.

- **Folding** of multi-line structs, tuples, lists and maps (the closing bracket
  stays visible), runs of two or more `//` comment lines, multi-line block
  comments and multi-line strings.

- **Editing niceties.** `//` and `/* */` comment toggling, auto-closing and
  surrounding pairs for brackets and quotes, and a word pattern that treats
  `r#type` and `-1.5` as single words.

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
fields or keys and unknown extensions do not block formatting.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `ronUltra.diagnostics.enabled` | `true` | Report the diagnostics listed above |
| `ronUltra.format.enabled` | `true` | Enable the formatter. When off, no formatting edits are produced |
| `ronUltra.format.printWidth` | `80` | Widest line a container may be collapsed onto. `0` expands every non-empty container |
