# Repo scripts

Maintenance commands for this monorepo. Plain Node — `.mjs` with JSDoc types, no
dependencies, no build step.

```sh
pnpm repo                      # list commands
pnpm repo <command> --help     # options for one
pnpm bump --minor -f vim-ultra # shortcuts for the common ones
pnpm sync-vscodeignore
pnpm test:scripts
```

## Layout

| Path | Holds |
| --- | --- |
| `main.mjs` | Entry point: the command list and the top-level error handler |
| `commands/` | One file per command, each exporting a `Command` |
| `lib/` | Shared pieces: dispatch, repo layout, semver, JSON files, reporting |
| `templates/` | Files copied into extensions verbatim |
| `test/` | `node --test` suites |

## Adding a command

Write `commands/<name>.mjs` exporting a `Command` — `name`, `summary`, `options`
(each with a `describe`, which is what the help text prints), and `run(ctx)`,
which returns an exit code. `ctx` carries the parsed `values`, the `repo`, and
`write`. Then list it in `main.mjs` and add a suite under `test/`, using
`makeRepo()` from `test/helpers.mjs` to work against a temporary checkout rather
than this one.

Throw `UsageError` for bad input: it prints the message and the relevant help
without a stack trace. Anything else is a bug and prints in full.

Type checking comes from `jsconfig.json` and needs no setup: the editor reads it
and checks the JSDoc as you type.
