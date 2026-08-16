# Gitignore Generator

Write a `.gitignore` for the folder you have open, picked from eleven templates
that ship inside the extension. Nothing is downloaded — no gitignore.io, no
`github/gitignore` checkout, no cache to warm or invalidate — so the command
works the same offline as online.

## Features

- **Eleven bundled templates**, multi-select. The picker lists Default, Node,
  Python, Rust, Go, Java, macOS, Windows, Linux, VS Code and JetBrains, in that
  order, each with a one-line description (`Node.js, npm, pnpm, yarn`; `Cargo
  build artifacts`; `IntelliJ / PyCharm / WebStorm`; …). Filtering matches the
  descriptions as well as the names, so typing `gradle` finds Java and `venv`
  finds Python. Tick as many as you want; Default starts ticked.

- **The Default template** is a single pragmatic mix rather than one ecosystem:
  `.DS_Store`, Rust `target`, `node_modules` and the usual JS build and cache
  directories, debug logs, `.env` files (with `!.env.example` kept), `*.local`,
  `*.zip`, `temp`, `.claude`, `.vscode`, `.sqlx-cache`, `*.db` and
  `__pycache__`. It is the right pick for a small polyglot repo; the
  per-language templates are fuller and stricter.

- **Labelled sections.** Every template is written under a
  `### Gitignore Generator: <name>` header, with a blank line between sections,
  so you can tell later which block came from where and delete one cleanly.

- **Append or overwrite, your choice.** If a `.gitignore` already exists,
  **Generate .gitignore** stops and asks in a modal dialog: Overwrite replaces
  the file with just the sections you picked, Append keeps what is there and
  adds to it, and dismissing the dialog does nothing at all. When appending,
  trailing blank lines on the existing file are collapsed to exactly one blank
  line before the new sections.

- **Multi-root aware.** With several folders in the workspace you are asked
  which one the `.gitignore` belongs in. With one folder it is used without a
  prompt. With no folder open you get an error telling you to open one.

- **The file opens afterwards**, in an editor tab, so you can see what was
  written and edit it before committing.

## Usage

1. Run **Gitignore: Generate .gitignore** from the Command Palette.
2. If the workspace has more than one folder, choose the target folder.
3. Tick the templates you want and confirm. Default is pre-ticked; untick it if
   you would rather assemble from the per-language templates.
4. If the folder already has a `.gitignore`, choose Overwrite or Append in the
   dialog.
5. The `.gitignore` opens in an editor. Review it and commit.

To skip the overwrite question entirely and always add to the end of the file,
use **Gitignore: Append Templates to .gitignore** instead. It creates the file
if it does not exist yet, and never removes anything.

Note that appending does not deduplicate: running it twice with the same
template gives you two copies of that section. Git does not mind repeated
patterns, but you may want to tidy up.

## Commands

- **Gitignore: Generate .gitignore** (`gitignore-generator.generate`) — pick
  templates and write `.gitignore`, asking what to do if one already exists.
- **Gitignore: Append Templates to .gitignore** (`gitignore-generator.append`) —
  pick templates and append them to `.gitignore`, creating it if needed.

## Settings

This extension contributes no settings.
