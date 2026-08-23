# 0002 — Rust owns the template, TypeScript owns the types, and they meet once per file

**Status**: Accepted · **Date**: 2026-08-22

## Context

The brief is "Rust + TypeScript". The question is where the line goes, and it is not a matter of
taste: seven of fast-analyzer's rules ask whether one TypeScript type is assignable to another, and
that question has exactly one implementation in the world.

Structural assignability over TypeScript's type system — unions, intersections, generics, conditional
and mapped types, `strictNullChecks`, declaration merging, and the relation cache that keeps it
tractable — is tens of thousands of lines of `checker.ts`, has no specification, and changes every
release. Reimplementing it in Rust is not a large task; it is a different project, and the version we
built would disagree with `tsc` in ways nobody could enumerate.

So TypeScript stays. The design question is how little stays with it.

## Decision

Three rules:

1. **TypeScript owns what only the compiler knows**: the AST, the checker, symbol resolution, and
   therefore component discovery ([0004](0004-drop-web-component-analyzer.md)) and type
   assignability.
2. **Rust owns what only the template knows**: parsing, spans, the component registry, rule
   evaluation, and every position query.
3. **They meet once per file, in each direction.** Rust never calls back into JavaScript. A rule
   that cannot decide emits a **binding fact** — the question, with everything needed to answer it —
   and the plugin answers the batch after the pass.

The protocol is [architecture.md §4](../design/architecture.md#4-the-boundary-protocol).

## Consequences

**The engine is a pure function of what it is told.** It does no I/O, has no host callbacks, and
cannot ask a question mid-pass. That is what makes `cargo test` run the real engine against recorded
JSON payloads instead of against a mocked type checker, and it is worth more than the flexibility it
gives up.

**Type identity crosses as an interned id, never as a structure.** Rust compares ids for equality
when deduplicating and hands them back when it needs a question answered; it never inspects a type.
Serialising `SimpleType` graphs across the boundary would be both expensive and a second, lossy model
of TypeScript's types.

**One crossing per file, not one per binding.** `pdf-ultra`'s template has ~80 bindings; a
per-binding round trip is ~80 crossings per diagnostic pass. Whether the batch is small enough in
practice is [budget 3](../research/spikes.md#budget-3--type-oracle-round-trip).

**The type rules stay in TypeScript, in substance unchanged.** They keep using `ts-simple-type` and
the same `isAssignableInXBinding` helpers fast-analyzer uses. Porting them would mean porting
`ts-simple-type`. What changes is *which* binding they are asked about, and Rust decides that.

**The honest accounting**: roughly 19 of 26 rules, the whole parser, the registry, and all ten
position features are Rust. Component discovery, seven rules, and CSS are TypeScript. Calling this a
"Rust extension" without that table would be a claim the code does not support, which is why the
table is in [proposal.md §3](../proposal.md#3-the-constraint-that-shapes-everything) rather than
buried here.

## Alternatives

**Rust calls back into JS for each type question** — `wasm_bindgen` imports make this easy. Rejected:
it makes the engine's control flow depend on the host's, and it makes every engine test need a mock
oracle.

**Component discovery in Rust too**, with `oxc` or `swc` parsing the TypeScript. Tempting — the
discovery in fast-analyzer is purely syntactic — and rejected, because "purely syntactic" is exactly
what is wrong with it: resolving `@customElement({ name: CONST })` needs the checker, and that is the
bug that motivates this whole RFC
([research/corpus.md §2](../research/corpus.md#2-why-fast-analyzer-finds-none-of-it)). A second
parser would also be a second source of truth that drifts from what tsserver believes.

**Everything in TypeScript.** The honest fallback, named in
[proposal.md §8](../proposal.md#8-alternatives-considered). The seam this decision creates is what
makes it a substitution rather than a rewrite.

## Revisit if

- The batch of binding facts turns out to dominate the pass
  ([budget 3](../research/spikes.md#budget-3--type-oracle-round-trip)) — the answer would be to move
  more rule logic into the plugin, not to add callbacks.
- TypeScript ever exposes its type relations as a callable service outside the compiler. It will not,
  but if it did, the split moves.
