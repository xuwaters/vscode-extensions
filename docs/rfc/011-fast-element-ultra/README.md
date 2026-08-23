# RFC 011: FAST Element Ultra — a Rust analysis engine for FAST Element templates

**Status**: Proposed · **Implementation**: not started
**Date**: 2026-08-22 · **Last updated**: 2026-08-22
**Extension**: `wx-vsce-fast-element-ultra` (`extensions/fast-element-ultra`, new)
**Rust crates**: `crates/fast/{fast-template-syntax,fast-html-data,fast-analyzer-core,fast-analyzer-wasm}` (new)
**References**: [fast-analyzer](../../../temp/fast-analyzer) (MIT, a fork of lit-analyzer 2.0.3) ·
[`@microsoft/fast-element` 3.0.2](../../../temp/microsoft-fast) (MIT, reference only)

---

## The one-paragraph version

FAST Element templates are strings as far as TypeScript is concerned, so nothing checks them. The one
tool that does — [`temp/fast-analyzer`](../../../temp/fast-analyzer) — is our fork of lit-analyzer
with ~1,000 lines of FAST support bolted onto ~11,700 lines written for lit, and it is thin enough
that **it discovers zero components in this repository**, because all three of our FAST extensions
name their tag with a `const` and the fork's extractor only accepts string literals. This RFC
proposes replacing it: a FAST-native component model, a rule set that describes FAST's semantics
rather than lit's, and the analysis engine — parser, registry, rules, and every position query —
written in Rust and compiled to one WASM artifact. Type checking stays in TypeScript, because
`checker.ts` cannot be reimplemented and pretending otherwise would quietly drop seven rules.

## Where it stands

| | |
| --- | --- |
| Status | Proposed. Nothing built |
| Tasks | 0 / 63 across 5 phases ([tasks/](tasks/README.md)) |
| Decisions | 9 recorded, 5 open questions ([decisions/](decisions/README.md)) |
| Measurements taken | Corpus and parity inventories only. **No performance number anywhere in this RFC is measured** ([research/spikes.md](research/spikes.md)) |
| Blocking unknown | Whether a tsserver plugin under `node_modules/` survives `vsce package --no-dependencies` (P1-09) |

## What is already established, and what is not

Two things in this RFC are facts, gathered from the source:

- **The parity inventory** — what fast-analyzer actually does, feature by feature and rule by rule,
  counted from its tree. [research/parity.md](research/parity.md).
- **The corpus** — 26 typed templates, 74 event bindings, 55 `@observable` members and 5 elements
  across csv-ultra, pdf-ultra and typst-ultra, and the specific reasons fast-analyzer sees none of
  them. [research/corpus.md](research/corpus.md).

Everything about *speed* is a hypothesis. The RFC argues Rust is the right engine from the shape of
the workload and from lit-analyzer's own 150 ms bail-out constant — not from a benchmark, because
none has been run. [research/spikes.md](research/spikes.md) lists what has to be measured, in what
order, and what result would send us back to §8's TypeScript fallback.

## Layout

```
011-fast-element-ultra/
├── README.md          ← you are here: status, orientation, maintenance rules
├── proposal.md        ← the RFC: motivation, goals, non-goals, architecture, phases, risks
├── decisions/         ← why things are the way they are (9 ADRs + open questions)
├── design/            ← how it will work (5 documents, to become living ones)
├── research/          ← what was measured, and what still has to be
└── tasks/             ← 63 tasks across 5 phases
```

| Read this | If you want to |
| --- | --- |
| [proposal.md](proposal.md) | **Start here.** The argument, the scope, the plan, the risks |
| [research/corpus.md](research/corpus.md) | See the evidence that the current tool does not fit |
| [research/parity.md](research/parity.md) | Know exactly what "parity with fast-analyzer" means |
| [research/fast-element.md](research/fast-element.md) | Look up FAST 3.x syntax and API as we read it from source |
| [research/spikes.md](research/spikes.md) | Check whether this can work before committing to Phase 2 |
| [design/architecture.md](design/architecture.md) | See the process model, the virtual document, the boundary protocol |
| [design/component-model.md](design/component-model.md) | Work on element discovery |
| [design/rules.md](design/rules.md) | Work on a rule, or look up why a lit rule was dropped |
| [design/features.md](design/features.md) | Work on an IDE feature, or look up a setting |
| [design/crates.md](design/crates.md) | Work on the Rust side: crate boundaries and what may depend on what |
| [decisions/](decisions/README.md) | Understand why a choice was made, and what would reverse it |
| [tasks/](tasks/README.md) | See what is next |

## What is new here relative to the rest of the repo

Twelve extensions already follow "Rust crate → `wasm-pack --target nodejs` → `require()` from
TypeScript". This one keeps that and changes three things:

1. **The host is not the extension host.** The WASM is loaded by a **TypeScript server plugin**
   running inside tsserver. This repo has never shipped one, and the packaging path is different
   enough to be the first task in Phase 1 ([0001](decisions/0001-tsserver-plugin-not-lsp.md),
   [0006](decisions/0006-wasm-inside-tsserver.md)).
2. **The Rust engine is not self-sufficient.** It asks TypeScript questions it cannot answer, over a
   declared protocol ([0002](decisions/0002-rust-engine-typescript-oracle.md)). Every other Rust
   crate here is a closed system.
3. **The subject is this repo's own code.** csv-ultra, pdf-ultra and typst-ultra are the test corpus.
   That is unusually good for a language tool and it is worth exploiting: a regression here is a
   regression somebody notices the same week.

## Maintaining these documents

Same lanes as [RFC 010](../010-typst-ultra/README.md), because the split is what stops this becoming
stale documentation.

| Directory | Changes when | Rule |
| --- | --- | --- |
| `proposal.md` | Rarely — scope or phases change | Amend, don't rewrite. A changed decision belongs in `decisions/` |
| `decisions/` | A choice is made or reversed | **Append-only in spirit.** Supersede with a new record; never edit a decision's history |
| `design/` | Implementation reveals the design was wrong or incomplete | **Living.** Update in the same PR as the code |
| `research/` | A measurement is taken or redone | When a number changes, check every decision that cites it |
| `tasks/` | Every PR | Update in the same PR as the work. Task IDs are stable and never renumbered |

Three habits carried over, plus one specific to this RFC:

- **Numbers live in `research/`, once.** Everything else links to them.
- **A decision without a "revisit if" is a preference.** Every record names the observable condition
  that would make it wrong.
- **Superseded numbers get a visible marker, not a silent edit.**
- **Do not quote a performance figure this RFC has not measured.** Today that is all of them. When
  Phase 1 and Phase 5 produce real numbers, they go in `research/`, and the claims in `proposal.md`
  §1.3 and §9 get checked against them — including the possibility that they do not hold.
