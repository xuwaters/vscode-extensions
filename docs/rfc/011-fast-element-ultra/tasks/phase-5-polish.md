# Phase 5 — Polish and release

**Goal**: ship it, and replace every unmeasured claim in the RFC with a measured one.
**Exit criterion**: VSIX published, and [research/spikes.md](../research/spikes.md) has no open
budget.
**Status**: ☐ 0 / 10

| # | Task | Status |
| --- | --- | --- |
| P5-01 | TextMate injection grammar: HTML inside `` html` ` ``, CSS inside `` css` ` ``, and FAST's `:` / `?` / `@` prefixes marked distinctly. Decide vendor-with-licence (as fast-analyzer does with `vscode-lit-html`, MIT) versus write-our-own, and record it. [features.md §14](../design/features.md#14-syntax-highlighting) | ☐ |
| P5-02 | Snippets for the common shapes: a component skeleton, `when`, `repeat`, `ref`, an `@attr` member | ☐ |
| P5-03 | README with the feature list, the settings table, and a `fast-plugin.*` → `fastElementUltra.*` mapping for anyone moving over — including the three rules that no longer exist and the three that were renamed. [0008](../decisions/0008-naming-and-config.md) | ☐ |
| P5-04 | **Measurement.** Cold and warm diagnostic cost against fast-analyzer on the same input (with its tag extraction patched so it is doing comparable work); WASM artifact size, instantiation cost, resident heap, growth over 1,000 edits; per-project versus shared instances. Closes budgets 1 and 2 and [open question 5](../decisions/README.md#open-questions). Results into `research/`, then **check §1.3 and §9 of the proposal against them** | ☐ |
| P5-05 | Third-party notices: `cargo about` for the Rust side plus the JavaScript dependencies the plugin bundles (`vscode-css-languageservice`, `ts-simple-type`), and any vendored grammar's licence | ☐ |
| P5-06 | Test the packaged artifact: install the built VSIX into a clean VS Code, confirm the plugin loads, run the corpus checks against it. The P1-09 gate re-run as a permanent test rather than a one-off | ☐ |
| P5-07 | CI: `cargo test`, `cargo clippy`, the differential parser test, the extension's vitest suite, and a `wasm32-unknown-unknown` build. Note the repo does **not** run `cargo fmt` | ☐ |
| P5-08 | Error-path polish: what the user sees when the engine is poisoned, when the TypeScript version is out of range, and when a template uses `html.partial(…)` and is therefore not analysed. Each should say so, not fail silently | ☐ |
| P5-09 | Run the whole thing against a FAST codebase that is not ours, and fix what it finds. The corpus is 961 lines and everything in it was written by one person; that is not a sample | ☐ |
| P5-10 | Publish, and update [README.md](../README.md) and [tasks/README.md](README.md) with the real status | ☐ |

## Notes

**P5-04 is the task that makes this RFC honest.** Everything about performance in
[proposal.md](../proposal.md) is currently an argument from the shape of the workload, and the
documents say so in three places. When this task produces numbers, the claims get checked against
them — **including the possibility that they do not hold**. If the engine is not meaningfully faster
than fast-analyzer's TypeScript, that goes in `research/`, §1.3 gets amended, and the
TypeScript-fallback paragraph in [§8](../proposal.md#8-alternatives-considered) stops being
hypothetical.

**P5-09 is the one that will actually find bugs.** A corpus written by the person writing the linter
is a corpus that avoids the constructs the linter is bad at, without anyone deciding to. Candidates:
the FAST repo's own `examples/`, or the `fast-router` and `fast-ssr` packages.

**P5-06 exists because P1-09 is a gate, and gates rot.** The packaging arrangement is fragile — it
depends on `.vscodeignore` generation, vsce's dependency handling, and tsserver's probe paths, none
of which are ours. It needs a test that fails when any of them changes.
