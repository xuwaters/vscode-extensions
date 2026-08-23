# Measurements

**Taken**: 2026-08-23, on the machine that built the artifact (Apple Silicon,
Node 24.17, wasm-pack 0.15, release profile with `wasm-opt -Os`). Method:
`FAST_MEASURE=1 pnpm vitest run test/measure.test.ts` in
`extensions/fast-element-ultra`, plus one fresh-process timing. These close
what [spikes.md](spikes.md) could close without a desktop; what remains open
is listed at the bottom.

Every number here supersedes the *absence* of numbers the RFC shipped with.
[proposal.md §1.3 and §9](../proposal.md) were checked against them — see the
verdicts.

## Budget 2 — artifact, instantiation, memory

| | Measured |
| --- | --- |
| Artifact size | **588,057 bytes** raw, **213,222 bytes** gzipped |
| Fresh-process `require` + WebAssembly compile | **1.7 ms** |
| First `Engine` + config | **0.7 ms**; subsequent engines ~0.03 ms |
| RSS growth over 1,000 upsert+analyze cycles (2.6 KB template) | **none observed** (586 MB → 586 MB test-process RSS; the engine's share is not separable at this scale, which is itself the result: no growth trend) |
| Per-cycle cost of upsert+parse+analyze | **0.059 ms** |

For scale: typst-ultra's artifact is ~19 MB. This engine is a parser, tables
and rules, and the size says so.

**Instance model** (open question 5): `wasm-pack --target nodejs` instantiates
one WebAssembly instance per process at `require` time; per-project `Engine`
objects are separate Rust registries inside that one instance. Two projects
cannot see each other's registries; they do share a heap, so a trap poisons
every project's engine at once — accepted, recorded in
[0011](../decisions/0011-containment-is-the-plugins-try-catch.md).

## Budget 1 — cold and warm diagnostics

Measured through the full pipeline the plugin runs — extraction, engine
analysis, binding facts answered by the checker, CSS service — on the largest
corpus file, `pdf-ultra/webview/viewer/template.ts` (445 lines, 11 documents):

| | Measured |
| --- | --- |
| Cold `getSemanticDiagnostics`, including building the whole `ts.Program` from nothing | **383 ms** |
| The same, warm (program built, extraction cached) | **0.73 ms** |

The cold number is dominated by program construction and type checking, which
inside tsserver has already happened — the plugin never pays it. The number
that maps to a keystroke is the warm one, and **0.73 ms needs no wall-clock
budget**, which is the behaviour [proposal.md §9](../proposal.md#9-success-criteria)
criterion 5 asked for. lit-analyzer's 150 ms bail-out has no analogue here and
none was implemented.

**Not measured**: the side-by-side against `temp/fast-analyzer` with its tag
extraction patched. The `temp/` tree is not present in this checkout, so the
comparison spikes.md wanted has no subject; the absolute criterion is met
regardless, and [spikes.md](spikes.md) itself says the absolute number is the
one that matters.

## Budget 3 — type-oracle round trip

Binding facts per file over the corpus, one engine crossing per document each
way (decision [0002](../decisions/0002-rust-engine-typescript-oracle.md)):

| File | Facts | Documents |
| --- | ---: | ---: |
| csv-ultra template.ts | 32 | 13 |
| pdf-ultra template.ts | 60 | 11 |

The prediction held: the batch is smaller than the binding count (csv-ultra
has ~55 expression bindings; 32 reach a type rule) and there are no callbacks
in either direction. Answering the batch is included in the 0.73 ms warm
number above; it is not separable from it at this precision and does not need
to be.

## Still open

- **The real-VS Code load test** (P1-09's last step, P5-06): install the VSIX
  into a clean VS Code and read the plugin's name in the TS Server log. The
  headless approximation — extract the VSIX, resolve the plugin exactly as
  tsserver's probe does, load the factory, run the engine — is automated in
  `scripts/verify-vsix.mjs` and passes; the desktop step needs a desktop.
- **A 10× synthetic template** for the linearity check. The 1,000-edit cycle
  at 0.059 ms/cycle bounds it from below; a dedicated scaling curve was not
  taken.
- **fast-analyzer side-by-side**, per above — requires restoring `temp/`.
