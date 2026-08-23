# 0007 — Rules describe FAST's semantics, even when that means dropping an inherited one

**Status**: Accepted · **Date**: 2026-08-22

## Context

fast-analyzer's 23 rules came from lit-analyzer, where they encode `lit-html`'s runtime behaviour.
FAST's runtime behaves differently in several places, and the fork's response has been to keep the
rule and work around the difference.

The clearest case is `no-nullable-attribute-binding`. lit coerces a `null` bound to an attribute into
the string `"null"`; FAST removes the attribute. So the rule is a false positive against FAST by
construction. The fork handles it twice:

```ts
// lit-analyzer-config.ts — the rule is forced off in both modes
"no-nullable-attribute-binding": ["off", "off"],

// is-assignable-in-attribute-binding.ts — and neutralised in the shared helper,
// because no-incompatible-type-binding routes through the same code
typeB = stripNullAndUndefined(typeB);
```

A rule disabled in the config *and* defeated in the engine is not a rule. It is a scar.

There is also `no-invalid-boolean-binding`, which has a default severity, appears in the extension's
settings UI, and is implemented by nothing at all.

## Decision

The rule set is derived from FAST's behaviour, not from lit-analyzer's list. Concretely:

- **Drop** a rule whose premise is false for FAST: `no-nullable-attribute-binding`,
  `no-legacy-attribute` (Polymer's `foo$=`), `no-invalid-boolean-binding` (dead).
- **Rewrite** a rule whose question is right but whose subject is lit's:
  `no-invalid-directive-binding` (lit's directive table → FAST's), `no-incompatible-property-type` →
  `no-incompatible-attr-config` (`@property({type})` → `@attr({mode, converter})`),
  `no-property-visibility-mismatch` → `no-attr-visibility-mismatch` (`@internalProperty` → there is
  no such thing in FAST).
- **Add** rules for traps that only exist in FAST — six of them, of which
  `no-non-reactive-binding` is the one that matters.
- **Change a default** where the evidence says so: `no-unknown-event` goes from `off`/`off` to
  `warn`/`warn`.

Full catalogue and rationale per rule: [design/rules.md](../design/rules.md).

## Consequences

**A rule id from fast-analyzer may not exist here**, so a user's `fast-plugin.rules.*` settings do
not transfer. That is accepted in [0008](0008-naming-and-config.md) — it is a new extension, not an
upgrade.

**The shared type helpers lose their workarounds.** `stripNullAndUndefined` is not ported;
FAST's attribute-removal semantics are modelled in the helper directly, which is the same behaviour
arrived at honestly.

**`no-non-reactive-binding` is the payoff.** It has no lit equivalent — `${this.count}` in a lit
`render()` is re-evaluated by definition — so no amount of maintaining the fork would have produced
it. FAST binds a non-function value once, at view-creation time, and the mistake compiles, typechecks
and silently produces a frozen UI
([research/fast-element.md §9](../research/fast-element.md#9-reactivity-and-the-mistake-it-invites)).
Its default is [open question 4](README.md#open-questions).

**Changing `no-unknown-event`'s default is a judgement, not a measurement.** It is `off` upstream
because lit's event model made it noisy; FAST's `$emit` gives a definite list, and event bindings are
the most common binding in the corpus — 74 of ~163. If it fires on correct code in the corpus, the
default goes back.

## Revisit if

- A dropped rule turns out to have a FAST-relevant version we missed. `no-legacy-attribute` is the
  candidate: it could be repurposed to detect lit syntax (`.prop=`) in a FAST template. That is not
  the same rule, so it would be a new record and a new id, not a resurrection.
- A new rule proves noisy on real code. The test is the corpus plus whatever we can find; a rule that
  fires on correct code gets its default lowered or gets deleted.
