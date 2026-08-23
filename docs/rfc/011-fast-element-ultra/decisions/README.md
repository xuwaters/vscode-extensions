# Decisions

Architectural decision records for RFC 011. Append-only in spirit: a reversed decision gets a new
record that supersedes the old one, and the old one keeps its history.

Every record names the observable condition that would make it wrong. A decision without a
"revisit if" is a preference.

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-tsserver-plugin-not-lsp.md) | Ship a TypeScript server plugin, not a language server | Accepted |
| [0002](0002-rust-engine-typescript-oracle.md) | Rust owns the template, TypeScript owns the types, and they meet once per file | Accepted |
| [0003](0003-own-template-parser.md) | Write the template parser rather than bind an existing one | Accepted, gated |
| [0004](0004-drop-web-component-analyzer.md) | Drop `web-component-analyzer` and write a FAST component model | Accepted |
| [0005](0005-css-stays-in-typescript.md) | `css` templates keep using `vscode-css-languageservice` | Accepted |
| [0006](0006-wasm-inside-tsserver.md) | Run the WASM inside tsserver, not in a child process | Accepted |
| [0007](0007-fast-rule-semantics.md) | Rules describe FAST's semantics, even when that means dropping an inherited one | Accepted |
| [0008](0008-naming-and-config.md) | `wx-vsce-fast-element-ultra`, `fastElementUltra.*`, no migration from `fast-plugin.*` | Accepted |
| [0009](0009-no-lit-compatibility.md) | No lit support, in any form | Accepted |

## Open questions

Things this RFC does not decide. Each names what closes it.

| # | Question | Closed by |
| --- | --- | --- |
| 1 | Can `.vscodeignore` negation keep `node_modules/wx-fast-element-tsplugin/` in a VSIX built with `--no-dependencies`? | [P1-09](../tasks/phase-1-foundation.md) — **blocking**. [gate 1](../research/spikes.md#gate-1--can-the-plugin-be-packaged-at-all) |
| 2 | Our own parser, or `swc_html_parser`? | [P1-05](../tasks/phase-1-foundation.md) decides on the differential result, not on preference. [gate 2](../research/spikes.md#gate-2--does-our-parser-match-parse5) |
| 3 | What TypeScript version range do we claim, and how do we degrade outside it? | [P1-08](../tasks/phase-1-foundation.md). fast-analyzer tests four versions; whatever we claim, we test |
| 4 | Should `no-non-reactive-binding` default to `warning` or `error`? | [P3-06](../tasks/phase-3-diagnostics.md), after running it over the corpus and over a codebase that uses one-time bindings deliberately |
| 5 | One WASM instance per tsserver project, or one shared instance keyed by project? | [P5-04](../tasks/phase-5-polish.md), on the memory measurement. Per-project is the starting assumption because it cannot leak across projects |

Question 1 is the only one that can invalidate the design. It is the first task in Phase 1 for that
reason: if the plugin cannot be packaged, nothing else about the plan survives contact.
