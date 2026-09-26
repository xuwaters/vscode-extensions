# VS Code Extensions

A monorepo of VS Code extensions. Most of them pair a thin TypeScript client with
a Rust engine compiled to WebAssembly, so parsing, analysis and rendering run
inside the extension host with no native binaries to install.

Each extension has its own README with the full feature list and settings.

## Extensions

### Languages

| Extension | What it does |
| --- | --- |
| [Typst Ultra](extensions/typst-ultra) | Typst language server and live, two-way-synced preview on unmodified upstream typst; PDF/SVG/PNG export and BibTeX support |
| [Typst Ultra Fonts](extensions/typst-ultra-fonts) | Typst's default font set, packaged for Typst Ultra |
| [WGSL / GLSL Shader](extensions/wgsl-shader) | Language server for WGSL (validated by naga) and GLSL, including shaders embedded in Rust and TypeScript |
| [FAST Element Ultra](extensions/fast-element-ultra) | Diagnostics, completion, hover, navigation and rename inside FAST Element `html` and `css` tagged templates |
| [Protocol Buffers (proto3)](extensions/protobuf) | Highlighting, outline and diagnostics for `.proto` files |
| [Cap'n Proto Ultra](extensions/capnproto) | Highlighting, outline and diagnostics for `.capnp` schemas |
| [Mojom IDL Ultra](extensions/mojom) | Highlighting, outline, navigation and diagnostics for `.mojom` files |
| [JSON Ultra](extensions/json-ultra) | JSON, JSONC, JSON5 and JSON Lines: diagnostics, comment-preserving formatting, JSON-path hovers, a JSON Lines table preview |
| [dotenv (.env)](extensions/dotenv) | Highlighting, diagnostics, formatting and variable completion for `.env` files |
| [Makefile](extensions/makefile) | Highlighting and outline for GNU Make |
| [cargo-make (Makefile.toml)](extensions/cargo-make) | Highlighting, outline, completion, hover and diagnostics for `Makefile.toml` |
| [Diesel schema.rs](extensions/diesel-schema) | Outline, hover, completion and diagnostics for Diesel `schema.rs` files |
| [Askama Templates](extensions/askama-templates) | Highlighting and snippets for Askama templates |

### Viewers and editors

| Extension | What it does |
| --- | --- |
| [Markdown Preview Ultra](extensions/markdown-preview-ultra) | Live Markdown preview with math, mermaid, GitHub alerts, a TOC and two-way scroll sync |
| [PDF Ultra](extensions/pdf-ultra) | PDF viewer built on pdf.js, with find, outline, zoom and live reload |
| [CSV Ultra](extensions/csv-ultra) | CSV and TSV as an editable, sortable spreadsheet, plus rainbow columns in the text editor |
| [Log Viewer Ultra](extensions/log-viewer) | Viewer for large `.log` files with ANSI colour rendering |
| [Vim Ultra](extensions/vim-ultra) | Modal Vim editing: motions, operators, text objects, registers, visual modes, search and `:s` |

### Tools

| Extension | What it does |
| --- | --- |
| [Git Compare](extensions/git-compare) | Compare the working copy with any branch, tag or commit as a file tree in Source Control |
| [Cloudflare AI Models](extensions/cloudflare-ai-models) | Use Cloudflare Workers AI and AI Gateway models (or any OpenAI-compatible endpoint) as chat models in VS Code |
| [Claude Usage Ultra](extensions/claude-usage-ultra) | Claude Code plan usage and reset countdowns in the status bar |
| [Gitignore Generator Ultra](extensions/gitignore-generator) | Generate or extend `.gitignore` from bundled templates |
| [Base64 Tools](extensions/base64-tools) | Base64-encode and -decode the selected text |
| [Remove .cc-writes](extensions/rmccwrites) | Clean up empty `.cc-writes` directories left in a workspace |

## Installing

The extensions are not on the Marketplace. Build a `.vsix` (see below), then
install it with **Extensions: Install from VSIX…** or:

```sh
code --install-extension extensions/<name>/wx-vsce-<name>-<version>.vsix
```

## Building

### Prerequisites

- Node.js 20.19 or later
- pnpm, through corepack: `corepack enable` picks up the version pinned in
  `package.json`
- Rust (stable) with the WebAssembly target: `rustup target add wasm32-unknown-unknown`
- [wasm-pack](https://rustwasm.github.io/wasm-pack/): `cargo install wasm-pack`

Or open the repo in the dev container under [.devcontainer](.devcontainer),
which has all of this installed.

### Commands

```sh
pnpm install

# Package one extension; the result lands in extensions/<name>/
pnpm --filter wx-vsce-<name> package

# Or work on one extension step by step
pnpm --filter wx-vsce-<name> build:wasm   # Rust engine → extensions/<name>/wasm/ (Rust-backed extensions only)
pnpm --filter wx-vsce-<name> build        # TypeScript → dist/
pnpm --filter wx-vsce-<name> test

# Across the whole repo
pnpm typecheck
pnpm test
cargo test --workspace
```

`build` does not run `build:wasm`, so run it first for a Rust-backed extension.
`package` runs both.

Some extensions have a `CONTRIBUTING.md` with more detail, such as extra data
sources or how to regenerate their notices.

## Repository layout

| Path | Holds |
| --- | --- |
| `extensions/` | One folder per extension: TypeScript sources, grammars, `package.json` |
| `crates/` | The Rust engines. Crates used by one extension are grouped in a folder named after it, such as `crates/typst` and `crates/wgsl-shader` |
| `scripts/` | Repo maintenance commands (`pnpm repo`), such as version bumps. See [scripts/README.md](scripts/README.md) |
| `docs/rfc/` | Design documents for the larger extensions |

## License

Each extension is licensed separately; see the `LICENSE.md` in its folder.
Third-party material that an extension redistributes is listed in its
`THIRD-PARTY-NOTICES.md`.
