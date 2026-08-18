# RFC 010: Typst Ultra — a WASM Typst language server and live preview

**Status**: Implemented · **Implementation**: 60 / 62 tasks complete
**Date**: 2026-08-17 · **Last updated**: 2026-08-17
**Extension**: `wx-vsce-typst-ultra` ([`extensions/typst-ultra`](../../../extensions/typst-ultra))
**Rust crates**: [`crates/typst/{typst-session,typst-lsp-core,typst-preview-core,typst-lsp-wasm}`](../../../crates/typst)
**Upstream**: [typst 0.15.1](../../../temp/typst) (Apache-2.0) · [tinymist 0.15.4-rc1](../../../temp/tinymist) (Apache-2.0, reference only)

---

## The one-paragraph version

Typst has no first-party editor tooling. The mature option, [tinymist](https://github.com/Myriad-Dreamin/tinymist),
ships a per-platform native binary and maintains a **patched fork of the typst compiler**. This RFC took
the other road: build our own language server on **unmodified upstream typst crates**, compile it to a
single `wasm32-unknown-unknown` artifact, and ship that one file to every platform. It works. The
extension is a thin VSCode host: a `vscode-languageclient` talking to the WASM server in a child process,
plus a preview webview that receives per-page patches and syncs both ways with the editor.

## Where it stands

| | |
| --- | --- |
| Tasks complete | 60 / 62 ([tasks/](tasks/README.md)) — 1 scoped, 1 blocked on hardware |
| Tests | 751 Rust · 91 extension, including the real artifact and the built server over LSP |
| Artifact | 26.5 MB `.wasm`, 10 MB gzipped · **16 MB VSIX** |
| Open questions | 0 of 8 — all resolved in [decisions/](decisions/README.md) |
| Research debts | 4 of 7 closed; the 3 open ones each need hardware or a network |

The two incomplete tasks are [P4-06](tasks/phase-4-polish.md) (browser build — scoped in
[design/browser.md](design/browser.md), blocked on whether vscode.dev serves cross-origin isolation
headers) and [P4-09](tasks/phase-4-polish.md) (Windows/Linux verification — needs those machines; every
platform branch is unit-tested from any host in the meantime).

## What measurement changed

The RFC was written on spike numbers from synthetic documents. Building it produced better ones, and
three of them contradict what was written:

| Claim in the RFC | What was measured |
| --- | --- |
| Transport is ~30 ms and is "where the risk lives" | **3.7 ms.** 8× cheaper. [transport.md](research/transport.md) |
| A page is ~386 KB | **470 KB** for a real two-column paper. [corpus.md](research/corpus.md) |
| Coordinate rounding would "cut it substantially" | **2.9%.** `typst-svg` already rounds through `ryu` |
| Cold compile, 75 pages < 300 ms | **524 ms at 104 pages** — 5.0 ms/page against 3.5. Target misses; it should be restated per page |
| Font parsing might dominate cold start | **10 ms**, 2.5% of the budget |
| `evictAge: 1` chosen on synthetic data | Confirmed on real documents, by narrower margins |

The transport result is the important one: it is what kept
[0006](decisions/0006-preview-rendering.md)'s escape hatches in Phase 4 instead of promoting them.

## Layout

```
010-typst-ultra/
├── README.md          ← you are here: status, orientation, maintenance rules
├── proposal.md        ← the RFC: motivation, goals, non-goals, phases, risks
├── decisions/         ← why things are the way they are (ADRs, 10 records)
├── design/            ← how it works (living documents, updated as built)
├── research/          ← what was measured, and what we owe upstream
└── tasks/             ← what is done and what is next (4 phases, 62 tasks)
```

| Read this | If you want to |
| --- | --- |
| [proposal.md](proposal.md) | **Start here.** The argument, the scope, the plan, the risks |
| [tasks/](tasks/README.md) | See what is done, what is not, and why |
| [research/spike.md](research/spike.md) | Check whether this could work. The feasibility numbers |
| [research/transport.md](research/transport.md) | The preview latency budget, measured |
| [research/corpus.md](research/corpus.md) | How real documents behave, as against `#lorem` |
| [research/upstream.md](research/upstream.md) | What we owe upstream, and where the published API ran out |
| [decisions/](decisions/README.md) | Understand why a specific choice was made, and what would reverse it |
| [design/architecture.md](design/architecture.md) | See the process model, data flow, and protocols |
| [design/crates.md](design/crates.md) | Work on the Rust side: crate boundaries, `World`, VFS, fonts, packages |
| [design/lsp-features.md](design/lsp-features.md) | Work on a language feature, or look up a setting |
| [design/preview.md](design/preview.md) | Work on the preview, custom editor, sync, or export |
| [design/browser.md](design/browser.md) | Scope the vscode.dev build before starting it |

## What is new here relative to the rest of the repo

Nine extensions already follow the "Rust crate → `wasm-pack --target nodejs` → `require()` in the extension
host" pattern. Typst Ultra keeps it but changes two things:

1. **The WASM runs in a child process, not the extension host** — a cold compile blocks for up to 524 ms
   on a real book and the heap is never returned to the OS ([0003](decisions/0003-server-in-child-process.md)).
2. **It speaks LSP.** The repo's first `vscode-languageclient` consumer. The payoff is an editor-agnostic
   server plus cancellation, progress, and capability negotiation for free, rather than hand-rolling them
   across ~20 providers.

It also brought the repo its **first CI workflow** ([`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml)),
because "unmodified typst builds for `wasm32-unknown-unknown`" is a claim about a target nobody compiles
for by accident.

## Maintaining these documents

The point of the split is that each directory has a different lifecycle. Keeping them in their lanes is
what stops this becoming stale documentation.

| Directory | Changes when | Rule |
| --- | --- | --- |
| `proposal.md` | Rarely — scope or phases change | Amend, don't rewrite. A changed decision belongs in `decisions/`, not here |
| `decisions/` | A choice is made or reversed | **Append-only in spirit.** Supersede with a new record; never edit a decision's history |
| `design/` | Implementation reveals the design was wrong or incomplete | **Living.** Update in the same PR as the code. These describe the system as built |
| `research/` | A measurement is taken or redone | When a number changes, check every decision that cites it |
| `tasks/` | Every PR | Update in the same PR as the work. Task IDs are stable and never renumbered |

Three habits worth keeping:

- **Numbers live in `research/`, once.** Everything else links to them. When a benchmark is re-run, there
  is exactly one place to edit — and the decision records that cite it are the checklist of what to
  re-examine.
- **A decision without a "revisit if" is a preference.** Every record names the observable condition that
  would make it wrong.
- **Superseded numbers get a visible marker, not a silent edit.** Several figures in this RFC have now
  changed — the 411 ms latency bug, the 423 MB heap estimate, the ~50 ms transport estimate, the 386 KB
  page — and each is called out where it was used, because a reader who remembers the old number needs to
  know it moved.
