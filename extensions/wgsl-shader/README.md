# WGSL / GLSL Shader

Shader language support for VS Code: syntax highlighting, completion, an outline,
and real validation for **WGSL** and **GLSL**. The diagnostics come from
[naga](https://github.com/gfx-rs/wgpu) 30 — the same parser and validator `wgpu`
uses — compiled to WebAssembly and running in-process. No language server, no
toolchain to install.

## File types

| Language | Extensions |
| --- | --- |
| WGSL | `.wgsl` |
| GLSL | `.glsl`, `.vert`, `.frag`, `.comp`, `.geom`, `.tesc`, `.tese`, `.vsh`, `.fsh`, `.gsh`, `.vshader`, `.fshader`, `.gshader`, `.glslv`, `.glslf`, `.vertexshader`, `.fragmentshader` |

Anything else can be pointed at a language by hand with `files.associations`:

```jsonc
"files.associations": { "*.inc": "glsl" }
```

## Features

- **Syntax highlighting** for both languages. WGSL: comments, attributes
  (`@group`, `@workgroup_size`, …), declarations and control flow, function
  definitions and calls, built-in and user types, numeric and boolean constants.
  GLSL: preprocessor directives with their arguments, `layout(…)` qualifier
  names, every built-in scalar, vector, matrix, sampler, texture and image type,
  `gl_`-prefixed built-ins, built-in versus user function calls, and the
  identifiers GLSL reserves for future use, flagged as invalid.

- **Validation** with naga. The front end parses, then the full `Validator`
  with all capabilities enabled runs, so both syntax errors and semantic ones —
  type mismatches, bad bindings, invalid entry points — surface as diagnostics
  at the reported line and column. Runs when a file is opened and, unless you
  turn `…validate.onSave` off, on every save; `…validate.onType` adds a
  300 ms-debounced pass while you type, and the **Validate Current File**
  commands run it on demand.

- **Completion**. WGSL: 100 built-in functions, 68 types including the `vec4f` /
  `mat3x3h` short forms, 27 keywords, and 16 attributes offered as
  `@attribute`. GLSL: 158 built-in functions, 156 types including the whole
  `isampler2DArray` / `uimageCube` families, 43 keywords and qualifiers, 45
  `layout(…)` qualifier names, 14 preprocessor directives offered as
  `#directive`, and 37 `gl_` built-in variables — each labelled with the stages
  it belongs to, so `gl_GlobalInvocationID` reads as *compute* at a glance.
  Alongside those, the symbols naga finds in the file you are editing — your
  functions, global variables and types.

- **Outline and breadcrumbs**. WGSL publishes `fn` and `struct` declarations;
  GLSL publishes function definitions, `struct`s, named interface blocks
  (`uniform Camera { … }`) and qualified globals (`layout(location = 0) in vec3
  position;`). Go to Symbol in File, the Outline view and breadcrumbs all work,
  and they keep working while the file does not parse.

- **Editing niceties** from the language configurations: `//` and `/* */`
  comment toggling, bracket matching and auto-closing, indent on `{`, dedent on
  `}`, and folding markers — `// region` / `// endregion` in both, plus
  `#if` / `#endif` folding in GLSL.

- **Embedded shaders** in Rust, JavaScript and TypeScript — see below.

## The GLSL shader stage

GLSL has no way to say inside the file which stage it is, and naga needs one
before it can parse at all. The stage is worked out in this order, and shown in
the status bar (**GLSL: fragment**); **GLSL: Show Shader Stage of Current File**
spells out the same thing:

1. `#pragma shader_stage(vertex|fragment|compute)`, the `glslc` directive — this
   always wins, so it is the way to override the rest.
2. The file extension: `.vert`, `.frag`, `.comp` and their long forms.
3. What the source uses: `local_size_x` or `gl_GlobalInvocationID` means
   compute, `gl_FragCoord` or `gl_FragColor` means fragment, `gl_Position`
   means vertex. A file with none of those is treated as a fragment shader.

So a bare `.glsl` file usually lands on the right stage by itself, and
`#pragma shader_stage(…)` is there for when it does not.

## What naga's GLSL front end does not cover

naga implements a subset of GLSL, and a file outside it would otherwise report
an error on nearly every line. Those files are **highlighted but not
validated**, with no diagnostics at all; the status bar says
**GLSL: … (not validated)** and its tooltip says why.

- **Stages**: vertex, fragment and compute only. Geometry, tessellation, mesh
  and ray tracing shaders are not validated.
- **Versions**: `#version 440`, `450` and `460` core, or no `#version` line at
  all. GLSL ES (`#version 300 es`, as WebGL uses) and older desktop versions
  (`330` and below) are not validated.
- **Textures and samplers**: naga follows Vulkan GLSL, where a texture and a
  sampler are separate objects combined at the call site. A combined
  `uniform sampler2D tex;` does not validate; the Vulkan spelling does:

  ```glsl
  layout(set = 0, binding = 1) uniform texture2D albedo;
  layout(set = 0, binding = 2) uniform sampler albedo_sampler;
  // ...
  vec4 base = texture(sampler2D(albedo, albedo_sampler), uv);
  ```

Highlighting, completion and the outline are unaffected by all of this — they
cover the whole language.

## Embedded shaders

Shader source written inside another language is highlighted when the string is
tagged with a `/* wgsl */` or `/* glsl */` block comment. All spellings of the
tag work (`/*wgsl*/`, `/*  glsl  */`).

In Rust, on plain, byte and raw strings — including `r#"…"#` with any number of
hashes, which is the usual way to write a shader since neither language needs
escapes:

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
const shader = /* glsl */ `
    #version 450
    layout(local_size_x = ${size}) in;
    void main() { }
`;
```

Escapes stay the host language's (`\n` in a Rust `"…"` is a Rust escape, not
shader text), and the string delimiters keep their host scopes, so the rest of
the file is unaffected. This is highlighting only: completion and validation
apply to shader files, not to embedded strings.

### rust-analyzer hides it by default

rust-analyzer emits a `string` semantic token covering the whole literal, and in
VS Code semantic tokens override TextMate scopes — so the shader body stays one
flat string colour. Turn the semantic token off:

```jsonc
"rust-analyzer.semanticHighlighting.strings.enable": false
```

This extension offers to do that the first time it sees a Rust file with a
`/* wgsl */` or `/* glsl */` tag, preferring the workspace over user settings so
the rest of your Rust keeps its semantic highlighting. Set
`wgsl.rust.highlightHint` to `false` to stop it asking.

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
| `wgsl.validate.onType` | `false` | Validate WGSL files while typing, debounced by 300 ms |
| `wgsl.completion.enabled` | `true` | Enable WGSL code completion |
| `glsl.validate.onSave` | `true` | Validate GLSL files on save |
| `glsl.validate.onType` | `false` | Validate GLSL files while typing, debounced by 300 ms |
| `glsl.completion.enabled` | `true` | Enable GLSL code completion |
| `glsl.showStageInStatusBar` | `true` | Show the stage a GLSL file is validated as |
| `wgsl.rust.highlightHint` | `true` | Offer the rust-analyzer fix above |

## Commands

- **WGSL: Validate Current File** (`wgsl.validateFile`) — validate the active
  WGSL document now.
- **GLSL: Validate Current File** (`glsl.validateFile`) — validate the active
  GLSL document now.
- **GLSL: Show Shader Stage of Current File** (`glsl.showShaderStage`) — say
  which stage the active GLSL file is validated as, or why it is not validated.
