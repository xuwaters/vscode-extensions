# The rule catalogue

**Status**: living — all 26 rules implemented; a seeded fixture per rule in
`extensions/fast-element-ultra/test/diagnostics.test.ts` and silence over the
corpus in `test/corpus.test.ts` pin the behaviour. Where things run:
19 rules in Rust (`crates/fast/fast-analyzer-core/src/rules.rs`); the type
rules in the oracle (`tsplugin/oracle.ts`,
[0010](../decisions/0010-checker-not-ts-simple-type.md)); the class-file
rules R10/R12/R19/R20 at discovery (`tsplugin/extract.ts`); R11 and F4 at the
engine's registry level; R13 and F3's `::part` half through the CSS service
(`tsplugin/css.ts`). One engine-decidable special case: `@event="literal"` is
reported as `no-noncallable-event-binding` without the oracle — a string is
never callable.

26 rules: 17 carried from fast-analyzer unchanged in intent, 3 rewritten because the lit version
asks the wrong question of FAST, 6 new because they describe traps that only exist in FAST. Three of
fast-analyzer's are dropped.

The inventory this is derived from is [research/parity.md §3](../research/parity.md#3-the-rule-set).
The semantics being checked are in [research/fast-element.md](../research/fast-element.md).

Severities are `[default, strict]`. "Oracle" marks a rule that emits binding facts for TypeScript to
answer ([architecture.md §4.3](architecture.md#43-rust--ts-binding-facts)) rather than deciding in
Rust.

---

## Carried

| Id | Rule | Default | Oracle | What it checks |
| --- | --- | --- | :---: | --- |
| R1 | `no-unknown-tag-name` | off / warn | | The tag is a known built-in or a registered component |
| R2 | `no-missing-import` | off / warn | | A component used here is reachable from this module's imports |
| R3 | `no-unclosed-tag` | warn / error | | Every element is closed; custom elements are not self-closed |
| R4 | `no-unknown-attribute` | off / warn | | `attr="…"` names a declared attribute or a global one |
| R5 | `no-unknown-property` | off / warn | | `:prop="…"` names a declared property |
| R6 | `no-unknown-event` | **warn** / warn | | `@event="…"` names an event the component emits |
| R7 | `no-unknown-slot` | off / warn | | `slot="…"` names a slot declared with JSDoc `@slot` |
| R8 | `no-unintended-mixed-binding` | warn / warn | | `?x="${v}/"` — a stray `/`, `"`, `'` or `}` swept into the value |
| R9 | `no-expressionless-property-binding` | error / error | | `:prop="literal"` with no `${…}` — a property binding needs an expression |
| R10 | `no-invalid-attribute-name` | error / error | | `@attr({ attribute: "…" })` is a legal attribute name |
| R11 | `no-invalid-tag-name` | error / error | | A registered tag name is a legal custom element name |
| R12 | `no-missing-element-type-definition` | off / off | | The component is on `HTMLElementTagNameMap` |
| R13 | `no-invalid-css` | warn / error | | `` css` ` `` contents, via `vscode-css-languageservice` |
| R14 | `no-noncallable-event-binding` | error / error | ● | `@event="${…}"` binds something callable |
| R15 | `no-boolean-in-attribute-binding` | error / error | ● | `attr="${bool}"` — sets the string `"false"`, which is truthy |
| R16 | `no-complex-attribute-binding` | error / error | ● | `attr="${obj}"` — sets `"[object Object]"`. `:prop` was meant |
| R17 | `no-incompatible-type-binding` | error / error | ● | The bound expression's type fits the target member's |

One default changes. **R6 `no-unknown-event` moves from `off`/`off` to `warn`/`warn`**, because event
bindings are the most common binding in the corpus — 74 of ~163
([research/corpus.md §1](../research/corpus.md#1-what-is-there)) — and a misspelled event name is
completely silent at runtime. It is `off` upstream because lit's event model made it noisy; FAST's
`$emit` gives a definite list. Revisit if it fires on correct code in the corpus.

## Rewritten

### no-invalid-directive-binding

**R18** · error / error · oracle

**Lit version**: checks lit-html's directive table (`classMap`, `ifDefined`, `repeat`, `until`, …)
against the position each is legal in.

**FAST version**: same idea, FAST's set
([research/fast-element.md §7](../research/fast-element.md#7-directives)).

| Directive | Legal position |
| --- | --- |
| `when`, `repeat`, `render` | content only |
| `ref`, `slotted`, `children` | element expression only — between attributes |

```ts
html<X>`<div class="${ref('el')}">`      // ✗ ref in an attribute value
html<X>`<div>${ref('el')}</div>`         // ✗ ref in content
html<X>`<div ${when(…)}>`                // ✗ when as an element expression
html<X>`<div ${ref('el')}>${when(…)}`    // ✓
```

Identification is by resolved symbol, not by the type-name heuristic fast-analyzer uses
(`CaptureType` / `HTMLBindingDirective` / `StatelessAttachedAttributeDirective`), which cannot tell
one directive from another and therefore cannot say anything about position.

### no-incompatible-attr-config

**R19** · warn / error · oracle

**Lit version** (`no-incompatible-property-type`): checks `@property({ type: Boolean })` against the
declared TypeScript type.

**FAST version**: FAST has no `type`; it has `mode` and `converter`
([research/fast-element.md §4](../research/fast-element.md#4-member-declarations)).

| Written | Reported when |
| --- | --- |
| `@attr({ mode: "boolean" }) x: string` | The property is not `boolean` — boolean mode reflects presence/absence |
| `@attr({ mode: "reflect" }) x: object` | The value has no useful string form; a converter is needed |
| `@attr({ converter: c }) x: T` | `c`'s `fromView` return type is not assignable to `T` |
| `@attr x: SomeObject` | Same as reflect-with-no-converter |

### no-attr-visibility-mismatch

**R20** · off / warn

**Lit version** (`no-property-visibility-mismatch`): public members should use `@property`,
non-public should use `@internalProperty`.

**FAST version**: FAST has no internal-property decorator; `@attr` is the public/reflected one and
`@observable` is the internal one. So the rule is:

| Written | Reported |
| --- | --- |
| `@attr private x` / `@attr protected x` | An attribute is part of the public DOM contract; a private member cannot be |
| `@observable public x` | Not reported. A public observable is an ordinary, correct thing in FAST |

Only the first direction survives, which makes this a smaller rule than lit's. That is the correct
size for it.

## New

### no-non-reactive-binding

**F1** · warn / error · oracle

The highest-value rule in this document. FAST binds a **function** reactively and anything else
**once, at view-creation time**
([research/fast-element.md §9](../research/fast-element.md#9-reactivity-and-the-mistake-it-invites)):

```ts
html<MyEl>`<span>${x => x.count}</span>`      // ✓ updates
html<MyEl>`<span>${myEl.count}</span>`        // ✗ frozen for the life of the module
html<MyEl>`<button ?disabled="${x.locked}">`  // ✗ same, and looks even more like it works
```

Both compile, both are legal FAST, and there is no equivalent in lit — `${this.count}` inside a lit
`render()` is re-evaluated by definition, which is why no inherited rule covers this.

Reported when an interpolated expression is an **identifier or property
access** whose type has no call signatures, whose symbol is not a `const`,
and which is not a directive, template or `Binding` value. The restriction to
value-*reads* was forced by the corpus (the rule's first shape would have
fired on it): `${SOME_CONST}`, `${'literal'}` and `${someEnum.Value}` are
exempt as constants, and a **call** like `${shortcut('Ctrl+F', '⌘F')}` — six
occurrences in csv-ultra's titles — is a deliberate one-time interpolation of
a computed value and is exempt by shape. The mistake this rule exists for is
*reading* something that looks reactive; computing something is not that
mistake. The constness and callability tests are computed at extraction and
travel with the placeholder (architecture.md §3.1), so the rule decides in
Rust with no oracle round trip. When either half is unknown, the rule stays
silent rather than guessing.

Its default is resolved (open question 4, closed): **`warning` normal /
`error` strict** — safe because of the shape restriction above, which is what
kept the corpus gate silent.

### no-invalid-directive-target

**F2** · error / error

`ref`, `slotted` and `children` take a **property name as a string**
([research/fast-element.md §7](../research/fast-element.md#7-directives)). TypeScript cannot check
that string against the class. We can — the template's `TSource` is known
([component-model.md §6](component-model.md#6-template-source-types)).

```ts
class CsvGrid extends FASTElement { findInput!: HTMLInputElement; }

html<CsvGrid>`<input ${ref('findInput')}>`   // ✓
html<CsvGrid>`<input ${ref('findInpt')}>`    // ✗ no such member — did you mean 'findInput'?
```

With a quick fix offering the nearest member name, and a `slotted`/`children` variant that also
checks the member's type is a node collection. 17 `ref('…')` calls in the corpus have no checking of
any kind today.

### no-slot-without-shadow-root

**F3** · warn / warn

`shadowOptions: null` puts a component in the light DOM, where `<slot>` does nothing. The template
still compiles and the slotted content silently never appears.

```ts
@customElement({ name: 'x-thing', template, shadowOptions: null })
// template contains <slot></slot>                                  ← ✗
```

typst-ultra uses `shadowOptions: null` today, so this is a live configuration in the corpus.

Also covers `::part` in the component's `` css` ` ``, and `slotted('…')` (F2's sibling), both of
which are shadow-DOM-only.

### no-duplicate-tag-name

**F4** · error / error

Two components in the program registering the same tag name. FAST throws at registration time, so
this is a crash the analyzer can see coming. The registry already knows
([component-model.md §4](component-model.md#4-what-rust-does-with-the-facts)); the rule is reporting
what it knows.

Reported on both declarations, each pointing at the other.

### no-untyped-template

**F5** · off / warn

```ts
const t = html`<div>${x => x.anything}</div>`;    // TSource = any: nothing is checked
const t = html<MyEl>`<div>${x => x.anything}</div>`;
```

When `TSource` defaults to `any`, TypeScript checks neither the member names nor the return types
inside the bindings, and neither can we
([research/fast-element.md §8](../research/fast-element.md#8-template-typing)). The template is a
blind spot, and the fix is one type argument.

Reported only when the template is **used as a component's `template`** — an untyped `html` fragment
composed into a larger template is normal and gets its source type from its parent. Quick fix:
insert the type argument, inferred from the component that references the template.

Defaults to `off` because it is a style rule for existing code and would fire on every untyped
template in a codebase at once. All 26 templates in the corpus are already typed, so it starts silent
here — which is the steady state it is aiming at.

### no-implicit-prevent-default

**F6** · off / off

A FAST event binding calls `preventDefault()` **unless the handler returns `true`**
([research/fast-element.md §10](../research/fast-element.md#10-event-handlers-and-preventdefault)).
For `@click` that is usually wanted. For `@keydown` on a text input it eats every keystroke — which
is exactly what happened in csv-ultra, and why its template carries a 12-line comment and a `keys()`
wrapper.

Reported when a handler bound to a key or input event returns something that is not `true`. Off by
default in both modes because the behaviour is intended as often as not; it earns its place as
something a team can turn on after being bitten once.

## Dropped

| Rule | Why |
| --- | --- |
| `no-nullable-attribute-binding` | FAST removes an attribute on `null`/`undefined` rather than coercing it to `"null"`. The rule is a false positive by construction — fast-analyzer already forced it to `off`/`off` *and* patched the shared assignability helper to neutralise it ([research/parity.md §3](../research/parity.md#3-the-rule-set)) |
| `no-legacy-attribute` | Polymer's `foo$=` syntax. Not FAST, not lit, not ours |
| `no-invalid-boolean-binding` | A dead id: a default severity and a settings entry, referenced by no code in fast-analyzer |

`securitySystem` — the `ClosureSafeTypes` setting that gates
`is-assignable-binding-under-security-system` — goes with them. FAST's equivalent mechanism is
`DOMPolicy`, which is different enough that a rule for it is new work rather than parity
([research/fast-element.md §6](../research/fast-element.md#6-dom-policy)).

## Quick fixes

Every rule that can name the right answer offers one. Carried from fast-analyzer where it exists,
new where the rule is new:

| Rule | Fix |
| --- | --- |
| R1 `no-unknown-tag-name` | Rename to the nearest known tag |
| R2 `no-missing-import` | Add the import |
| R3 `no-unclosed-tag` | Close the tag |
| R4/R5/R6 unknown attribute/property/event | Rename to the nearest known name; or add to `globalAttributes`/`globalEvents` |
| R9 `no-expressionless-property-binding` | Drop the `:` — it was meant to be an attribute |
| R12 `no-missing-element-type-definition` | Add the `HTMLElementTagNameMap` entry |
| R16 `no-complex-attribute-binding` | Add the `:` — it was meant to be a property |
| F1 `no-non-reactive-binding` | Wrap in an arrow: `${x => …}` |
| F2 `no-invalid-directive-target` | Rename to the nearest member |
| F5 `no-untyped-template` | Insert the type argument |

Nearest-name suggestions come from `strsim` in Rust, replacing `didyoumean2`.

## Suppression

`// @ts-ignore` on the line before a template line, as fast-analyzer supports, plus
`fastElementUltra.dontShowSuggestions` to strip the "did you mean" tails from messages. Rule
severities are per rule via `fastElementUltra.rules.<id>`, and `strict` selects the second column of
every default above.
