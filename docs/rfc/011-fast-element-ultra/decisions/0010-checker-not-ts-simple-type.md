# 0010 — The type oracle uses the checker itself, not ts-simple-type

**Status**: Accepted · **Date**: 2026-08-23 · **Amends**: [0002](0002-rust-engine-typescript-oracle.md)

## Context

[0002](0002-rust-engine-typescript-oracle.md) said the type rules would keep
using `ts-simple-type` "exactly as fast-analyzer does". Implementation said
otherwise: `ts-simple-type` is pinned at `2.0.0-next.0`, last published for
the TypeScript 4.x API, and this repo builds against TypeScript 6. A library
that re-models the compiler's types is exactly the kind of dependency that
breaks silently across major compiler versions — and being *differently*
right about assignability than the compiler the user builds with is worse
than being tied to it.

## Decision

The oracle (`tsplugin/oracle.ts`) answers binding facts with:

1. **`TypeFlags` reasoning** for the coercion-shaped rules —
   `no-boolean-in-attribute-binding` (BooleanLike after stripping
   null/undefined), `no-complex-attribute-binding` (not primitive-like),
   boolean-attribute fitness, numeric/boolean attribute targets, and literal
   checks. These are questions about the *kind* of a type, which flags answer
   exactly.
2. **`checker.isTypeAssignableTo`** for structural assignability
   (`no-incompatible-type-binding` on property bindings and closed string
   sets). It is marked internal but has existed unchanged since TypeScript
   4.4, is the same relation `tsc` itself uses, and is guarded: when absent,
   the check is skipped rather than guessed.

FAST's attribute-removal semantics live in the oracle directly:
`checker.getNonNullableType` strips null/undefined from a bound value before
any attribute comparison — the behaviour fast-analyzer arrived at by patching
`stripNullAndUndefined` into a shared helper, arrived at here honestly
([0007](0007-fast-rule-semantics.md)).

## Consequences

- No dependency on an unmaintained type-model library; no drift between what
  the oracle believes and what the user's compiler believes.
- One internal-API dependence, fenced: `isTypeAssignableTo` and `resolveName`
  are looked up dynamically and their absence degrades to "don't report",
  never to a wrong report.
- The seven type rules' substance is preserved; the corpus gate (silence over
  strict mode) and a seeded fixture per rule pin the behaviour.

## Revisit if

- TypeScript removes or renames `isTypeAssignableTo` — the fallback is
  already in place (skip), and the fix would be the public API if one ever
  appears.
- A rule needs an assignability nuance the checker relation doesn't express
  (attribute coercion tables beyond what `TypeFlags` covers). That would be
  new modelling, not a reason to resurrect `ts-simple-type`.
