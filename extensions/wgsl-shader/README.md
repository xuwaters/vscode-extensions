# WGSL Shader

WGSL language support for VS Code: syntax highlighting, completion, an outline,
and real validation — the diagnostics come from
[naga](https://github.com/gfx-rs/wgpu), the same parser and validator `wgpu`
uses, compiled to WebAssembly and running in-process. No language server, no
toolchain to install.

## Features

- **Syntax highlighting** for `.wgsl`: line and block comments, attributes
  (`@group`, `@workgroup_size`, …), declarations and control flow, function
  definitions and calls, built-in and user types, numeric and boolean
  constants, and punctuation.

- **Validation** with naga. `wgsl::parse_str` runs first, then the full
  `Validator` with all capabilities enabled, so both syntax errors and semantic
  ones — type mismatches, bad bindings, invalid entry points — surface as
  diagnostics at the reported line and column. Runs when a file is opened and,
  unless you turn `wgsl.validate.onSave` off, on every save;
  `wgsl.validate.onType` adds a 300 ms-debounced pass while you type, and
  **WGSL: Validate Current File** runs it on demand.

- **Completion**: 94 built-in functions (math, texture, atomic, packing,
  barriers), 68 types including the `vec4f` / `mat3x3h` short forms and every
  texture and sampler type, 27 keywords, and 16 attributes offered as
  `@attribute`. Alongside those, the symbols naga finds in the file you are
  editing — your functions, global variables and types — each labelled so you
  can tell them from the built-ins.

- **Outline and breadcrumbs**: `fn` and `struct` declarations are published as
  document symbols, so Go to Symbol in File, the Outline view and breadcrumbs
  all work.

- **Editing niceties** from the language configuration: `//` and `/* */`
  comment toggling, bracket matching and auto-closing for `{}`, `[]`, `()` and
  `<>` (angle brackets are skipped inside strings), indent on `{`, dedent on
  `}`, and `// region` / `// endregion` folding markers.

- **Embedded WGSL** in Rust, JavaScript and TypeScript — see below.

## Embedded WGSL

Shader source written inside another language is highlighted when the string is
tagged with a `/* wgsl */` block comment. All spellings of the tag work
(`/*wgsl*/`, `/*  wgsl  */`).

In Rust, on plain, byte and raw strings — including `r#"…"#` with any number of
hashes, which is the usual way to write a shader since WGSL needs no escapes:

```rust
const SHADER: &str = /* wgsl */ r#"
    @fragment
    fn fs_main() -> @location(0) vec4f {
        return vec4f(1.0, 0.0, 0.0, 1.0);
    }
"#;
```

In JavaScript, JSX, TypeScript and TSX, on template literals, with `${…}`
substitutions kept as host-language expressions:

```ts
const shader = /* wgsl */ `
    @compute @workgroup_size(${size})
    fn cs_main(@builtin(global_invocation_id) id: vec3u) { }
`;
```

Escapes stay the host language's (`\n` in a Rust `"…"` is a Rust escape, not
WGSL text), and the string delimiters keep their host scopes, so the rest of the
file is unaffected. This is highlighting only: completion and validation apply
to `.wgsl` files, not to embedded strings.

### rust-analyzer hides it by default

rust-analyzer emits a `string` semantic token covering the whole literal, and in
VS Code semantic tokens override TextMate scopes — so the shader body stays one
flat string colour. Turn the semantic token off:

```jsonc
"rust-analyzer.semanticHighlighting.strings.enable": false
```

This extension offers to do that the first time it sees a Rust file with a
`/* wgsl */` tag, preferring the workspace over user settings so the rest of your
Rust keeps its semantic highlighting. Set `wgsl.rust.highlightHint` to `false` to
stop it asking.

There is no way to disable the semantic token for one string: precedence is
decided per token by VS Code, and rust-analyzer's setting is scoped per workspace
folder at finest. The change is narrow, though — comparing rust-analyzer's
semantic tokens for the same file with the setting on and off, the only tokens
that disappear are `string` and `escapeSequence`, both of which the Rust TextMate
grammar already colours. Keywords, macros, variables, types, numbers, operators
and comments are unaffected.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `wgsl.validate.onSave` | `true` | Validate WGSL files on save |
| `wgsl.validate.onType` | `false` | Validate while typing, debounced by 300 ms |
| `wgsl.completion.enabled` | `true` | Enable code completion |
| `wgsl.rust.highlightHint` | `true` | Offer the rust-analyzer fix above |

## Commands

- **WGSL: Validate Current File** (`wgsl.validateFile`) — validate the active
  document now.
