# 0011 — Panic containment lives in the plugin's try/catch, not in catch_unwind

**Status**: Accepted · **Date**: 2026-08-23 · **Amends**: [0006](0006-wasm-inside-tsserver.md), [architecture.md §1.1](../design/architecture.md#11-failure-containment)

## Context

The design named `catch_unwind` at every `wasm_bindgen` entry point as
containment layer 1. Testing the real artifact (P1-07, done against the
built `.wasm`, not against native `cargo test`) showed that layer does not
exist on the shipping target: `wasm32-unknown-unknown` builds with
`panic = "abort"`, a Rust panic compiles to the `unreachable` instruction,
and the trap unwinds no Rust frames — `catch_unwind` catches nothing. The
call surfaces in JavaScript as a `RuntimeError` thrown out of the glue.

The native tests passed; the artifact test failed. This is precisely why
P1-07 demanded "tested by deliberately panicking" against the real thing.

## Decision

The containment contract is:

1. **The plugin wraps every engine call in try/catch** (`SafeEngine.guard`).
   A throw is counted; the second poisons the instance — a trapped instance's
   memory is not trustworthy — and a poisoned engine answers nothing.
2. **Recoverable engine errors never throw at all.** Bad JSON, unknown
   documents: the Rust adapter converts them to `None` + `lastError()`, and
   they do not count toward poisoning.
3. **Every decorated language-service method falls back** to the undecorated
   one on any failure, so the end state remains "TypeScript, unmodified".

`debugPanic()` stays exported so the containment path is permanently tested
against the real artifact (`test/smoke.test.ts`): one panic → still ok, two →
poisoned, TypeScript's own diagnostics still flow, the event is in the log.

The `catch_unwind` wrappers remain in the Rust adapter: they are real under
native `cargo test` and they are what converts recoverable errors on every
target. They are no longer claimed as the panic barrier.

## Consequences

- One heap: per-project `Engine` objects live in one WebAssembly instance
  (`--target nodejs` instantiates at require time), so poisoning is
  process-wide across projects. Accepted — the alternative is per-project
  instantiation machinery for an event that, when it happens twice, argues
  the engine shouldn't be trusted anywhere in that process anyway. A TS
  Server restart resets everything.
- The status item surfaces the poisoned state to the user
  (design/features.md, P5-08) instead of failing silently.

## Revisit if

- Rust's `wasm32` exception-handling support (`panic = "unwind"` via WASM
  exceptions) stabilises on stable toolchains — layer 1 could then move back
  into Rust and poisoning could become per-call recovery.
- A trap is ever observed corrupting tsserver *through* the try/catch — that
  would mean the glue's invariants are weaker than believed, and the child
  process escape hatch in [0006](0006-wasm-inside-tsserver.md) applies.
