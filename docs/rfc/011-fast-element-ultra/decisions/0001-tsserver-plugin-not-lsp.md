# 0001 — Ship a TypeScript server plugin, not a language server

**Status**: Accepted · **Date**: 2026-08-22

## Context

Every other extension in this repo that provides language features either registers VS Code providers
directly or, in typst-ultra's case, runs an LSP server. Neither works here.

FAST templates live inside `.ts` and `.js` files. Anything useful to say about them — is this
expression assignable to that member, does this class declare that property — requires a
`ts.Program` with a type checker over the whole project. There are three ways to get one:

1. **Be a tsserver plugin.** tsserver hands you its `LanguageService`, its `Program`, and its
   `TypeChecker`, and lets you decorate the service's methods.
2. **Run an LSP server that builds its own `ts.Program`.**
3. **Register VS Code providers in the extension host and build a `ts.Program` there.**

## Decision

Be a tsserver plugin, via `contributes.typescriptServerPlugins`, as lit-analyzer does.

## Consequences

**What this buys.** The type graph already exists and is already incremental. Diagnostics appear
inline with TypeScript's own, in the same pass, with no second squiggle source and no ordering
problem. Completions merge with TypeScript's rather than competing with them. `enableForWorkspaceTypeScriptVersions`
means the plugin runs against whatever TypeScript the workspace pins, so our answers match the
compiler the user actually builds with.

**What it costs.**

- We live inside tsserver, a process the user did not choose to spend memory on, and a crash there
  takes every TypeScript feature down. [0006](0006-wasm-inside-tsserver.md) and
  [architecture.md §1.1](../design/architecture.md#11-failure-containment) are the mitigation.
- Packaging is awkward: tsserver resolves plugins from `node_modules/<name>`, and this repo packages
  with `vsce package --no-dependencies`. This is
  [gate 1](../research/spikes.md#gate-1--can-the-plugin-be-packaged-at-all) and it is the first
  task in Phase 1.
- No LSP, so no other editor gets this for free.
- Debugging is harder: the code runs in a process started by VS Code with `TSS_DEBUG`.

**Why not option 2.** A second `ts.Program` over the same files is a second copy of a type graph
that already exists in memory, built from the same sources, producing the same answers. On a
workspace of any size that is hundreds of megabytes to learn nothing new. It also has to track
tsconfig changes, file watching, and module resolution independently — all of which tsserver already
does, and does correctly.

**Why not option 3.** Same duplication as option 2, plus the extension host is a worse place for it:
it is the process that has to stay responsive for the whole UI.

## Revisit if

- tsserver's plugin API stops exposing what we need — the `LanguageService` decoration pattern is
  not a stability contract, and Microsoft has changed it before.
- The packaging gate fails all three of its fallbacks, making the plugin unshippable in this repo's
  build.
- Editor-agnosticism becomes a goal. It is not one today; the repo builds VS Code extensions.
