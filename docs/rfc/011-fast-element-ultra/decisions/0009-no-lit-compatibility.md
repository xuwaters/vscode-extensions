# 0009 — No lit support, in any form

**Status**: Accepted · **Date**: 2026-08-22

## Context

The obvious question about a project that starts from a lit tool is whether to keep supporting lit.
It would be cheap in places — the two libraries have similar-looking template syntax — and it would
make the extension useful to more people.

## Decision

No. Not `.prop=`, not `lit-html` directives, not `@property`, not `LitElement`, not `@state`, not
`@internalProperty`, not `ifDefined`/`classMap`/`styleMap`/`repeat`/`until`, not lit's security
system.

## Why the similarity is a trap

The syntaxes overlap in a way that makes "support both" actively harmful rather than merely costly:

| Written | lit | FAST |
| --- | --- | --- |
| `.prop="${x}"` | property binding | **an attribute literally named `.prop`** |
| `:prop="${x}"` | an attribute named `:prop` | **property binding** |
| `?attr="${x}"` | boolean attribute | boolean attribute |
| `@event="${x}"` | event listener | event listener |
| `${x.foo}` | re-evaluated every render | **bound once, forever** |
| `${x => x.foo}` | binds a function as a value | **the reactive binding** |
| `attr="${null}"` | sets the string `"null"` | **removes the attribute** |

Three of those seven rows are cases where the *same source text* means something different. A tool
that supports both must decide which it is looking at, and the only signals are the imports and the
template's type argument — both of which can be absent, and one of which is `any` by default.
Guessing wrong produces confident, wrong diagnostics, which is worse than no diagnostics.

This is not hypothetical: fast-analyzer's constants file currently declares **both** `.` and `:` as
property modifiers, so `.prop=` in a FAST template is treated as a property binding when FAST would
set an attribute called `.prop`.

## Consequences

**A mixed lit + FAST codebase gets one of the two checked.** Both extensions can be installed
([0008](0008-naming-and-config.md)); each reports on the templates it recognises and, in the
overlap, both report. That is a real limitation and it is the correct one — the alternative is a
heuristic that is wrong some of the time and cannot say which times.

**`lit-plugin` remains the answer for lit**, is actively maintained, and this RFC creates no reason
to change that.

**The scope stays bounded.** Every "we could also handle…" question about Stencil, Polymer, Angular
Elements or vanilla custom elements has the same answer, for the same reason: the value of this tool
is that it knows exactly one library's semantics precisely.

**Detection is by resolved symbol.** A template is FAST's when its tag function resolves to
`@microsoft/fast-element`'s `html`, not when it is spelled `html`
([component-model.md §2.2](../design/component-model.md#22-which-decorator-is-customelement)). A file
with both libraries imported gets each template classified correctly, which is the one part of the
mixed case we can do properly.

## Revisit if

- Nothing plausible. The two libraries would have to converge on a syntax, and they are moving apart:
  FAST 3 is further from lit than FAST 2 was.
