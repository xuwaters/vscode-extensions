# Decisions

Architectural decision records for RFC 011. Append-only in spirit: a reversed
decision gets a new record that supersedes the old one, and the old one keeps
its history.

Every record names the observable condition that would make it wrong. A
decision without a "revisit if" is a preference.

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-tsserver-plugin-not-lsp.md) | Ship a TypeScript server plugin, not a language server | Accepted, implemented |
| [0002](0002-rust-engine-typescript-oracle.md) | Rust owns the template, TypeScript owns the types, and they meet once per file | Accepted, implemented — amended by [0010](0010-checker-not-ts-simple-type.md) |
| [0003](0003-own-template-parser.md) | Write the template parser rather than bind an existing one | Accepted — **gate passed**: the differential suite (25 cases + every corpus template) matches parse5 with the divergences enumerated |
| [0004](0004-drop-web-component-analyzer.md) | Drop `web-component-analyzer` and write a FAST component model | Accepted, implemented |
| [0005](0005-css-stays-in-typescript.md) | `css` templates keep using `vscode-css-languageservice` | Accepted, implemented |
| [0006](0006-wasm-inside-tsserver.md) | Run the WASM inside tsserver, not in a child process | Accepted, implemented — containment mechanism corrected by [0011](0011-containment-is-the-plugins-try-catch.md) |
| [0007](0007-fast-rule-semantics.md) | Rules describe FAST's semantics, even when that means dropping an inherited one | Accepted, implemented |
| [0008](0008-naming-and-config.md) | `wx-vsce-fast-element-ultra`, `fastElementUltra.*`, no migration from `fast-plugin.*` | Accepted, implemented |
| [0009](0009-no-lit-compatibility.md) | No lit support, in any form | Accepted, implemented |
| [0010](0010-checker-not-ts-simple-type.md) | The type oracle uses the checker itself, not ts-simple-type | Accepted |
| [0011](0011-containment-is-the-plugins-try-catch.md) | Panic containment lives in the plugin's try/catch, not in catch_unwind | Accepted |

## Open questions — all closed

| # | Question | Answer |
| --- | --- | --- |
| 1 | Can `.vscodeignore` negation keep the plugin in a VSIX built with `--no-dependencies`? | **No — structurally.** vsce's file collection globs with `ignore: 'node_modules/**'` before `.vscodeignore` is consulted, so nothing under node_modules is ever offered to the ignore rules. The gate's **fallback 3** ships: `scripts/inject-tsplugin.mjs` rewrites the VSIX after packaging, and `scripts/verify-vsix.mjs` proves the result resolves exactly as tsserver's probe does. The per-extension `.vscodeignore-extra` mechanism was added to `sync-vscodeignore` for the rest of the exception |
| 2 | Our own parser, or `swc_html_parser`? | **Our own** — [gate 2](../research/spikes.md#gate-2--does-our-parser-match-parse5) passed on the differential evidence (`test/parser-differential.test.ts`): every corpus template and the adversarial set match parse5, with the deliberate divergences (unclosed stays unclosed, no synthesized `tbody`, placeholders first-class) asserted as our behaviour |
| 3 | What TypeScript version range, and how do we degrade? | **>= 5.5, < 8**, declared in the plugin and tested (`test/smoke.test.ts`): outside the range the factory logs why and returns the language service untouched. Exercised against the workspace's TypeScript 6 |
| 4 | `no-non-reactive-binding`: `warning` or `error`? | **`warning` normal / `error` strict**, with the rule narrowed to make that safe: it fires only on identifier and property-access expressions whose type has no call signatures and whose symbol is not a `const` — a call like `` `${shortcut('a','b')}` `` (csv-ultra, six occurrences) is a deliberate one-time interpolation and is exempt. That narrowing is what let the corpus gate stay silent; see [design/rules.md](../design/rules.md#no-non-reactive-binding) |
| 5 | Per-project WASM instances, or one shared? | **One instance per process, one `Engine` (registry) per project** — what `wasm-pack --target nodejs` gives. Registries are isolated; the heap is shared, so poisoning is process-wide ([0011](0011-containment-is-the-plugins-try-catch.md)). Memory numbers in [research/measurements.md](../research/measurements.md) show no growth pressure |
