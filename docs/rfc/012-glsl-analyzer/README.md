# RFC 012 — A full GLSL analyzer of our own

Replace the heuristic GLSL walk + Vulkan-only naga validation in `extensions/wgsl-shader`
with a self-written analyzer: real preprocessor, real parser, real semantics, and a
builtin spec generated from docs.gl and embedded as Rust tables.

| File / folder | What it is |
| --- | --- |
| [proposal.md](proposal.md) | The RFC: motivation, goals, architecture, phases, risks, budgets |
| [tasks/](tasks/README.md) | **Source of truth for progress.** Board + one file per phase |
| [design/](design/) | Design documents; written as their phase starts, linked from tasks |
| [decisions/](decisions/README.md) | Decision records; open questions live in its README until closed |
| [research/](research/references.md) | Reference-repo study notes, docs.gl survey, measurements |

## Ground rules for anyone (human or agent) executing this RFC

1. `temp/glslang` and `temp/glsl_analyzer` are **read-only references — never copy their
   code**, only learn from it. `temp/glslang/Test/` may be read in place by corpus tests
   (skip when absent), never copied into the repo.
2. Update the phase task file in the same change as the work. A task is done when its
   tests are green, not when the code exists.
3. House rules apply: no `cargo fmt` (crates are hand-formatted), tests live in-tree as
   real crate tests, generated files are committed with their generator.
4. Work stays inside `crates/glsl/*` plus the named integration points in
   `crates/wgsl/wgsl-lsp-core`; WGSL behaviour must not change.
