# WGSL / GLSL Shader

A language server for WGSL, WESL and GLSL. Completion that knows what the
cursor is pointing at, hover with real types, go-to-definition, rename, semantic
highlighting, and real diagnostics — WGSL validated by [naga], the same front
end `wgpu` uses, WESL imports followed from file to file, and GLSL checked by an
analyzer written for this extension.

It works in `.wgsl`, `.wesl` and `.glsl` files, and inside shaders written in
Rust and TypeScript string literals.

[naga]: https://github.com/gfx-rs/wgpu/tree/trunk/naga

## What you get

| | |
| --- | --- |
| **Completion** | Members and swizzles after `.`, attributes after `@`, directives after `#`, address spaces inside `var<…>`, layout qualifiers inside `layout(…)`, and everything in scope everywhere else. Locals rank above file-scope names, which rank above the language's own. |
| **Hover** | The declaration as written, the type inferred for it, and the `//` comment above it. Built-ins show their signature and what they do — for GLSL, *every* overload the declared `#version` has, with the reference page's own prose. |
| **Go to definition** | Including `camera.view`, which resolves through the *type* of `camera` to the field's declaration in that struct. Reaches files you have not opened. |
| **Find references, rename** | Across the file, with built-in names refused before the rename box opens. |
| **Outline and symbol search** | Nested — a struct's fields and a function's parameters sit under it. Entry points get their own icon. `⌘T` searches every shader in the workspace, opened or not. |
| **Semantic highlighting** | Colour by what a name *resolves to*: `Light` paints as a struct because a struct was declared under that name, `dot` as a library function because the language defines one. GLSL adds macro invocations, and dims the conditional branch you are not in. |
| **Diagnostics** | For WGSL, parse and validation errors from naga, on the token that caused them. For WESL, the same, checked with the file's imports linked in, plus imports that name nothing. For GLSL, preprocessor, parser and semantic errors from this extension's own analyzer, each with a stable `GLSL####` code — in every dialect, not just the one naga reads. |
| **Signature help** | Parameter hints for every built-in and for your own functions. GLSL's built-ins are overloaded, so you get the whole set — filtered to the `#version` in force, printed in the specification's own generic notation (`genType`, `gsampler2D`), with the one whose arity still fits what you have typed highlighted. |
| **Inlay hints** | Inferred types on bindings that declare none (WGSL) or the size an implicitly sized array takes from its initialiser (GLSL), and parameter names at call sites. Off by default. |
| **Code actions** | Add a missing `#version`, pin a guessed GLSL stage with `#pragma shader_stage(…)`, switch a WGSL type between `vec4f` and `vec4<f32>`. |
| **Folding** | Bodies, comment runs, and the `#if` branch that is switched off. |
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

## GLSL, in every dialect

GLSL is analysed by an analyzer written for this extension: a real
preprocessor, the GLSL 4.60 grammar, a type model, overload resolution, and a
builtin table generated from the [OpenGL reference pages][docs.gl] — 161
functions over 717 overloads and 31 `gl_*` variables, plus a hand-written 39
functions and 58 variables for the compatibility surface the reference pages do
not document but shaders still use. Every entry carries the versions, profiles
and stages it exists in. The result answers for **GLSL 1.10–4.60 and GLSL ES
1.00–3.20**, in the core, compatibility and ES profiles, for all six shader
stages.

One analyzer reads all of them. What changes between dialects is the *answer*,
not whether you get one:

| You are writing | It looks like | And it is read as |
| --- | --- | --- |
| **GLSL ES** — WebGL, WebGL 2, mobile | `#version 300 es`, `precision highp float;`, combined `sampler2D` uniforms, no bindings | ES's own built-in set. `texture` exists here and `sampler1D` does not; a `#version 100` file gets `texture2D` instead, and is told so when it asks for the other. |
| **Desktop OpenGL**, core or compatibility | `#version 330 core`, combined samplers, bindings the driver assigns | The desktop set for that exact version, plus — at 1.10 through 1.50 — the compatibility surface that is still everywhere in the wild: `attribute`, `varying`, `gl_FragColor`, `gl_ModelViewMatrix`. |
| **Vulkan GLSL** | `#version 450`, `layout(set = …, binding = …)`, textures and samplers as separate objects | The same, with `sampler2D(texture, sampler)` understood as the constructor it is. `#extension` lines are read, and a file that declares one is not told its extension's types are unknown names. |

### Hover, completion and signature help

- **Hover** on a built-in shows *every* overload the declared `#version` has,
  with the reference page's own prose and its per-parameter notes — not one
  hand-written signature, and not the desktop-only `sampler1D` forms in a WebGL
  shader. Hover on your own name shows the declaration as written, the type
  inferred for it, and the `//` comment above it.
- **Completion** is filtered by version *and* stage: `gl_FragCoord` in a
  fragment shader, `gl_Position` in a vertex one, and neither the
  compatibility-profile names nor the double-precision types in a shader that
  has no such thing. After a `.` it offers the members or swizzles the type
  actually has; inside `layout(…)` it offers layout qualifiers; after `#` it
  offers directives. Macros you have defined are offered from the macro table.
- **Signature help** shows the whole overload set, filtered the same way, in the
  specification's generic notation — `genType mix(genType x, genType y, float a)`
  rather than a dozen expanded rows — and highlights the overload whose arity
  still fits the arguments you have typed.

Diagnostics carry a stable code — `GLSL0001`–`GLSL0025` from the preprocessor,
`GLSL0100`–`GLSL0110` from the parser, `GLSL0200`–`GLSL0227` from semantic
analysis — so a message can be looked up and a rule can be argued with.

The analyzer is deliberately quiet when it is unsure. A file whose parse or
preprocessing failed gets hover and go-to-definition and **no** semantic errors;
a file with an `#extension` line gets no unknown-name errors, because it is
using something no table here models; and a `gl_`-prefixed or vendor-suffixed
name is never "unknown". A false squiggle costs more than a missed one.

[docs.gl]: https://docs.gl

### Which stage, and which version

GLSL has no way to say, in the file, which stage it is. The stage is taken from,
in order:

1. `#pragma shader_stage(vertex | fragment | compute | geometry | tesscontrol | tesseval)`,
   which `glslc` also reads;
2. the file extension (`.vert`, `.frag`, `.comp`, `.geom`, `.tesc`, `.tese`, and
   the usual variants);
3. the stage-exclusive built-ins the source uses (`gl_FragColor` → fragment).

The first two are a *declaration* and the third is a **guess**. A guess never
produces an error — it only makes hovers and completion better — and the status
bar marks it with a `?`. A code action offers to write the guess down as a
`#pragma`.

The version comes from `#version`, and everything above depends on it: which
built-ins exist, which overloads hover shows, which names completion offers.

A file that declares no `#version` is GLSL 1.10 by the specification, and that is
what the extension does with it. It is rarely what the author meant. Shader
fragments concatenated at runtime, and files compiled with the version supplied
on the command line, are ordinary practice — and holding them to 1.10 reports
every modern name as missing. Set **`glsl.defaultVersion`** to what those files
really are, written the way the directive is:

```jsonc
"glsl.defaultVersion": "300 es"   // or "450", or "330 core"
```

It applies only to files that declare nothing; a `#version` line in the file
always wins. There is also a code action, *Add `#version 450`*, for when the
right answer is to make the file say so itself.

### If you write Vulkan GLSL

You get the same treatment as everyone else. Earlier versions of this extension
validated GLSL with naga's front end, which implements **Vulkan** GLSL at
`#version 440`/`450`/`460` only — so a WebGL shader, or an OpenGL one with
combined `sampler2D` uniforms and driver-assigned bindings, was highlighted and
never checked, and `glsl.validate.dialect` existed to control the skipping.

That is gone. Every dialect is checked now, by one analyzer, and the setting
with it.

## WESL: WGSL with modules

[WESL] is WGSL plus what a shader project grows into: `import`, module paths,
and conditional translation with `@if`, `@elif` and `@else`. A `.wesl` file gets
everything a `.wgsl` file does, and its imports are followed across files:

```wesl
import package::lighting::{attenuate, Light};
import package::lights::point;

@if(high_quality) const SAMPLES: u32 = 16u;
@else             const SAMPLES: u32 = 4u;

fn shade(light: Light, position: vec3f, normal: vec3f) -> vec3f {
    return point::shade(light, position, normal) * attenuate(length(position));
}
```

- **Go to definition** on `attenuate`, `Light` or `point::shade` lands on the
  declaration in the file it comes from — opened or not. On a module name
  (`lighting`, `point`) it opens that module's file.
- **Hover** shows the imported declaration's signature, the module it is from,
  and its doc comment.
- **Completion** after `::` lists what that module holds — its submodules and
  its declarations. Straight after `import` it offers `package`, `super` and the
  dependencies `wesl.toml` names. Imported names complete as what they are, a
  function as a function.
- **Diagnostics** report an import that names nothing, on the segment that
  fails: ``cannot find `attenuat` in `package::lighting` ``. Declarations of the
  same name under `@if`/`@else` are not duplicates. And naga type-checks the
  file with what it imports linked in — see below.
- **Semantic highlighting** paints an imported function as a function and a
  module as a namespace.

### How a path finds its file

A WESL file is a module named by its path from the package root:
`shaders/lights/point.wesl` is `package::lights::point`.

- **`package::`** starts at the root `wesl.toml` names — `root = "./shaders"`,
  which is also the default. The nearest `wesl.toml` above a file decides its
  package. With none, the file's own folder is the root.
- **`super::`** is the module the file sits in, and each further `super` climbs
  one more. Climbing out of the package is an error; outside any package it is
  allowed.
- **A path starting with a name** — `import noise::perlin;` — is a dependency.
  One listed in `wesl.toml` with a `path` is followed into that folder, which is
  read through its own `wesl.toml`. A dependency from a package manager is not
  something the extension can see, so an import from one is left alone: never
  followed, never flagged.

When `lighting.wesl` and `lighting.wgsl` both exist, `lighting.wesl` is the
module.

### What is checked, and what is not

naga reads one WGSL module and knows nothing of imports or `@if`. So a `.wesl`
file that uses either is **linked** first, the way a WESL linker would: the
file, plus every declaration it reaches through its imports — and what those
reach in turn — renamed so names from different modules cannot clash.
Conditional translation picks the branches the `wesl.features` setting says.
naga validates the result, types and all.

- A problem in the file is reported where it is, and names an imported
  declaration by its path: ``no definition in scope for identifier:
  `package::lib::gone` ``.
- A problem inside an imported module is reported on the use that pulled it
  in, as ``in `package::lights::point`: …``, with a link to where it actually
  is.
- Only one configuration is checked: code a condition removes is not. Set
  `wesl.features` to check another — `{ "high_quality": true }`.
- Nothing is linked while an import names nothing, or reaches into a
  dependency from a package manager, or a condition is more than feature
  names, `!`, `&&`, `||` and parentheses. The file then gets syntax checks and
  import resolution, and no type checking, rather than errors about code the
  extension cannot see.
- Hover and completion do not see imported types yet: `light.` after an
  imported `Light` offers nothing.

A `.wesl` file that uses no WESL syntax is WGSL, and naga validates it as it
stands.

WESL reads the `wgsl.*` settings, plus `wesl.features`.

[WESL]: https://wesl-lang.dev

## Settings

Every setting exists under both `wgsl.` and `glsl.`. WESL files read the
`wgsl.` ones.

| Setting | Default | |
| --- | --- | --- |
| `validate.onSave` | `true` | Validate when the file is saved. |
| `validate.onType` | `false` | Validate on every keystroke. Off because a file is invalid for most of the time it is being edited. |
| `completion.enabled` | `true` | |
| `semanticTokens` | `true` | Colour by resolved meaning. |
| `inlayHints.enabled` | `false` | |
| `inlayHints.types` | `true` | Inferred types on bindings that declare none (WGSL); the deduced size of an implicitly sized array (GLSL). |
| `inlayHints.parameterNames` | `true` | `mix(e1: a, e2: b, e3: t)`. |
| `format.enable` | `false` | The re-indenter. |
| `format.indentWidth` | `4` | |
| `embedded.enabled` | `true` | Language features inside tagged string literals. |
| `embedded.diagnostics` | `false` | Validate embedded shaders too. |

Plus four of their own:

| Setting | Default | |
| --- | --- | --- |
| `glsl.defaultVersion` | `""` | The `#version` to analyse a file that declares none as — `450`, `330 core`, `300 es`. Empty follows the specification, which says 1.10. See [above](#which-stage-and-which-version). |
| `glsl.showStageInStatusBar` | `true` | Shows the version and stage, with a `?` when the stage had to be guessed. |
| `wgsl.rust.highlightHint` | `true` | Offer the rust-analyzer fix described above. |
| `wesl.features` | `{}` | The features `@if`, `@elif` and `@else` test when a WESL file is checked. One left out is false. See [above](#what-is-checked-and-what-is-not). |

`glsl.validate.dialect` was removed: it existed only to switch naga's
Vulkan-only GLSL front end off for sources it could not read, and nothing skips
validation any more. A setting left over in your `settings.json` is ignored.

## Commands

| Command | |
| --- | --- |
| `WGSL / WESL: Validate Current File` | Validate now, whatever `validate.onSave` says. |
| `GLSL: Validate Current File` | The same. |
| `GLSL: Show Shader Stage of Current File` | Which stage, and why. |
| `WGSL / GLSL: Restart Language Server` | |

## Languages and file types

**WGSL** — `.wgsl`.

**WESL** — `.wesl`, with `wesl.toml` read for the package root and path
dependencies.

**GLSL** — `.glsl`, `.vert`, `.frag`, `.comp`, `.geom`, `.tesc`, `.tese`,
`.vsh`, `.fsh`, `.gsh`, `.vshader`, `.fshader`, `.gshader`, `.glslv`, `.glslf`,
`.vertexshader`, `.fragmentshader`.

## How it runs

The server is a child process, not part of the extension host: a shader
compiler is not the kind of thing to run in-process, and a panic on a malformed
module costs a restart rather than every extension in the window.

Analysis is Rust compiled to WebAssembly — one artifact for every platform VS
Code runs on, with no toolchain to install and nothing to download on first use.

## License

`NO LICENSE`. The third-party notices — the Khronos Group GLSL reference prose
embedded as hover documentation, and the Rust dependency graph linked into the
WASM, naga included — are in `LICENSE.md` and `THIRD-PARTY-NOTICES.md`, both
shipped in the package.
