# Contributing to Askama Templates

## Setup

```sh
pnpm install
pnpm --filter wx-vsce-askama-templates run build
pnpm --filter wx-vsce-askama-templates test
```

Open the repository in VS Code and run the extension host to try the grammars
on the files in `examples/`.

## Adding a template language

The languages are generated from one table,
[scripts/languages/languages.json](scripts/languages/languages.json). Add an
entry there:

```json
{ "id": "askama-sh", "name": "Shell Script", "scope": "source.shell.askama", "host": "source.shell", "extensions": [".sh"] }
```

- `host` is the TextMate scope of the format being templated. To find it, look
  up the format's `contributes.grammars[].scopeName` in the extension that
  ships it. For a built-in, that is
  `/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/<name>/package.json`.
- `extensions` lists base names only. Each one is expanded with all four
  template suffixes (`.askama`, `.j2`, `.jinja`, `.jinja2`).

Then regenerate and test:

```sh
pnpm --filter wx-vsce-askama-templates run build:languages
pnpm --filter wx-vsce-askama-templates test
```

The generator writes these files:

- `package.json`: `contributes.languages`, `grammars` and `snippets`
- `syntaxes/askama-<name>.tmLanguage.json`
- the `injectionSelector` in `syntaxes/askama-injection.tmLanguage.json`
- `src/languages.ts`

Don't edit those parts by hand. The test suite runs the generator with
`--check` and fails if any of them are stale.

The tests also check each `host` against the VS Code installed on your machine,
and tokenize a template with VS Code's real grammars. If a host grammar ships
in a third-party extension instead of VS Code, mark it `"provider": "extension"`.
If the format has no grammar anywhere, write one by hand and mark the entry
`"bundled": true`; Cap'n Proto is an example. Then update the language list in
`README.md`.
