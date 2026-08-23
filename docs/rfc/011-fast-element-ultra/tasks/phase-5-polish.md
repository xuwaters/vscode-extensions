# Phase 5 — Polish and release

**Goal**: ship it, and replace every unmeasured claim in the RFC with a measured one.
**Exit criterion**: VSIX published, and [research/spikes.md](../research/spikes.md) has no open
budget.
**Status**: ☑ 6 / 10, ◐ 3, ☐ 1

| # | Task | Status |
| --- | --- | --- |
| P5-01 | TextMate injection grammar, `:` / `?` / `@` prefixes marked | ☑ — **write-our-own** (no vendored grammar, no carried licence): `syntaxes/fast-element-html.injection.json` embeds `text.html.basic` in `html\`\`` and `source.css` in `css\`\``, re-enters `source.ts` in `${…}`, and captures binding prefixes where the HTML grammar leaves room — inside a tag the HTML grammar's own attribute rule usually wins, which colours `:prop` as an attribute rather than distinctly; noted as the accepted limit of not forking the HTML grammar |
| P5-02 | Snippets | ☑ — component skeleton, `when`, `repeat`, `ref`, `@attr`, `@observable` |
| P5-03 | README with features, settings, and the `fast-plugin.*` → `fastElementUltra.*` mapping | ☑ — including the three removed rules and the three renames |
| P5-04 | **Measurement** | ◐ — everything measurable headless is measured and in [research/measurements.md](../research/measurements.md): artifact 588 KB/213 KB gz, compile 1.7 ms, warm pass **0.73 ms**, cold-with-program 383 ms, fact batches 32/60, no growth over 1,000 edits at 0.059 ms/cycle; proposal §1.3/§9 checked (the per-keystroke claim holds; no bail-out exists or is needed). Open: the fast-analyzer side-by-side (`temp/` absent) and a 10× synthetic scaling curve. Rerun: `FAST_MEASURE=1 pnpm vitest run test/measure.test.ts` |
| P5-05 | Third-party notices | ☑ — `cargo about` for the Rust side (`pnpm run licenses`), LICENSE.md for the bundled JS (`vscode-css-languageservice`, `vscode-languageserver-textdocument`) and the embedded web-custom-data; no vendored grammar to license |
| P5-06 | Test the packaged artifact | ◐ — `pnpm run verify:vsix` extracts the built VSIX, resolves the plugin exactly as tsserver's probe does, loads the factory against real TypeScript, and runs the engine; it is the P1-09 gate as a repeatable check and fails when vsce collection, the zip injection, or the probe layout changes. The clean-desktop install + TS Server log read remains manual |
| P5-07 | CI: cargo test, clippy, the differential test, the vitest suite, a wasm32 build | ☑ — the fast crates joined `.github/workflows/ci.yml`'s clippy scope and wasm32 build; `cargo test --workspace` already runs the 89 Rust tests; the vitest suites (differential included) run under `pnpm test` and skip themselves when `wasm/` is absent, the repo's established arrangement. The repo does not run `cargo fmt`, and neither does anything here |
| P5-08 | Error-path polish: poisoned, out-of-range TypeScript, `html.partial` | ☑ — the status item says poisoned/disabled/out-of-range; the version gate logs the range it wanted; a partial template gets a suggestion-severity "not analyzed" diagnostic instead of half-checked silence |
| P5-09 | Run against a FAST codebase that is not ours | ☐ — not done. Candidates remain the FAST repo's `examples/`, `fast-router`, `fast-ssr`. The corpus-author bias the task warns about is real and this is the task that would find it |
| P5-10 | Publish, and update the RFC status | ◐ — `wx-vsce-fast-element-ultra-0.1.0.vsix` is built, injected, verified, and in the extension directory (the repo's convention for its private extensions); RFC docs updated. Marketplace publication and the desktop install check are the remainder |

## Notes

**P5-04's honesty check ran.** [proposal.md §1.3](../proposal.md#13-why-now-and-why-here)'s argument
was that a native engine removes the pressure the 150 ms bail-out exists for; the measurement (0.73 ms
warm, full pipeline) supports it, and no bail-out was implemented. What the measurement *cannot* say
without `temp/` restored is how much of that is Rust versus how much fast-analyzer's TypeScript would
also have managed — the fallback paragraph in §8 stays honest rather than hypothetical-retired.

**P5-09 is the known debt.** Every template this analyzer has ever seen was written by the person who
wrote the analyzer.
