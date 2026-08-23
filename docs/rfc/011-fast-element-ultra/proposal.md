# RFC 011: FAST Element Ultra — a Rust analysis engine for FAST Element templates

**Status**: Proposed
**Date**: 2026-08-22
**Extension name**: `wx-vsce-fast-element-ultra` (`extensions/fast-element-ultra`, new)
**TS server plugin**: `wx-fast-element-tsplugin` (shipped inside the extension, new)
**Rust crates**: `crates/fast/{fast-template-syntax,fast-html-data,fast-analyzer-core,fast-analyzer-wasm}` (all new)
**References**: [`temp/fast-analyzer`](../../../temp/fast-analyzer) — a fork of
[lit-analyzer](https://github.com/runem/lit-analyzer) 2.0.3, MIT;
[`temp/microsoft-fast`](../../../temp/microsoft-fast) — `@microsoft/fast-element` 3.0.2, MIT
**Affected components**: `extensions/fast-element-ultra` (new), `crates/fast/` (new), root
`Cargo.toml` (workspace members)

---

## 1. Motivation

[FAST Element](https://github.com/microsoft/fast) is a web-component library with a tagged-template
syntax that looks like HTML and is not HTML:

```ts
const template = html<CsvGrid>`
  <div class="chrome">
    <button class="btn ${x => (x.hasHeader ? 'on' : '')}"
            aria-pressed="${x => String(x.hasHeader)}"
            ?disabled="${x => x.locked}"
            @click="${x => x.toggleHeader()}">
      ${when(x => x.busy, html`<span class="spinner"></span>`)}
    </button>
    <input ${ref('findInput')} :value="${x => x.query}" />
  </div>
`;
```

Inside that backtick, TypeScript sees one string. It will not tell you that `?disabled` expects a
boolean, that `aria-pressd` is a typo, that `<butto>` never closed, that `ref('findInput')` names a
property that no longer exists, or that `<csv-grid>` is used in a file that never imported it. All of
those are ordinary mistakes, and all of them currently surface at runtime as a control that silently
does nothing.

### 1.1 What exists today

The only tool that covers this is [`temp/fast-analyzer`](../../../temp/fast-analyzer) — our own fork
of lit-analyzer with FAST support bolted on. It works, and everything good about it came from
lit-analyzer. That is also the problem:

| | |
| --- | --- |
| Total library code inherited | **~11,700 lines** of lit-oriented TypeScript ([research/parity.md](research/parity.md)) |
| FAST-specific code added | **~1,000 lines** across 46 files, one of which is the analyzer |
| The FAST component analyzer | **250 lines**, one file, `flavors/fast-element-analyzer.ts` |
| Rules whose semantics are lit's, not FAST's | at least 5 of 23 ([design/rules.md](design/rules.md)) |
| Upstream relationship | a fork of an MIT project, rebased by hand |

The fork carries the whole of lit: `lit-html` directive tables, Polymer's `foo$=` legacy syntax,
`@property({type: Boolean})` reflection rules, `@internalProperty` visibility rules,
`web-component-analyzer` (2.9 MB, built to discover LitElement) — none of which describe FAST. Every
one of those is code we maintain, ship, and must reason around when a FAST behaviour differs.

### 1.2 The bolt-on does not fit

This is not a theoretical complaint. **Run fast-analyzer on this repository and it finds zero FAST
components.** Three extensions here — csv-ultra, pdf-ultra, typst-ultra — ship 26 typed FAST
templates and 16 files that import `@microsoft/fast-element`
([research/corpus.md](research/corpus.md)). All three register their element the same way:

```ts
export const CSV_GRID_TAG = 'csv-grid';

@customElement({ name: CSV_GRID_TAG, template, styles })
export class CsvGrid extends FASTElement { … }
```

and fast-analyzer's tag extraction requires a string literal:

```ts
// temp/fast-analyzer/packages/lit-analyzer/src/lib/analyze/flavors/fast-element-analyzer.ts:95
if (nameProp != null && ts.isStringLiteralLike(nameProp.initializer)) {
    return nameProp.initializer.text;
}
```

A `const` reference is not a string literal, so `analyzeClassDeclaration` returns `undefined`, no tag
is registered, and every downstream feature — completion, hover, go-to-definition, unknown-attribute
checking — is silently unavailable for the code this repo actually writes.

That single bug is a two-line fix. The reason it is worth putting in an RFC is that it is
*representative*. The FAST analyzer is 250 lines against a component model with three registration
forms, five member-declaration forms, and an inheritance chain, and it handles roughly one of each
([design/component-model.md](design/component-model.md#5-what-fast-analyzer-covers-today)):

| FAST construct | fast-analyzer |
| --- | --- |
| `@customElement({ name: "literal" })` | ✅ |
| `@customElement({ name: CONST })` | ❌ — the case this repo uses, 3 of 3 |
| `@customElement("my-tag")` | ⚠️ handled in the plugin's rename path, not in the analyzer |
| `FASTElement.define(Type, …)` / `MyEl.define(…)` | ❌ |
| `@attr` / `@observable` on a property | ✅ |
| `@attr` / `@observable` on a getter/setter | ❌ — `isPropertyDeclaration` only |
| `@volatile` | ❌ |
| `attributes: [...]` in the definition | ❌ |
| Members inherited from a base class or mixin | ❌ |
| `@slot` / `@fires` / `@csspart` / `@cssprop` JSDoc | only when `web-component-analyzer` recognises the class — it does not recognise FAST |
| `this.$emit("name")` | ✅ |

And in at least one place the inherited lit semantics are actively **wrong** for FAST. FAST's binding
engine removes an attribute when the bound value is `null` or `undefined`; lit's coerces it to the
string `"null"`, which is why lit has a `no-nullable-attribute-binding` rule. The fork deals with
this twice: it forces the rule's default severity to `off` in both normal and `strict` mode, *and* it
patches `stripNullAndUndefined` into the shared attribute-binding assignability path
([`is-assignable-in-attribute-binding.ts:19`](../../../temp/fast-analyzer/packages/lit-analyzer/src/lib/rules/util/type/is-assignable-in-attribute-binding.ts)),
because `no-incompatible-type-binding` runs through the same helper and would otherwise report the
same false positive. A rule disabled in the config and neutralised in the engine is a rule that
should not have been inherited, and that is the shape every future divergence takes.

Meanwhile `no-invalid-boolean-binding` is carried in the rule-id union and the default-severity table
with `["error", "error"]`, and **no rule module implements it** — it is a dead id that still appears
in the extension's settings UI.

### 1.3 Why now, and why here

This repo has 22 extensions, twelve of them built on one pattern: a Rust crate compiled to WASM,
loaded from a TypeScript host. It also, since [RFC 009](../009-markdown-preview-ultra/proposal.md) and
[RFC 010](../010-typst-ultra/proposal.md), **writes its own webviews in FAST Element** — 371, 445 and
145 lines of template in csv-ultra, pdf-ultra and typst-ultra respectively. We are a FAST consumer
with no FAST tooling, sitting on a toolchain that is a good fit for the problem.

Template analysis is exactly the workload Rust is good at: tokenize a few kilobytes of
almost-HTML, walk a tree, run two dozen rules, and do it on every keystroke. lit-analyzer's own
source concedes the pressure — it ships a wall-clock bail-out:

```ts
// temp/fast-analyzer/packages/lit-analyzer/src/lib/analyze/constants.ts:25
export const MAX_RUNNING_TIME_PER_OPERATION = 150; // Default to small timeouts.
```

Past 150 ms, `isCancellationRequested` starts returning `true`, the analyzer's loops stop early, and
it logs the abandonment at `error` level — into a log that is `off` by default. The user gets *fewer
diagnostics on a bigger file* and no sign that anything was dropped
([research/parity.md §6](research/parity.md#6-the-150-ms-budget)). Whether a native engine removes
the pressure entirely is a claim this RFC has **not** measured; what it can say is that the budget
exists because the current implementation needs one.

## 2. Goals and Non-Goals

### Goals

1. **Feature parity with `temp/fast-analyzer`**, minus everything lit-specific. Every IDE feature it
   provides — diagnostics, completion + details, quick info, go-to-definition, find-all-references,
   rename info and locations, code fixes, closing-tag completion, folding, formatting, colour
   decorators, and the CLI-equivalent workspace analysis — provided for FAST templates.
   The per-feature contract is [design/features.md](design/features.md).
2. **A FAST-native component model.** All three registration forms, all five member-declaration
   forms, inherited and mixed-in members, `attributes:` configuration, `@volatile`, JSDoc
   `@slot`/`@fires`/`@csspart`/`@cssprop`, and tag names that come from a `const`.
   See [design/component-model.md](design/component-model.md).
3. **A rule set that describes FAST**, not lit. 26 rules: 17 carried over, 3 lit-only rules dropped,
   3 rewritten because the lit version asks the wrong question of FAST, and 6 new ones that only
   make sense for FAST — including `no-non-reactive-binding`, which catches `${x.foo}` written where
   `${x => x.foo}` was meant, a mistake with no equivalent in lit.
   See [design/rules.md](design/rules.md).
4. **The analysis engine in Rust**, compiled to one `wasm32-unknown-unknown` artifact: the template
   parser, the HTML data tables, the component registry, the rule engine, and every position-based
   IDE query. Testable natively with `cargo test`, with WASM bindings isolated in one thin crate —
   the repo's established engine/adapter split.
5. **Type checking stays in TypeScript**, because it must (§3). The boundary between the two is a
   documented, versioned protocol, not an accident — see
   [design/architecture.md](design/architecture.md).
6. **No fork.** `@microsoft/fast-element` is read as a reference and consumed as a published package.
   Nothing in `temp/` is patched, vendored, or rebased.
7. **Validated against real code**: the 26 templates and 16 files in this repo's own extensions are a
   permanent test corpus ([research/corpus.md](research/corpus.md)).

### Non-Goals

- **Any lit support at all.** Not `.prop=`, not `lit-html` directives, not `@property`, not
  `LitElement`. A user who wants lit tooling should install lit-plugin; running both is fine and
  neither will duplicate the other's diagnostics, because ours only activate on FAST templates.
  See [0009](decisions/0009-no-lit-compatibility.md).
- **Being a general web-components analyzer.** No Stencil, no Polymer, no Angular Elements, no
  `custom-elements.json` ingestion in Phase 1 (§10 lists it as a candidate follow-up).
- **Declarative FAST templates in `.html` files** — the `f-repeat` / `f-when` / `{{ }}` syntax in
  `fast-element/src/declarative`. Genuinely interesting, genuinely out of the parity scope this RFC
  is measured against. The parser crate is designed not to preclude it (§10).
- **Re-implementing the TypeScript type checker in Rust.** See §3. This is the constraint the whole
  design bends around, and it is not negotiable.
- **A CSS language service in Rust.** `css` template literals keep using
  `vscode-css-languageservice`, on the TypeScript side. See
  [0005](decisions/0005-css-stays-in-typescript.md).
- **A standalone `npx` CLI.** The "analyze the whole workspace" command runs inside the extension and
  reports into the Problems panel, rather than shelling out to a terminal as fast-analyzer does. See
  [design/features.md §12](design/features.md#12-workspace-analysis).
- **A browser (vscode.dev) build.** VS Code's TS server runs in a web worker there and the plugin
  probe mechanism differs; every other extension in this repo has the same constraint.
- **Formatting FAST templates.** We forward TypeScript's own format edits for the template range, as
  fast-analyzer does; we do not write an HTML formatter.

## 3. The constraint that shapes everything

Seven of fast-analyzer's 23 rules ask a question only the TypeScript compiler can answer:

> Given `?disabled="${x => x.locked}"` on `<csv-grid>`, is the return type of that arrow function
> assignable to the declared type of `CsvGrid.locked`?

Answering that needs structural assignability over TypeScript's type system — unions, intersections,
generics, conditional types, `strictNullChecks`, declaration merging, and the type relation cache
that makes it tractable. That is tens of thousands of lines of `checker.ts`, it has no specification,
and it changes every TypeScript release. **It cannot be reimplemented in Rust**, and any design that
pretends otherwise is a design that quietly drops seven rules.

So the type checker stays where it is, and the interesting question becomes *how little* has to stay
with it. The answer, worked out in [0002](decisions/0002-rust-engine-typescript-oracle.md):

| Concern | Where | Why |
| --- | --- | --- |
| Finding `html`/`css` tagged templates, reading decorators, resolving `const` tag names | **TypeScript** | It has the AST and the checker already; a second parser would be a second source of truth |
| Type assignability for the 7 type rules | **TypeScript** | Forced, per above |
| CSS validation and completion inside `` css` ` `` | **TypeScript** | `vscode-css-languageservice` exists and is good |
| Template parsing, tree, spans, unclosed-tag detection | **Rust** | Hot path, runs per keystroke |
| Built-in HTML/SVG element, attribute and event tables | **Rust** | Static data; generated into the binary |
| The component registry and its invalidation | **Rust** | Shared by every feature; benefits from one owner |
| The 19 rules that are not type rules | **Rust** | Structural; no checker needed |
| Completion, hover, definition, rename, code fixes, folding, closing tags | **Rust** | All are "resolve a position in the tree, then look something up" |
| Did-you-mean suggestions, name similarity | **Rust** | `strsim`, replacing `didyoumean2` |

Roughly: **TypeScript owns the compiler's knowledge; Rust owns the template's knowledge.** The line
is drawn by what only `tsc` can answer, not by preference.

The type rules do not become a stream of round-trips. Rust runs a single diagnostic pass and, where a
rule needs a type comparison, emits a **binding fact** — target tag, target member, binding kind,
expression index, span — instead of a verdict. TypeScript answers a batch of them at the end of the
pass, in TypeScript, using `ts-simple-type` exactly as fast-analyzer does. One boundary crossing per
file, no callbacks in either direction. [design/architecture.md §4](design/architecture.md#4-the-boundary-protocol)
specifies the payload.

## 4. Architecture

```
┌──────────────────────── VS Code extension host ──────────────────────────┐
│  extensions/fast-element-ultra/dist/extension.js                         │
│  · reads fastElementUltra.* settings → api.configurePlugin(…)            │
│  · colour decorators inside html`` / css``                               │
│  · "Analyze workspace" command → Problems panel                          │
└───────────────────────────────┬──────────────────────────────────────────┘
                                │ vscode.typescript-language-features API
┌───────────────────────────────▼──────────────────────────────────────────┐
│  tsserver (VS Code's, or the workspace's)                                │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │ node_modules/wx-fast-element-tsplugin/index.js   (CommonJS)        │  │
│  │  · decorates LanguageService: completions, diagnostics, quickinfo, │  │
│  │    definition, references, rename, code fixes, closing tag …       │  │
│  │  · FAST component discovery from the TS AST + checker              │  │
│  │  · answers binding-fact type queries via ts-simple-type            │  │
│  │  · CSS documents via vscode-css-languageservice                    │  │
│  │  ┌──────────────────────────────────────────────────────────────┐  │  │
│  │  │ fast_analyzer_wasm.wasm      (wasm-pack --target nodejs)      │  │  │
│  │  │  template parser · component registry · rule engine ·         │  │  │
│  │  │  completions · hover · definition · rename · code fixes       │  │  │
│  │  └──────────────────────────────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────┘
```

Three things about this diagram are decisions, each with a record:

- **It is a TypeScript server plugin, not a language server.** An LSP server would need its own
  `ts.Program` — a second copy of a type graph tsserver already holds, which on a large workspace is
  hundreds of megabytes for no new information. lit-analyzer made this call correctly.
  [0001](decisions/0001-tsserver-plugin-not-lsp.md).
- **The WASM runs inside tsserver, not in a child process.** The engine is small and its work is
  short; a child process would add an IPC hop to a per-keystroke path.
  [0006](decisions/0006-wasm-inside-tsserver.md) also names the escape hatch and what would trigger
  it.
- **The plugin ships inside the extension, under `node_modules/`,** because tsserver's plugin
  resolution requires it. That interacts awkwardly with `vsce package --no-dependencies` and is the
  first thing Phase 1 verifies — [tasks/phase-1-foundation.md](tasks/phase-1-foundation.md), task
  P1-09.

### 4.1 The virtual document

Both sides address template text in one coordinate space, and it is lit-analyzer's, because
lit-analyzer's is right: each `${…}` is replaced by an underscore run of *exactly the same length*
carrying its index in base 36.

```
source:      html`<a href=${url}>${text}</a>`
substituted:      <a href=__0_>_____1_</a>
```

Every offset in the substituted text is `templateStart + offset` in the source file, with no mapping
table. The substitution is simultaneously a valid attribute name, unquoted attribute value, and text
node, so a placeholder in any legal binding position still parses. Rust receives the substituted
text and a placeholder table; every span Rust returns is already a source-file offset once the host
adds `templateStart`. Details in [design/architecture.md §3](design/architecture.md#3-the-virtual-document).

## 5. Rust crates

Four crates under `crates/fast/`, following the grouping-directory pattern `crates/typst/` set:

| Crate | Contents | Depends on |
| --- | --- | --- |
| `fast-template-syntax` | Tolerant tokenizer and tree for interpolated HTML: elements, attributes with name/value/modifier spans, text, comments, placeholders, raw-text and foreign-content handling, unclosed-tag reporting | — |
| `fast-html-data` | Generated tables of built-in HTML/SVG elements, global attributes, DOM events, plus loading of VS Code custom-data JSON | — |
| `fast-analyzer-core` | Component registry, document store, rule engine, and every position query: completion, hover, definition, rename, code fixes, folding, closing tag, highlights | both above |
| `fast-analyzer-wasm` | `wasm-bindgen` surface; serde payloads; panic containment | `fast-analyzer-core` |

`fast-analyzer-core` never imports `wasm_bindgen`, so `cargo test` exercises the real engine on the
host. Boundaries and the reasoning behind them: [design/crates.md](design/crates.md).

## 6. Phases

Five phases, sequenced so each ends with something demonstrable. Full task list with IDs in
[tasks/README.md](tasks/README.md).

| Phase | Ends when | Tasks |
| --- | --- | --- |
| **1 — Foundation** | The template parser matches parse5 on the corpus, and an empty plugin carrying the WASM survives `vsce package` and loads in a real tsserver | [phase-1](tasks/phase-1-foundation.md) |
| **2 — Component model** | Every element in this repo's three extensions is discovered, with its members, from every registration form | [phase-2](tasks/phase-2-component-model.md) |
| **3 — Diagnostics** | All 26 rules report, including the 7 that need the type oracle, with severity and `strict` plumbed through | [phase-3](tasks/phase-3-diagnostics.md) |
| **4 — IDE features** | Completion, hover, definition, references, rename, code fixes, closing tags, folding, colours and CSS all work | [phase-4](tasks/phase-4-ide-features.md) |
| **5 — Polish and release** | Grammar, README, licences, performance budget met, VSIX published | [phase-5](tasks/phase-5-polish.md) |

Phase 1 is deliberately front-loaded with the two risks that could invalidate the design: whether our
parser can match parse5, and whether the plugin can be packaged at all.

## 7. Risks

| Risk | Severity | Mitigation |
| --- | --- | --- |
| **`vsce package --no-dependencies` strips `node_modules/`**, and tsserver only loads plugins from there | **High** — blocks shipping | Verified first, P1-09, before any engine work. `.vscodeignore` negation (`!node_modules/wx-fast-element-tsplugin/**`) is the expected fix; if it fails, the fallback is a post-package VSIX rewrite step |
| **A Rust panic aborts tsserver**, taking every TypeScript feature with it | **High** | `catch_unwind` at every `wasm_bindgen` entry point, panics converted to a null result plus a logged diagnostic; the plugin treats a poisoned instance as "engine unavailable" and falls through to the undecorated language service. P1-07 |
| **Our template parser diverges from parse5** on some construct, producing phantom diagnostics | Medium | Differential test against parse5 over the repo corpus plus lit-analyzer's own parser fixtures, as a Phase 1 gate (P1-05). `swc_html_parser` is the named fallback — [0003](decisions/0003-own-template-parser.md) |
| **WASM instantiation cost is paid per project** in a multi-project workspace | Medium | One compiled `WebAssembly.Module` cached at module scope, one instance per project. Measured in P5-04 |
| **`typescriptServerPlugins` requires `enableForWorkspaceTypeScriptVersions`**, so the plugin runs inside whatever TypeScript version the workspace pins | Medium | Support a declared range and degrade to "engine off" outside it, rather than throwing. fast-analyzer tests against four TS versions; we test against the range we claim |
| **The `${x => …}` unwrapping heuristic** is subtler than the fork's `getCallSignatures()[0]` — overloads, generics, `this` parameters | Medium | Enumerated as its own task (P3-08) with the fork's known-good cases as regression tests |
| **Scope creep into being a general web-components analyzer** | Low | §2 non-goals, and [0009](decisions/0009-no-lit-compatibility.md) |

## 8. Alternatives considered

**Keep patching `temp/fast-analyzer`.** Cheapest today. It leaves us maintaining a fork of an MIT
project whose upstream is lit-shaped, shipping ~11,700 lines to use ~1,000, and hand-rebasing. Every
FAST/lit divergence lands as another patch inside a rule that was written for the other library — the
`stripNullAndUndefined` shape from §1.2. Rejected on carrying cost, not on quality.

**Write it entirely in TypeScript, but FAST-native.** Perfectly reasonable, and roughly half the
work of this proposal. It gives up the per-keystroke headroom that removes the 150 ms bail-out, and
it puts this extension outside the pattern the other 24 follow. Worth naming as the honest fallback
if Phase 1's risks materialise: the design keeps a clean seam at the WASM boundary, so a TypeScript
engine behind the same protocol would be a substitution, not a rewrite.

**Everything in Rust, including the type system.** Rejected in §3.

**A standalone LSP server with its own `ts.Program`.** Rejected in
[0001](decisions/0001-tsserver-plugin-not-lsp.md) — duplicate type graph, no new information.

**A Rust engine in a child process, with the plugin as a thin client.** This is the shape RFC 010
chose for typst, and for typst it was right: a cold compile blocks for half a second and the heap
never comes back. Template analysis is neither. Kept as the documented escape hatch in
[0006](decisions/0006-wasm-inside-tsserver.md).

## 9. Success criteria

The design is validated when, on this repository's own extensions:

1. All 5 FAST elements are discovered with their full member sets, from `@customElement({ name: CONST })`
   — the case fast-analyzer misses entirely.
2. All 26 templates parse with zero false diagnostics.
3. Renaming `CsvGrid.hasHeader` updates the `:prop` bindings and the `${ref('…')}` strings that name it.
4. `${x.foo}` written for `${x => x.foo}` is reported.
5. A cold `getSemanticDiagnostics` on the largest template file completes without hitting any
   wall-clock bail-out, and a warm one is fast enough to run per keystroke. The budget and how it is
   measured: [research/spikes.md](research/spikes.md) — **no performance number in this RFC is
   measured yet**, and none should be quoted until Phase 5 measures it.

## 10. Deliberately deferred

Each of these is coherent, none is Phase 1:

- **Declarative `.html` templates** (`f-when`, `f-repeat`, `{{ }}`) — a second front end over the
  same tree, no type oracle needed, so it is nearly pure Rust. The most likely follow-up.
- **`custom-elements.json` (CEM) ingestion** from `node_modules`, so third-party FAST component
  libraries light up without being in the program.
- **`fast-router` route-configuration checking.**
- **Semantic highlighting inside templates**, beyond the TextMate injection grammar Phase 5 ships.
- **A `wx-fast-analyze` CLI** for CI, once the workspace-analysis path exists in the extension.

## 11. Open questions

Tracked with owners and closing conditions in [decisions/README.md](decisions/README.md#open-questions);
summarised here:

1. Can `.vscodeignore` negation keep `node_modules/wx-fast-element-tsplugin/` in the VSIX under
   `--no-dependencies`? (P1-09 — blocking)
2. Is our own parser the right call, or `swc_html_parser`? (P1-05 decides on evidence)
3. What TypeScript version range do we claim, and how do we degrade outside it?
4. Should `no-non-reactive-binding` default to `warning` or `error`? It is the highest-value new
   rule and also the one most likely to fire on intentional one-time bindings.
5. Does the engine need per-project instances, or can one instance serve a whole tsserver keyed by
   project id?
