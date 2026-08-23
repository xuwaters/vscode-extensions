# Phase 1 — Foundation

**Goal**: the parser, the WASM artifact, and a plugin skeleton that actually loads.
**Exit criterion**: an installed VSIX whose plugin is loaded by a real tsserver and whose engine
parses a template — and a parser that matches parse5 on the corpus.
**Status**: ☑ 9 / 10, ◐ 1 (P1-09's clean-VS Code log check is the one remaining manual step)

This phase contains no user-visible feature on purpose. It contains the two questions that can
invalidate the design, and answering them first is cheaper than answering them in Phase 4.

| # | Task | Status |
| --- | --- | --- |
| P1-01 | Workspace scaffolding: `crates/fast/*` with the four crate skeletons, root `Cargo.toml` members and exclude, `extensions/fast-element-ultra` with `package.json`, `tsconfig.json`, `tsdown.config.mts`, and a `.vscodeignore` from `pnpm sync-vscodeignore`. Naming per [0008](../decisions/0008-naming-and-config.md) | ☑ — plus the `.vscodeignore-extra` mechanism `sync-vscodeignore` needed to grow for P1-09 |
| P1-02 | `fast-template-syntax`: tokenizer. Tags, attributes with modifier/name/value spans, quotes, text, comments, doctype. Placeholder recognition in all three positions. Unit tests per token kind | ☑ — 31 unit tests |
| P1-03 | `fast-template-syntax`: tree builder. Nesting, unclosed-tag reporting *without* auto-closing, void elements, raw-text elements, foreign content where self-closing is legal | ☑ |
| P1-04 | Virtual-document round trip: the substitution model, implemented on the TypeScript side, with a property test that every offset maps back to the same source character | ☑ — `test/virtualdoc.test.ts`, 200 generated templates. One model addition: the engine converts UTF-16 ⇄ UTF-8 at its edge, so the plugin never converts (`documents.rs`) |
| P1-05 | **Gate**: differential test against parse5 | ☑ — **passed**; `test/parser-differential.test.ts`, divergences enumerated as our asserted behaviour. lit-analyzer's own fixtures not replayed (`temp/` absent from the checkout). Decides open question 2: own parser stays |
| P1-06 | `fast-analyzer-wasm`: `Engine` with `upsertFile` / `removeFile` / `setConfig` / `analyze` / `query`, JSON in and out. `wasm-pack build --target nodejs` wired into `pnpm build:wasm` | ☑ — artifact 588 KB / 213 KB gzipped |
| P1-07 | Panic containment, **tested by deliberately panicking** | ☑ — and the test against the real artifact **corrected the design**: `catch_unwind` catches nothing under wasm32's `panic = "abort"`; containment is the plugin's try/catch + poisoning ([0011](../decisions/0011-containment-is-the-plugins-try-catch.md)). `test/smoke.test.ts` panics twice and asserts TypeScript's own diagnostics still flow |
| P1-08 | Plugin skeleton: `create(info)`, `decorateLanguageService`, config via `onConfigurationChanged`, structured logging, TypeScript version range | ☑ — range **>=5.5 <8**, degrade-to-untouched tested. Closes open question 3 |
| P1-09 | **Gate, blocking**: the plugin inside a VSIX built with `--no-dependencies` | ◐ — **fallback 1 is impossible** (vsce globs with `ignore: 'node_modules/**'` before `.vscodeignore` is read); **fallback 3 ships**: `scripts/inject-tsplugin.mjs` rewrites the VSIX, `scripts/verify-vsix.mjs` extracts it and resolves the plugin exactly as tsserver's probe does, loads the factory, runs the engine — automated and passing. Remaining: install into a clean desktop VS Code and read the plugin's name in the TS Server log |
| P1-10 | End-to-end smoke test: plugin loads the real `.wasm`, engine parses a corpus template and returns its tree | ☑ — `test/smoke.test.ts` (assembled plugin) + the differential suite (trees over every corpus template) |

## Notes

**P1-09's answer changes the mechanism, not the model.** tsserver's probe finds the injected
directory exactly as it would a vsce-packed one; what failed is vsce's willingness to pack it, which
the RFC predicted might happen and budgeted a fallback for. The arrangement is fragile in the ways
P5-06 predicted, and `verify:vsix` is the test that fails when any of its three moving parts
(vsce collection, zip injection, tsserver probe layout) changes.

**P1-07 is the phase's best argument for testing against artifacts.** The native tests passed and
were wrong; the artifact test failed and was right.
