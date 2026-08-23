# Packaging scripts

The commands that assemble, inject, and verify this extension's TypeScript
server plugin. Plain Node — `.mjs` with JSDoc types, no dependencies, no build
step.

```sh
pnpm fast-element                        # list commands
pnpm fast-element <command> --help       # options for one
pnpm build                               # runs assemble-tsplugin
pnpm package                             # runs inject-tsplugin
pnpm verify:vsix                         # runs verify-vsix
```

## Layout

| Path | Holds |
| --- | --- |
| `main.mjs` | Entry point: the command list and the top-level error handler |
| `commands/` | One file per command, each exporting a `Command` |
| `lib/layout.mjs` | Where everything lives; the plugin's name and file list |
| `lib/zip.mjs` | Reaching inside a VSIX — a plain zip — and temp directories |
| `lib/checks.mjs` | The shared shape of "check this, report it, exit non-zero" |

Dispatch, flag parsing, and help text come from the repo's script framework at
[`scripts/lib/cli.mjs`](../../../scripts/lib/cli.mjs) rather than a copy of it.
What differs here is the context commands receive: a `Layout` describing this
extension, where the monorepo commands get a `Repo`.

## Why these exist at all

tsserver resolves a plugin only from a probe location's
`node_modules/<pluginName>`, and VS Code probes the extension's install
directory — so the plugin must be a real package directory inside the
extension, in the working tree and in the VSIX both. vsce will not put it
there. See [CONTRIBUTING.md](../CONTRIBUTING.md#packaging-and-why-it-is-unusual)
for the full story.

## Adding a command

Write `commands/<name>.mjs` exporting a `Command` — `name`, `summary`,
`options` (each with a `describe`, which is what the help text prints), and
`run(ctx)`, which returns an exit code. `ctx` carries the parsed `values`, the
`layout`, and `write`. Then list it in `main.mjs`.

Take every path from `ctx.layout` rather than composing one, so the layout
stays stated in a single place. Throw `UsageError` for bad input: it prints the
message and the relevant help without a stack trace. A missing build product is
not bad input — report it with `reportMissing` and return 1.

Type checking comes from `jsconfig.json`; `pnpm typecheck` runs it, and the
editor reads it as you type.
