# Phase 6 — Polish & release

**Goal:** ship it. **Needs:** Phase 5.

**Exit criterion:** VSIX packaged from a clean build; user/dev docs updated;
measurements and notices in place. — **met**, `wx-vsce-wgsl-shader-0.6.0.vsix`,
833,062 bytes, built after the full verification below.

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P6-01 | `examples/` refreshed to exercise the new analyzer (an ES 300 shader, an OpenGL combined-sampler shader, a Vulkan 450 shader — each clean under our diagnostics); README.md user documentation for GLSL analysis (README = product manual, per repo convention). | examples pass the no-error gate | ☑ | Added `test-es300.frag` (precision statements, a function-like macro, an `#ifdef`) and `test-opengl.frag` (combined samplers, driver-assigned bindings, `mat3` tangent frame); `test.{vert,frag,comp}` stay as the Vulkan 4.50 set. Both new files clean on first run of `the_extensions_examples_analyse_cleanly` (5 files, 0 errors) and added to `the_shipped_examples_produce_no_errors` (3 → 5) and `grammar.test.ts`. One test added: `each_shipped_example_is_analysed_as_the_dialect_it_declares` — `3.00 es` / `3.30` / `4.50`, stage known, nothing skipped. README gained a per-dialect table, a *Hover, completion and signature help* section and a fuller `glsl.defaultVersion`; a garbled signature-help row fixed |
| P6-02 | CONTRIBUTING.md dev docs: crate map, how to regenerate the spec (`temp/docs.gl` + one command), how corpus tests skip, budgets and where they are asserted. | n/a | ☑ | Crate map now names all seven with a ships-in-wasm / depends-on table; *The two analyses* became *One server, two languages, two pipelines* (the old one still had naga validating GLSL); new sections for the spec pipeline and regeneration, the four corpus gates and their `SKIP` behaviour, the four §8 budgets and the tests that assert them, and an `examples/` table that records the byte-offset constraint on `test.{vert,frag,comp}` |
| P6-03 | Third-party notices finalised (docs.gl/Khronos attribution from P1-10; confirm no other new obligations). | n/a | ☑ | cargo-about wired up the way `typst-ultra` and `fast-element-ultra` already do it: `wgsl-lsp-wasm/about.{toml,hbs}` + a `licenses` pnpm script. The Khronos section is prose *inside* `about.hbs`, so the whole file is generated and the attribution cannot drift. 59 crates: 44 Apache-2.0, 13 MIT, 1 Unicode-3.0, 1 Zlib. naga is present and the notice says why — `wgsl-in` only |
| P6-04 | Final measurements sweep into [research/measurements.md](../research/measurements.md): corpus pass rates, diagnostic counts, budgets, before/after feature comparison on the examples. | recorded numbers | ☑ | [§8](../research/measurements.md#8-the-release-sweep-p6-04). All four budgets re-measured on the shipped tree: wasm **byte-identical** at 1,834,673 (+47,336), 4.86 ms wasm, 3.43 ms native, 3.46 ms through the server, 424,200 B of generated Rust. Corpus 1,677 files / 0 panics on all three gates, 91.8 % of the GLSL files parse clean, 92.7 % analyse clean, 208-file false-positive gate at zero. §8.4 is the before/after on the three dialect examples |
| P6-05 | Version bump + CHANGELOG + `pnpm`-side wiring checked (`.vscodeignore` still correct) + VSIX packaged. | packaged VSIX opens with features live | ☑ | 0.5.1 → **0.6.0** via `node scripts/main.mjs bump --minor`; marketplace `description` no longer says GLSL is validated by naga. `CHANGELOG.md` is new — no extension in this repo had one; versions back to 0.4.0 reconstructed from the commits that set them. `.vscodeignore` byte-identical to `scripts/templates/.vscodeignore` and `sync-vscodeignore` reports no drift. `wx-vsce-wgsl-shader-0.6.0.vsix`, 20 files, 833,062 B |
| P6-06 | RFC closeout: board updated to final, open questions all closed, deferred items (§10) restated as a short list for a future RFC. | n/a | ☑ | Board at 6/6; all five research debts carry outcomes; decisions 0001–0008 all Accepted and q1–q4 all closed by a record. Deferred list at the bottom of [tasks/README.md](README.md) — proposal §10's five, plus the `PpToken`-owns-a-`String` arena from [measurements §6](../research/measurements.md#6-what-is-still-there) |

## Verification run before packaging

Every command green, in this order:

```sh
cargo run -p glsl-spec-gen -- --check                       # exit 0: tables are current
cargo test --release -p glsl-spec -p glsl-spec-gen -p glsl-syntax \
                     -p glsl-analysis -p wgsl-syntax -p wgsl-lsp-core   # 621 + 4 doctests
pnpm --filter wx-vsce-wgsl-shader typecheck                # tsc --noEmit
rm -rf extensions/wgsl-shader/wasm
pnpm --filter wx-vsce-wgsl-shader build:wasm               # 1,834,673 B, unchanged
pnpm --filter wx-vsce-wgsl-shader test                     # 69 vitest, 4.86 ms wasm budget
pnpm --filter wx-vsce-wgsl-shader licenses                 # cargo-about
pnpm --filter wx-vsce-wgsl-shader package                  # → 0.6.0.vsix
```

## Notes for whoever comes next

- **`examples/test.{vert,frag,comp}` are byte-offset fixtures.** The P3-08 outline
  parity spans in `glsl-syntax/src/tests/outline.rs` are transcriptions of what
  the deleted heuristic walk found, so they cannot be re-derived. P6-01 needed to
  drop a stale naga reference from a comment in `test.frag` and rewrote it to
  **exactly the same byte length** rather than move the spans; the module doc
  now says so. New examples are new files, which cost nothing.
- **A stale comment, fixed:** `features/code_actions.rs::add_version` still said a
  GLSL file with no `#version` "is parsed as whatever naga defaults to". Comment
  only; the behaviour (offer a fixed `#version 450`) is unchanged and is the
  right one — a file that states its version stays right whatever
  `glsl.defaultVersion` later becomes.
- **No analyzer bug was found in this phase.** Both new examples came back clean
  on their first run, which is the outcome a false-positive gate at 208 files
  predicts.
