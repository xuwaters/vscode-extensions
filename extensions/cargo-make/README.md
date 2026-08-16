# cargo-make (Makefile.toml)

Editing support for [cargo-make](https://github.com/sagiegurari/cargo-make)
`Makefile.toml` files: syntax highlighting with embedded script highlighting,
an outline, folding, hover docs, go-to-definition on task references, schema
completion, and diagnostics. Everything is computed in-process by a Rust
analyzer compiled to WebAssembly — there is no language server, and the
extension never shells out to `cargo make`.

The `cargo-make` language applies to files named `Makefile.toml` and to any
file matching `*.makefile.toml`.

## Features

- **Syntax highlighting** tuned for the TOML that cargo-make actually uses:
  `#` comments, `[table]` and `[[array-of-table]]` headers with dotted paths
  (quoted path segments included), bare and quoted keys, all four TOML string
  forms, booleans, dates, decimal / hex / octal / binary integers, floats
  including `inf` and `nan`, arrays and inline tables. Environment
  substitutions like `${CARGO_MAKE_WORKING_DIRECTORY}` are highlighted as
  template expressions inside basic strings.

- **Embedded script highlighting** — see below.

- **Outline and breadcrumbs**: every `[tasks.NAME]` becomes a symbol whose
  detail is the task's `description`, or, when there is none, a one-word
  summary of what it does (`command`, `script`, `run_task`, or
  `N dependencies`). `[env]` and `[config]` appear as groups with one child
  per key, each showing a whitespace-collapsed preview of its value truncated
  to 60 characters.

- **Folding** per table: each header folds down to the last non-blank line
  before the next header, so trailing blank lines collapse away and tables
  with no body are left alone. The language configuration also declares
  `# region` / `# endregion` markers.

- **Hover**: task field keys, `condition` criteria and `[config]` keys show a
  one-line description drawn from cargo-make's schema. Put the cursor on a
  task name in `dependencies`, `run_task` or an `alias` and you get the target
  task's description, category and dependencies instead; the same summary
  shows on a `[tasks.NAME]` header. Keys under `[env]` are labelled as
  environment variables.

- **Go to definition** from a task reference — in `dependencies`, `run_task`,
  `alias`, `linux_alias`, `windows_alias` or `mac_alias` — to the
  `[tasks.NAME]` header that defines it. References resolve within the same
  file; `extend`ed files are not followed.

- **Completion**, driven by the nearest table header above the cursor: 7
  top-level section names at the document root, 37 task fields inside
  `[tasks.NAME]`, 17 criteria inside a `condition` table, and 22 keys inside
  `[config]`. Keys already present in the current task or in `[config]` are
  filtered out. On the value side, `script_runner =` offers the built-in
  runners `@duckscript`, `@rust` and `@shell`, and `dependencies`, `run_task`
  and the `alias` fields offer the task names defined in the file, with each
  task's description as the completion detail. The line you are typing is
  removed before the document is re-parsed, so a half-written line does not
  cost you the rest of the file's task names. Nothing is offered while the
  cursor sits on a table header line.

- **Diagnostics** for TOML parse errors and structural mistakes — see below.

## Embedded scripts

Script values are highlighted as shell, whether written as a single-line
string, a `'''` or `"""` block, or an array of strings. The fields treated
this way are `script`, `install_script`, `condition_script`, `load_script`,
`linux_load_script`, `windows_load_script`, `mac_load_script`, `pre`, `main`
and `post`, each also with a dotted suffix (`script.linux`).

A shebang on the first line of a block switches the embedded grammar:
`python` gives Python, `node` gives JavaScript, `ruby` gives Ruby, `perl`
gives Perl, and `#!@rust` gives Rust. A sibling `script_runner` key cannot
influence this — TextMate grammars have no cross-key state — so a shebang is
the way to get non-shell highlighting.

```toml
[config]
min_version = "0.37.0"
default_to_workspace = false

[env]
TARGET_DIR = "${CARGO_MAKE_WORKING_DIRECTORY}/target"

[tasks.build]
description = "Build the release binary"
category = "Build"
command = "cargo"
args = ["build", "--release"]

[tasks.dist]
description = "Package the release build"
dependencies = ["build"]
script_runner = "bash"
script = '''
set -e
mkdir -p "${TARGET_DIR}/dist"
cp "${TARGET_DIR}/release/app" "${TARGET_DIR}/dist/"
'''

[tasks.report]
script = '''
#!/usr/bin/env python3
import os
print(os.listdir(os.environ["TARGET_DIR"]))
'''

[tasks.dist.condition]
platforms = ["linux", "mac"]
```

## Diagnostics

| Code | Severity | Reported when |
| --- | --- | --- |
| `CARGOMAKE001` | Error | The document is not valid TOML |
| `CARGOMAKE002` | Warning | A task declares more than one of `command`, `script`, `run_task` |
| `CARGOMAKE003` | Warning | A key under `[config]` is not a known config key |
| `CARGOMAKE004` | Warning | A key in a task table — or in a `linux` / `windows` / `mac` override — is not a known task field |
| `CARGOMAKE005` | Warning | A key in a task `condition` table is not a known criterion |
| `CARGOMAKE006` | Warning | A task lists itself in its own `dependencies` |

They are recomputed when a file is opened, when it is saved, and — unless you
turn `cargoMake.diagnostics.onType` off — 150 ms after you stop typing.
Turning `cargoMake.diagnostics.enabled` off clears them immediately.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `cargoMake.diagnostics.enabled` | `true` | Emit diagnostics for TOML parse errors, unknown task/config keys, conflicting task actions, and self-dependencies |
| `cargoMake.diagnostics.onType` | `true` | Recompute diagnostics as you type (debounced). Turn off to only recompute on save |
