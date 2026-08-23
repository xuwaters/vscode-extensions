# FAST Element 3.x: the syntax and API of record

**Source**: [`temp/microsoft-fast`](../../../../temp/microsoft-fast) — `@microsoft/fast-element`
**3.0.2**, MIT. Read on 2026-08-22. Line references are into that tree.

This page is the specification RFC 011 implements against. Where the analyzer and this page disagree,
this page is wrong and should be corrected from source — not the other way round.

---

## 1. Binding aspects

The prefix on an attribute name decides what the binding does. From
`src/templating/html-directive.ts:203-241` (`assignAspect`):

| Written | `DOMAspect` | Effect |
| --- | --- | --- |
| `class="${…}"` | `attribute` | `DOM.setAttribute(target, name, value)` |
| `:value="${…}"` | `property` | `target[name] = value` |
| `:classList="${…}"` | `tokenList` | special-cased in the same switch — `classList` with versioning |
| `?disabled="${…}"` | `booleanAttribute` | `DOM.setBooleanAttribute(target, name, value)` |
| `@click="${…}"` | `event` | `addEventListener(name, directive, options)` |
| `${…}` in text position | `content` | text node, or a composed view |

Note what is **not** here: lit's `.prop=` has no meaning in FAST, and FAST's `:prop=` has no meaning
in lit. This is the single most important syntactic difference, and it is why
[0009](../decisions/0009-no-lit-compatibility.md) refuses to support both.

`:classList` deserves its own handling: it is the one attribute name whose *value* changes the aspect
type. A rule checking `:classList` against a declared `classList` property would be wrong.

## 2. Attribute-removal semantics

FAST's `attribute` sink removes the attribute when the value is `null` or `undefined`, rather than
coercing to the string `"null"`. This is the behavioural difference that makes lit's
`no-nullable-attribute-binding` a false positive against FAST templates
([research/parity.md §3](parity.md#3-the-rule-set)) and why RFC 011 drops it.

## 3. Element registration

Three forms, all live in 3.0.2:

```ts
// 1. Decorator, string form                    src/components/fast-element.ts:232
@customElement("my-tag")
class MyElement extends FASTElement {}

// 2. Decorator, definition form                (the same function, object argument)
@customElement({ name: "my-tag", template, styles, shadowOptions, elementOptions, attributes })
class MyElement extends FASTElement {}

// 3. Imperative                                src/components/fast-element.ts:115-125 (3 overloads)
MyElement.define({ name: "my-tag", template });
FASTElement.define(MyElement, "my-tag");
```

`customElement(nameOrDef)` simply calls `define(type, nameOrDef)`, so the three are the same
mechanism. `PartialFASTElementDefinition` (`src/components/fast-definitions.ts:318`) is:

| Field | Type | Matters to us because |
| --- | --- | --- |
| `name` | `string` | The tag name. **Frequently a `const`, not a literal** — see [corpus.md §2](corpus.md#2-why-fast-analyzer-finds-none-of-it) |
| `template` | `ElementViewTemplate` | Links a component to the template whose `TSource` it is |
| `styles` | `ComposableStyles \| ComposableStyles[]` | Where `` css` ` `` documents attach |
| `attributes` | `(AttributeConfiguration \| string)[]` | Declares attributes **without** a decorator — a whole discovery path fast-analyzer has no code for |
| `shadowOptions` | `Partial<ShadowRootOptions> \| null` | `null` means light DOM: no slots, no `::part` |
| `elementOptions` | `ElementDefinitionOptions` | `extends`, for customised built-ins |

## 4. Member declarations

```ts
class MyElement extends FASTElement {
  @attr myAttr = "";                                  // attribute + observable property
  @attr({ attribute: "my-name" }) renamed = "";       // explicit attribute name
  @attr({ mode: "boolean" }) disabled = false;        // AttributeMode
  @attr({ mode: "fromView" }) value = "";
  @attr({ converter: myConverter }) parsed = null;
  @observable internal = 0;                           // reactive property, no attribute
  @volatile get computed() { … }                      // recomputes its dependency graph each read
}
```

- `AttributeMode = "reflect" | "boolean" | "fromView"` (`src/components/attributes.ts:41`),
  default `"reflect"`.
- `AttributeConfiguration` is `{ property, attribute?, mode?, converter? }` (`:49`).
- `attr` has two overloads (`:332`, `:342`): `@attr` bare and `@attr(config)`.
- Decorators are **legacy** decorators. Any TypeScript that reads them must handle both
  `ts.getDecorators` and the pre-5.0 `node.decorators` shape, as fast-analyzer's `getDecorators`
  helper does.

`mode` matters for rules: a `"boolean"` attribute reflects presence/absence, so binding a string to
it is a mistake; `"fromView"` never writes back to the DOM. This is FAST's analogue of lit's
`@property({type})`, and it is what `no-incompatible-attr-config` checks
([design/rules.md](../design/rules.md#no-incompatible-attr-config)).

## 5. Events

`$emit` is defined on the class `createFASTElement` builds (`src/components/fast-element.ts:82`):

```ts
public $emit(type: string, detail?: any, options?: Omit<CustomEventInit, "detail">): boolean | void
```

So an event's name comes from the first argument of a `this.$emit("…")` call, and its `detail` type —
which a template's `@name="${(x, c) => c.event}"` handler would want — comes from the second.
fast-analyzer records the name and types the event as `ANY`; recovering `detail` is a Phase 3
improvement.

## 6. DOM policy

FAST's injection defence is `DOMPolicy` (`src/dom-policy.ts`), a per-template or per-binding object
that can veto a sink. It is not lit's `ClosureSafeTypes` sanitizer, so the `securitySystem` setting
does not carry over ([research/parity.md §4](parity.md#4-configuration-surface)). A policy-aware rule
is possible and is deferred in [proposal.md §10](../proposal.md#10-deliberately-deferred).

## 7. Directives

Exported from `src/index.ts`:

| Directive | Signature source | Valid position |
| --- | --- | --- |
| `when(condition, template, elseTemplate?)` | `src/templating/when.ts:20` | content |
| `repeat(items, template, options?)` | `src/templating/repeat.ts:582` | content |
| `render(…)` | `src/templating/render.ts:743` | content |
| `ref(propertyName)` | `src/templating/ref.ts:34` | **element expression** — between attributes |
| `slotted(propertyNameOrOptions)` | `src/templating/slotted.ts:63` | element expression, on a `<slot>` |
| `children(propertyNameOrOptions)` | `src/templating/children.ts:109` | element expression |

`ref`, `slotted` and `children` take a **property name as a string**. TypeScript cannot check that
string against the class, and we can — this is the basis of `no-invalid-directive-target`
([design/rules.md](../design/rules.md#no-invalid-directive-target)) and of the string-literal
completions in [design/features.md §3](../design/features.md#3-completion).

fast-analyzer recognises directives by type name
(`is-lit-directive.ts`: `CaptureType`, `HTMLBindingDirective`, `StatelessAttachedAttributeDirective`),
which is a reasonable fallback but says nothing about *which* directive it is, and therefore nothing
about whether it is in a legal position.

## 8. Template typing

```ts
// src/templating/template.ts:372
export type HTMLTemplateTag = (<TSource = any, TParent = any>(
    strings: TemplateStringsArray,
    ...values: TemplateValue<TSource, TParent>[]
) => ViewTemplate<TSource, TParent>) & { partial(html: string): InlineTemplateDirective };
```

Two type parameters, both defaulting to `any`:

- **`TSource`** — what `x` is in `x => x.foo`.
- **`TParent`** — what `c.parent` is in `(x, c) => c.parent.foo`, used by `repeat`'s item templates.

`html.partial(str)` interpolates a raw HTML string. It is an escape hatch that defeats analysis by
construction; the design's answer is to stop analysing a template that uses it rather than to guess.

**When `TSource` defaults to `any`, TypeScript checks nothing inside the bindings** — not the member
names, not the return types. That is what makes an untyped template worth a diagnostic of its own
([design/rules.md](../design/rules.md#no-untyped-template)), and it is the one place where a FAST
template is meaningfully *less* safe than a lit one, where `this` is always typed.

## 9. Reactivity, and the mistake it invites

`ViewTemplate.create` (`src/templating/template.ts:335-343`) branches on what each interpolated value
is. A **function** becomes `oneWay(…)` — a binding re-evaluated whenever its dependencies change.
A `Binding` instance or a registered `HTMLDirective` is used as given. **Anything else** falls through
to `oneTime(() => staticValue)` and is **bound once, at view-creation time**:

```ts
} else if (!(definition = HTMLDirective.getForInstance(currentValue))) {
    const staticValue = currentValue;
    currentValue = new HTMLBindingDirective(oneTime(() => staticValue));
}
```

```ts
html<MyEl>`<span>${x => x.count}</span>`   // updates
html<MyEl>`<span>${myEl.count}</span>`     // frozen at the value it had when the module ran
```

Both compile. Both are legal FAST. Only one is usually what was meant. There is no equivalent trap in
lit, where `${this.count}` inside a component's `render()` is re-evaluated by definition — which is
precisely why no inherited rule covers it, and why `no-non-reactive-binding`
([design/rules.md](../design/rules.md#no-non-reactive-binding)) is proposed as the highest-value
new rule in RFC 011.

## 10. Event handlers and `preventDefault`

A FAST event binding calls `preventDefault()` on the event **unless the expression returns `true`**.
This is load-bearing enough that csv-ultra carries a 12-line comment and a `keys()` wrapper around it
(`extensions/csv-ultra/webview/viewer/template.ts:22-38`) after it ate every keystroke in three text
inputs.

An advisory rule is proposed at [design/rules.md](../design/rules.md#no-implicit-prevent-default),
default `off`, because the behaviour is intended as often as not.

## 11. Declarative templates (out of scope)

`src/declarative/` implements FAST templates written in plain HTML files, with a different syntax
(`src/declarative/syntax.ts`):

```
{{ expression }}      {{{ unescaped }}}      $c (execution context)      $e (event arg)
<f-repeat>  </f-repeat>      <f-when>  </f-when>      f-* attribute directives
```

Out of scope for RFC 011 — it is not part of fast-analyzer, so it is not part of parity — and listed
as the most likely follow-up in [proposal.md §10](../proposal.md#10-deliberately-deferred). It needs
no type oracle, which makes it almost pure Rust work on top of the parser crate. The parser's tree
should not assume its input came from a tagged template.
