# dotenv (.env)

Language support for `.env` files: syntax highlighting, diagnostics,
formatting, an outline, hovers, and completion of the keys defined in the file
you are editing. The analysis comes from a hand-written `.env` parser written
in Rust and compiled to WebAssembly, running in-process — no language server,
nothing to install.

```sh
# Database
POSTGRES_HOST=localhost
POSTGRES_PORT=5432
DATABASE_URL="postgres://${POSTGRES_HOST}:${POSTGRES_PORT}/app"

export API_TOKEN=abc123        # inline comment
LITERAL='no $interpolation here'
MOTD="first line
second line"
```

## Files it applies to

The `dotenv` language is bound to files named `.env`, `.env.local`,
`.env.development`, `.env.development.local`, `.env.production`,
`.env.production.local`, `.env.test`, `.env.test.local`, `.env.staging`,
`.env.example`, `.env.sample`, `.env.template` and `.env.defaults`, to anything
matching `.env.*`, and to `*.env` — so `.env.ci`, `.env.docker` and
`staging.env` are all picked up. The prefixed forms `*.env.local`,
`*.env.development`, `*.env.development.local`, `*.env.production`,
`*.env.production.local`, `*.env.test`, `*.env.test.local`, `*.env.staging`,
`*.env.example`, `*.env.sample`, `*.env.template` and `*.env.defaults` match
too, and `.env` is registered as a file extension, so `service.env` works as
well. Anything else can be switched over by hand with **Change Language Mode**.

## Features

- **Syntax highlighting.** Full-line comments (`# …`) and inline comments —
  which need a space or tab before the `#`. Assignments get three separate
  scopes: an optional `export` keyword, the key (letters, digits, `_` and `.`,
  starting with a letter or `_`), and the `=`. Values are coloured by form:
  double-quoted strings with `\x` escapes highlighted and `$NAME` / `${NAME}`
  references picked out; single-quoted strings left literal, with no
  interpolation; backtick strings with references picked out; and unquoted
  values, where `$NAME` and `${NAME}` are also recognised. Inside `${…}` the
  shell-style default suffixes (`${NAME:-fallback}`, `:?`, `:+`, `:=`) are
  understood, with the bare identifier scoped separately from the surrounding
  `${}` so a colour theme can pick it out. A quoted value that runs onto the
  following lines keeps its string highlighting through to the closing quote.

- **Diagnostics** for seven problems, listed in the table below. They are
  computed when a `.env` file is opened, on every save, and — unless you turn
  `dotenv.diagnostics.onType` off — while you type, debounced by 150 ms. Each
  message is prefixed with its code, for example "ENV005: duplicate key
  API_KEY".

- **Formatting**, both **Format Document** and **Format Selection**. The
  formatter normalises the shape of the file and never rewrites the bytes of a
  value: quoting style, escapes, inner runs of spaces and multi-line string
  contents all survive verbatim, and keys are never reordered (a value may
  reference a key defined above it, so sorting could change what a loader
  resolves). See below for exactly what it changes.

- **Outline and breadcrumbs.** Every assignment is published as a document
  symbol named after its key, with the value shown as the symbol's detail —
  whitespace collapsed and truncated at 60 characters. Go to Symbol in File,
  the Outline view and breadcrumbs all work.

- **Hover.** Hovering a key shows `KEY = value`. Hovering a `$NAME` or
  `${NAME}` reference resolves it against the same file and shows that key's
  value, or tells you the name is not defined here.

- **Completion** of variable references. Type `$` or `${` inside a value and
  you get the keys defined in the current file, filtered by whatever you have
  typed so far. Completion is deliberately silent at the start of a line —
  there you are naming a new key, not picking an existing one.

- **Folding**, on runs of two or more consecutive comment lines and on quoted
  values that span more than one line, plus `# region` / `# endregion` markers
  from the language configuration.

- **Editing niceties.** `#` line-comment toggling, auto-closing and surrounding
  pairs for `"`, `'`, `` ` `` and `{}`, and a word pattern that treats
  `APP.NAME` as one word for double-click and word-wise motion.

## Diagnostics

| Code | Severity | Reported for |
| --- | --- | --- |
| `ENV001` | Warning | A key that starts with a digit |
| `ENV002` | Warning | A line that is not blank, a comment, or an assignment |
| `ENV003` | Warning | Spaces around `=`, which some dotenv loaders reject |
| `ENV004` | Error | A quote that is opened and never closed |
| `ENV005` | Warning | The same key assigned twice in one file |
| `ENV006` | Information | `$NAME` / `${NAME}` naming a key not defined in this file |
| `ENV007` | Error | An assignment with no key, such as `=value` |

`ENV006` is informational on purpose: a `.env` file may legitimately refer to
variables that come from the surrounding environment rather than from the file
itself.

## Formatting

Running **Format Document** on a `.env` file:

- drops leading indentation on assignments and comments, and trailing
  whitespace everywhere;
- closes up whitespace around `=`, turning `KEY = value` into `KEY=value` —
  the same thing `ENV003` warns about;
- separates `export` from its key by exactly one space;
- separates an inline comment from the value by exactly one space;
- collapses runs of blank lines to `dotenv.format.maxBlankLines`, and removes
  blank lines at the top and bottom of the file entirely;
- ends the file with exactly one newline, unless
  `dotenv.format.insertFinalNewline` is off, in which case the trailing newline
  is stripped;
- keeps the document's line ending — a CRLF file is rewritten with CRLF.

Two things are left exactly as written. A value that begins with `#`
(`KEY=#note`) is ambiguous — loaders disagree about whether it holds the
literal text or nothing at all — so closing up the whitespace around `=` could
flip the reading, and the line is reproduced verbatim. Lines that failed to
parse (`ENV002`) mean nothing to the formatter, so they are reproduced too,
apart from surrounding whitespace.

If the file has any error-severity diagnostic — an unclosed quote or a missing
key — formatting is refused outright rather than guessing, and VS Code reports
that there are no formatting edits. The diagnostic stays visible so you can see
why.

**Format Selection** works the same way, but only on the lines you selected.
Blank lines at the edges of the selection are kept (still capped by
`dotenv.format.maxBlankLines`) since they belong to the surrounding file, and
selecting any line of a multi-line quoted value formats the whole assignment.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `dotenv.diagnostics.enabled` | `true` | Emit diagnostics for malformed assignments, unclosed quotes, duplicate keys, and unknown variable references |
| `dotenv.diagnostics.onType` | `true` | Recompute diagnostics as you type (debounced). Turn off to only recompute on open and save |
| `dotenv.completion.enabled` | `true` | Suggest keys defined in the current file when typing `$` or `${` inside a value |
| `dotenv.format.enabled` | `true` | Enable the formatter. When off, no formatting edits are produced |
| `dotenv.format.maxBlankLines` | `1` | Maximum consecutive blank lines to keep when formatting; `0` removes them all |
| `dotenv.format.insertFinalNewline` | `true` | End the formatted file with exactly one newline |
