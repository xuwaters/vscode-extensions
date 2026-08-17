# 0001 — Build on unmodified upstream typst; never fork

**Status**: Accepted
**Date**: 2026-08-17

## Context

Typst's compiler is published to crates.io as ~15 permissively-licensed crates, and `typst-ide` gives away
the semantically hard half of a language server (completion, hover, definition, preview↔source jumping).

[tinymist](https://github.com/Myriad-Dreamin/tinymist), the mature VSCode extension, does **not** use those
published crates. It patches nine of them to a fork
([`temp/tinymist/Cargo.toml:314`](../../../../temp/tinymist/Cargo.toml#L314)) in order to reach internals
upstream deliberately does not export. That buys richer features — signature help with evaluated argument
values, cross-package analysis, a content-hint-free preview — at the cost of re-forking on every upstream
release.

Depending on tinymist's crates would inherit that cost transitively. Writing our own server on the
published surface avoids it entirely, but only if we are disciplined about never reaching past the
published API.

The prerequisite was that the published stack works at all under `wasm32-unknown-unknown`. It does — 304
crates, zero patches ([research/spike.md §2](../research/spike.md#2-does-upstream-typst-build-for-wasm-unpatched)).

## Decision

Depend only on **published, unmodified** typst crates from crates.io. No `[patch.crates-io]`, no git
dependencies, no vendored compiler source.

When a desired feature needs something upstream does not expose, the options are, in order:

1. Rebuild it from what *is* exposed.
2. Cut the feature and record the gap.
3. Propose the export upstream.

Forking is not on the list.

## Consequences

**Makes easy.** Upgrading typst is a version bump plus whatever API drift the compiler introduces. The
0.15.0 → 0.15.1 drift we already hit in the spike was three signature changes, fixed in minutes. A
one-person project can carry this; it could not carry a fork.

**Makes hard.** A concrete, accepted feature deficit, enumerated in
[proposal.md §10](../proposal.md#10-known-limitations-from-the-no-fork-constraint):

- Signature help shows declared parameters, not evaluated argument values.
- Rename and find-references are scoped to the project's file graph; packages are read-only.
- Goto-definition on a standard-library item has no location to jump to.

**Also.** `typst-ide`'s API is genuinely stable *because* it is the published surface — the part upstream
maintains a compatibility story for. Tinymist's dependency on unpublished internals is the riskier
position, not the safer one.

## Revisit if

- A feature users actually ask for repeatedly proves impossible on the published API, **and** an upstream
  PR to export what is needed is rejected or stalls for more than two release cycles.
- Upstream starts breaking the published `World` / `typst-ide` surface every release, at which point the
  "no fork" discipline stops buying stability and only costs features.
