# Makefile

GNU Make support for VS Code: syntax highlighting, an outline, folding, and
diagnostics for the mistakes Make punishes hardest — spaces where a tab
belongs, an unclosed `define`, a dangling `ifeq`. The parser is a Rust crate
compiled to WebAssembly and running in the extension host, so there is no
language server and no toolchain to install. Recipe lines also get shell
option completion drawn from fish-shell's completion data.

## Features

- **Syntax highlighting** for targets and prerequisites, `#` comments,
  assignment operators (`=`, `:=`, `::=`, `?=`, `+=`, `!=`) with an optional
  `export` / `override` / `private` prefix, the conditional keywords (`ifeq`,
  `ifneq`, `ifdef`, `ifndef`, `else`, `endif`), the directives `export`,
  `unexport`, `override`, `private`, `undefine` and `vpath`, `define` …
  `endef` blocks, and `include` / `-include` / `sinclude` lines with their
  paths.

- **Recipe bodies are highlighted as recipes**: the leading `@`, `-` and `+`
  prefix characters are called out, and inside the line you get variable
  references (`$(CC)`, `${CC}`, and `$$` as an escape), automatic variables
  (`$@`, `$<`, `$^`, `$?`, `$*`, `$%`, `$+`, `$|`, plus the `$(@D)` / `$(@F)`
  forms), function calls such as `$(shell …)`, `$(wildcard …)` and
  `$(patsubst …)`, single-, double- and backtick-quoted shell strings, `#`
  shell comments, and trailing `\` line continuations. Variable and automatic
  variable references are also highlighted in prerequisite lists and inside
  `define` bodies.

- **Outline and breadcrumbs** built from the parsed file, not a regex sweep.
  Rules appear under their first target, with the remaining targets, the
  prerequisite list and a `::` marker for double-colon rules shown as detail;
  pattern rules (any target containing `%`) are recognised as such. Variable
  assignments show their operator and a truncated value preview, `define`
  blocks show `define` plus the operator, `include` lines show their paths,
  and `ifeq` / `ifdef` blocks become a container node whose children are the
  items in both branches.

- **Folding** for rule bodies that have a recipe, `define` … `endef` blocks
  and conditional blocks, on top of the `# region` / `# endregion` markers
  from the language configuration.

- **Diagnostics** for four classes of error, reported with a `MAKE00n` code:

  | Code | Message |
  | --- | --- |
  | `MAKE001` | recipe line uses spaces where a tab is required |
  | `MAKE002` | recipe line appears outside of any target rule |
  | `MAKE003` | `` `define` `` block is not closed — expected `endef` |
  | `MAKE004` | `` `ifeq` `` (or `ifneq` / `ifdef` / `ifndef`) is not closed — expected `endif` |

  They are computed when a file is opened, on save, and — unless you turn
  `makefile.diagnostics.onType` off — about 150 ms after you stop typing.

- **Tabs are enforced for you.** The extension sets
  `editor.insertSpaces: false`, `editor.detectIndentation: false` and
  `editor.useTabStops: true` for the `makefile` language, so a press of Tab in
  a recipe inserts a real tab regardless of how the rest of your editor is
  configured. If a space-indented line still sneaks in right after a rule
  header, `MAKE001` flags it.

- **Shell completion inside recipes.** Put the cursor in a recipe line and the
  extension completes the current command's options, its subcommands, and the
  values after `--option=`, from a database generated from fish-shell's
  completion scripts (roughly a thousand commands). It skips the recipe
  prefix characters, follows `;`, `|`, `||`, `&&` and backticks to find the
  command you are actually in, respects single and double quotes, joins `\`
  continuations, and strips a path so `/usr/bin/curl` still completes as
  `curl`. Nothing is offered on target headers, assignments or comments, and
  the command name itself is not completed — only its arguments. Results are
  capped at 250 per keystroke and descriptions are trimmed to their first
  sentence.

- **Editing niceties** from the language configuration: `#` line-comment
  toggling, bracket matching and auto-closing for `{}`, `[]` and `()`,
  auto-closing and surrounding for `"`, `'` and `` ` `` outside strings and
  comments, and `# region` / `# endregion` folding markers.

```makefile
# region build
CC      := gcc
CFLAGS  ?= -O2 -Wall
SRC     := $(wildcard src/*.c)
OBJ     := $(patsubst %.c,%.o,$(SRC))
VERSION != git describe --tags

.PHONY: all clean

all: app

app: $(OBJ)
	$(CC) $(CFLAGS) -o $@ $^

%.o: %.c
	@$(CC) $(CFLAGS) -c $< -o $@

ifeq ($(shell uname -s),Linux)
LDFLAGS += -ldl
endif

define greet
	echo "building $(1)"
endef

clean:
	-rm -f $(OBJ) app
# endregion

-include local.mk
```

## Files it applies to

The `makefile` language is bound to the filenames `Makefile`, `makefile` and
`GNUmakefile`, and to the extensions `.mk`, `.mak` and `.make`. Other spellings
(`Makefile.local`, `*.makefile`) are not claimed; use the language picker in
the status bar, or a `files.associations` entry, for those.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `makefile.diagnostics.enabled` | `true` | Emit diagnostics for unterminated define blocks, unclosed conditionals, and recipe lines that use spaces instead of tabs |
| `makefile.diagnostics.onType` | `true` | Recompute diagnostics as you type (debounced). Turn off to only recompute on save |
| `makefile.outline.showPhonyDeclarations` | `false` | Show `.PHONY: …` declaration lines in the document outline |

## License

Distributed under GPL-2.0-or-later. The extension's own code was written for
this project, but the shipped WebAssembly bundle embeds completion data
derived from [fish-shell](https://github.com/fish-shell/fish-shell), which is
GPL-2-or-later — see `LICENSE.md` and `THIRD_PARTY_NOTICES.md`.
