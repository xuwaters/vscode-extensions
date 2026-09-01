# WGSL / GLSL Shader

A language server for WGSL and GLSL. Completion that knows what the cursor is
pointing at, hover with real types, go-to-definition, rename, semantic
highlighting, and validation by [naga] — the same front end `wgpu` uses.

It works in `.wgsl` and `.glsl` files, and inside shaders written in Rust and
TypeScript string literals.

[naga]: https://github.com/gfx-rs/wgpu/tree/trunk/naga

## What you get

| | |
| --- | --- |
| **Completion** | Members and swizzles after `.`, attributes after `@`, directives after `#`, address spaces inside `var<…>`, layout qualifiers inside `layout(…)`, and everything in scope everywhere else. Locals rank above file-scope names, which rank above the language's own. |
| **Hover** | The declaration as written, the type naga inferred for it, and the `//` comment above it. Built-ins show their signature and what they do. |
| **Go to definition** | Including `camera.view`, which resolves through the *type* of `camera` to the field's declaration in that struct. Reaches files you have not opened. |
| **Find references, rename** | Across the file, with built-in names refused before the rename box opens. |
| **Outline and symbol search** | Nested — a struct's fields and a function's parameters sit under it. Entry points get their own icon. `⌘T` searches every shader in the workspace, opened or not. |
| **Semantic highlighting** | Colour by what a name *resolves to*: `Light` paints as a struct because a struct was declared under that name, `dot` as a library function because the language defines one. |
| **Diagnostics** | Parse and validation errors from naga, on the token that caused them. |
| **Signature help** | Parameter hints for every built-in and for your own functions. |
| **Inlay hints** | Inferred types on bindings that declare none, and parameter names at call sites. Off by default. |
| **Code actions** | Add a missing `#version`, pin a guessed GLSL stage with `#pragma shader_stage(…)`, switch a WGSL type between `vec4f` and `vec4<f32>`. |
| **Folding** | Bodies and comment runs. |
| **Formatting** | Re-indentation only. Off by default. |

## Shaders inside Rust and TypeScript

Tag a string literal with a `/* wgsl */` or `/* glsl */` comment and it gets
syntax highlighting **and** the language features above:

```rust
let shader = /* wgsl */ r#"
    @fragment
    fn fs_main() -> @location(0) vec4f {
        return vec4f(1.0, 0.0, 0.0, 1.0);
    }
"#;
```

```ts
const frag = /* glsl */ `
  #version 450
  layout(location = 0) out vec4 colour;
  void main() { colour = vec4(${red}, 0.0, 0.0, 1.0); }
`;
```

Interpolations and escape sequences are handled: `${red}` is treated as a value
of the right shape rather than as a syntax error.

Diagnostics are **off** inside embedded blocks by default. A shader written in a
string is often a fragment that gets concatenated with others at runtime, and
validating it as a standalone module reports errors about code nobody wrote.
Turn on `wgsl.embedded.diagnostics` (or `glsl.embedded.diagnostics`) if yours
are complete programs.

### If the colours are missing in Rust

rust-analyzer paints whole string literals with a `string` semantic token, and
semantic tokens win over TextMate scopes. The extension offers, once, to turn
off `rust-analyzer.semanticHighlighting.strings.enable`; Rust strings keep their
colour from the grammar either way.

## GLSL and its stages

GLSL has no way to say, in the file, which stage it is — and naga needs to know
before it can parse at all. The stage is taken from, in order:

1. `#pragma shader_stage(vertex | fragment | compute)`, which `glslc` also reads;
2. the file extension (`.vert`, `.frag`, `.comp`, and the usual variants);
3. the stage-exclusive built-ins the source uses (`gl_FragColor` → fragment).

The status bar shows which one was picked. `GLSL: Show Shader Stage of Current
File` explains it in full, and a code action offers to write a guess down as a
`#pragma`.

### What naga does not validate

naga's GLSL front end implements the vertex, fragment and compute stages at
`#version 440`, `450` and `460 core`. Outside that — GLSL ES, geometry,
tessellation, ray tracing — the file is **not validated**, and the status bar
says so rather than pretending.

Everything else still works there. Completion, hover, definition, rename,
symbols and folding come from a parser of this extension's own, which covers
every dialect and does not need the file to be valid. That parser is also why
those features keep working mid-keystroke, when the file is temporarily
nonsense — which is most of the time you are typing.

## Settings

Every setting exists under both `wgsl.` and `glsl.`.

| Setting | Default | |
| --- | --- | --- |
| `validate.onSave` | `true` | Validate when the file is saved. |
| `validate.onType` | `false` | Validate on every keystroke. Off because a file is invalid for most of the time it is being edited. |
| `completion.enabled` | `true` | |
| `semanticTokens` | `true` | Colour by resolved meaning. |
| `inlayHints.enabled` | `false` | |
| `inlayHints.types` | `true` | Inferred types on bindings that declare none. |
| `inlayHints.parameterNames` | `true` | `mix(e1: a, e2: b, e3: t)`. |
| `format.enable` | `false` | The re-indenter. |
| `format.indentWidth` | `4` | |
| `embedded.enabled` | `true` | Language features inside tagged string literals. |
| `embedded.diagnostics` | `false` | Validate embedded shaders too. |

Plus two of their own:

| Setting | Default | |
| --- | --- | --- |
| `glsl.showStageInStatusBar` | `true` | |
| `wgsl.rust.highlightHint` | `true` | Offer the rust-analyzer fix described above. |

## Commands

| Command | |
| --- | --- |
| `WGSL: Validate Current File` | Validate now, whatever `validate.onSave` says. |
| `GLSL: Validate Current File` | The same. |
| `GLSL: Show Shader Stage of Current File` | Which stage, and why. |
| `WGSL / GLSL: Restart Language Server` | |

## Languages and file types

**WGSL** — `.wgsl`.

**GLSL** — `.glsl`, `.vert`, `.frag`, `.comp`, `.geom`, `.tesc`, `.tese`,
`.vsh`, `.fsh`, `.gsh`, `.vshader`, `.fshader`, `.gshader`, `.glslv`, `.glslf`,
`.vertexshader`, `.fragmentshader`.

## How it runs

The server is a child process, not part of the extension host: a shader
compiler is not the kind of thing to run in-process, and a panic on a malformed
module costs a restart rather than every extension in the window.

Analysis is Rust compiled to WebAssembly — one artifact for every platform VS
Code runs on, with no toolchain to install and nothing to download on first use.
