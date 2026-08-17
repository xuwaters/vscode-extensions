# RFC 010: Typst Ultra — a WASM Typst language server and live preview

**Status**: Draft, awaiting review · **Implementation**: not started
**Date**: 2026-08-17 · **Last updated**: 2026-08-17
**Extension**: `wx-vsce-typst-ultra` (`extensions/typst-ultra`, new)
**Rust crates**: `crates/typst/{typst-session,typst-lsp-core,typst-preview-core,typst-lsp-wasm}` (new)
**Upstream**: [typst 0.15.1](../../../temp/typst) (Apache-2.0) · [tinymist 0.15.4-rc1](../../../temp/tinymist) (Apache-2.0, reference only)

---

## The one-paragraph version

Typst has no first-party editor tooling. The mature option, [tinymist](https://github.com/Myriad-Dreamin/tinymist),
ships a per-platform native binary and maintains a **patched fork of the typst compiler**. This RFC takes
the other road: build our own language server on **unmodified upstream typst crates**, compile it to a
single `wasm32-unknown-unknown` artifact, and ship that one file to every platform. A working prototype
confirms the whole upstream stack — compiler, layout, SVG, PDF, `typst-ide`, `typstyle` — builds and runs
in WASM under Node with **6 ms incremental recompiles** on a 30-page document. The extension is a thin
VSCode host: a `vscode-languageclient` talking to the WASM server in a child process, plus a preview
webview that receives per-page SVG patches and syncs both ways with the editor.

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
| [research/spike.md](research/spike.md) | Check whether this actually works. Every number in the RFC comes from here |
| [decisions/](decisions/README.md) | Understand why a specific choice was made, and what would reverse it |
| [design/architecture.md](design/architecture.md) | See the process model, data flow, and protocols |
| [design/crates.md](design/crates.md) | Work on the Rust side: crate boundaries, `World`, VFS, fonts, packages |
| [design/lsp-features.md](design/lsp-features.md) | Work on a language feature, or look up a setting |
| [design/preview.md](design/preview.md) | Work on the preview, custom editor, sync, or export |
| [research/references.md](research/references.md) | Check a license, or find the upstream API a design leans on |
| [tasks/](tasks/README.md) | Pick up work, or see where the project is |

## Current state

The RFC is written and the feasibility spike is complete. **No implementation has started.**

| | |
| --- | --- |
| Open questions | 0 of 8 — all resolved in [decisions/](decisions/README.md) |
| Tasks complete | 0 / 62 ([tasks/](tasks/README.md)) |
| Research debts | 7 open, each owned by a task ([tasks/README.md](tasks/README.md#research-debts)) |
| Biggest unknown | ~50 ms of the preview latency budget is estimated, not measured ([P3-05](tasks/phase-3-preview.md)) |

## What is new here relative to the rest of the repo

Nine extensions already follow the "Rust crate → `wasm-pack --target nodejs` → `require()` in the extension
host" pattern. Typst Ultra keeps it but changes two things:

1. **The WASM runs in a child process, not the extension host** — a cold compile blocks for up to 262 ms
   and the heap is never returned to the OS ([0003](decisions/0003-server-in-child-process.md)).
2. **It speaks LSP.** The repo's first `vscode-languageclient` consumer. The payoff is an editor-agnostic
   server plus cancellation, progress, and capability negotiation for free, rather than hand-rolling them
   across ~20 providers.

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
- **Superseded numbers get a visible marker, not a silent edit.** Two figures in this RFC have already
  changed (the 411 ms latency bug and the 423 MB heap estimate); both are called out where they were used,
  because a reader who remembers the old number needs to know it moved.
