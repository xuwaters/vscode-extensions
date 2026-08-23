# 0006 — Run the WASM inside tsserver, not in a child process

**Status**: Accepted · **Date**: 2026-08-22

## Context

[RFC 010](../../010-typst-ultra/decisions/0003-server-in-child-process.md) put typst's WASM in a
child process, for two reasons that were correct there: a cold compile blocks for over half a second,
and the WASM heap is never returned to the OS. Neither is true of a template analyzer, but the
question deserves asking again rather than inheriting an answer.

Options for where the engine runs, given [0001](0001-tsserver-plugin-not-lsp.md) puts the plugin
inside tsserver:

1. **In tsserver**, `require`d by the plugin.
2. **In a child process** the plugin spawns and talks to over IPC.
3. **In the extension host**, with the plugin forwarding through VS Code's plugin-configuration
   channel. (Not really an option — that channel is one-way and not a request/response transport.)

## Decision

In tsserver. `wasm-pack build --target nodejs` output, `require`d by the plugin, one compiled
`WebAssembly.Module` shared at module scope, one instance per `ts.server.Project`.

## Why not a child process

`getCompletionsAtPosition` is on the keystroke path. An IPC hop is a synchronous wait in a language
service method that has no way to be asynchronous — tsserver's `LanguageService` interface is
synchronous throughout. A child process would mean either blocking on a socket read inside a
completion request, or restructuring around a cache that answers stale and refreshes behind, which
is a considerably worse editor experience than the thing it protects against.

The two facts that justified a child process for typst do not hold: the engine's unit of work is
kilobytes of template rather than a book, and its heap is a parse tree plus a registry, both of
which are bounded by the size of the workspace's templates.

## Consequences

**A Rust panic can take down tsserver.** This is the whole cost of the decision and it is paid in
three layers ([architecture.md §1.1](../design/architecture.md#11-failure-containment)):

1. `catch_unwind` at every `#[wasm_bindgen]` entry point; a panic returns `None`, never an exception.
2. Panic counting per instance; the second one poisons the instance and drops it.
3. The plugin's method decoration falls through to the undecorated language service when a decorated
   method fails — fast-analyzer's `wrapTryCatch` already does this and it is worth keeping.

The end state of a catastrophic engine failure is therefore "TypeScript, unmodified", not a broken
editor. **This is the condition on which the decision rests**, and P1-07 tests it by deliberately
panicking.

**Memory in a process the user did not choose.** The artifact should be small — a parser, tables and
rules, with no compiler in it — but "should be" is not a measurement.
[budget 2](../research/spikes.md#budget-2--wasm-instantiation-and-memory-in-tsserver) measures the
artifact, instantiation cost, resident heap after the corpus, and growth over 1,000 edits.

**Per-project instances** so two projects cannot see each other's registries. Whether that is
affordable with several projects open is
[open question 5](README.md#open-questions).

**No browser build.** VS Code's TS server runs in a web worker on vscode.dev, where the plugin probe
mechanism and `require` both differ. Every other extension in this repo has the same constraint.

## The escape hatch

If the memory measurement is bad, or if panics prove uncontainable in practice, the engine moves to a
child process and the plugin gains a synchronous request cache: fresh answers for diagnostics, which
are already debounced, and last-known answers for completions, refreshed behind. That is a real
degradation and it is why it is a fallback rather than the plan.

## Revisit if

- [budget 2](../research/spikes.md#budget-2--wasm-instantiation-and-memory-in-tsserver) shows the
  engine adding materially to tsserver's footprint, or growing without bound across edits.
- A panic escapes all three layers in P1-07, or in the field.
- The engine grows something heavy. It should not: the moment it wants a compiler in it, this
  decision is wrong.
