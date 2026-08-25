# The component model

**Status**: living — implemented in `tsplugin/extract.ts`, exercised by
`test/discovery.test.ts` (every row of §5's table, including the ❌ rows),
`test/tag-name-map.test.ts` (§2.3) and the corpus gate. Two facts pinned from source while implementing: the default
attribute name is the property name **lowercased** (fast-element
`AttributeDefinition`: `attribute = name.toLowerCase()`), and `{ template }`
shorthand properties resolve through
`checker.getShorthandAssignmentValueSymbol` — plain `getSymbolAtLocation`
returns the property symbol and silently breaks the template link.

How a FAST element is discovered, what is known about it, and where each fact comes from. This is
the part [proposal.md §1.2](../proposal.md#12-the-bolt-on-does-not-fit) argues is the weakest point
of the current tool, so it is specified before anything else.

The syntax being modelled is documented in [research/fast-element.md](../research/fast-element.md);
this page is about extracting it.

---

## 1. Where discovery happens

**In TypeScript**, in the plugin, not in Rust. Decorators, class members, inheritance and `const`
resolution all want the AST and the checker, and a second parser would be a second source of truth
that drifts ([0002](../decisions/0002-rust-engine-typescript-oracle.md)). Rust receives
`ComponentFact`s and owns the registry, the merge order, and every lookup.

Discovery runs per source file, on `upsertFile`, and replaces everything that file previously
contributed.

## 2. Registration

Three forms, all supported ([research/fast-element.md §3](../research/fast-element.md#3-element-registration)):

```ts
@customElement("my-tag")                                   // A
@customElement({ name: "my-tag", template, styles })       // B
@customElement({ name: MY_TAG, template })                 // B, name from a const  ← the corpus case
MyElement.define({ name: "my-tag", template });            // C
FASTElement.define(MyElement, "my-tag");                   // C
```

### 2.1 Resolving the name

The name is read **through the checker**, not off the AST:

```ts
const type = checker.getTypeAtLocation(nameExpression);
if (type.isStringLiteral()) return type.value;
```

This handles a string literal, a `const` in the same file, a `const` imported from another module,
and an `as const` member access — all of which are the same thing to the checker and four different
AST shapes to a syntactic matcher. It is the fix for the bug in
[corpus.md §2](../research/corpus.md#2-why-fast-analyzer-finds-none-of-it), and it is why discovery
belongs on the TypeScript side.

A name that is not a string-literal type (a template literal, a computed value, a function call) is
not resolvable. The component is still registered — with its members, since those are still useful
to rename and hover — but with `tagName: null`, and it is excluded from tag-name lookups. Silently
dropping the whole component, which is today's behaviour, is the thing to avoid.

### 2.2 Which decorator is `customElement`?

By resolved symbol, not by name. `checker.getSymbolAtLocation` on the decorator's identifier, then
check the declaration's source file is `@microsoft/fast-element`. A local function called
`customElement` is not FAST's, and a renamed import (`import { customElement as element }`) is.

Same test for `attr`, `observable`, `volatile`, and the directives.

### 2.3 The fourth form: `HTMLElementTagNameMap`

Some libraries never write any of A/B/C at a place a file walk can read. A design system that
registers through its own wrapper —

```ts
export function defineToaster(options?: DefineComponentOptions): Promise<typeof Toaster> {
  return defineComponent(toasterBlueprint, options);   // tag = `${prefix}-${baseName}`
}
```

— has no decorator, no literal name, and its one real `.define` call sits inside the generic
wrapper where the receiver is a type parameter and the name is a runtime concatenation. §2.1 cannot
help: there is no string-literal type anywhere in the program. The same library consumed as a built
package is worse still, since the plugin sees only its `.d.ts`.

What such a library does have — or should, since `no-missing-element-type-definition` asks every
component for it — is the tag-name-map augmentation:

```ts
declare global {
  interface HTMLElementTagNameMap {
    "fui-toaster": Toaster;
  }
}
```

That names the tag *and* points at the class, which is everything a template needs. So discovery has
a fourth source: every dashed property of the merged `HTMLElementTagNameMap` whose type reaches
`FASTElement`, for tags no source file declared (a real declaration always wins — its facts are
exact). It is deliberately the only path that reads declaration files, and so the only one that
reaches into an installed package.

Three things follow from the class being someone else's:

- **No diagnostics.** The member rules run to collect facts and their output is dropped: an
  `@attr` mismatch in a published package is not this project's to fix.
- **Members without decorators.** A `.d.ts` keeps `position: ToastPosition` but not the `@attr`
  that made it an attribute. When the class carries no FAST decorators at all, every public member
  is offered as a property and the ones with an attribute-shaped type as an attribute too — a
  library's real `@attr` is never reported unknown, at the cost of accepting a few that are not.
- **No import rule.** `no-missing-import` is skipped: the augmentation is ambient, and the
  registration happened wherever the app was told to run it.

An entry whose type names no class of its own — the styled containers built from a factory, typed
as plain `FASTElement` — registers the tag with no members, so it takes global attributes and
nothing else. The facts live in one synthetic registry file (`fast-element-ultra:tag-name-map`),
recomputed whenever the program changes.

## 3. Members

| Source | Produces | Notes |
| --- | --- | --- |
| `@attr prop` | attribute + property | Attribute name defaults to the property name |
| `@attr({ attribute: "x" }) prop` | attribute `x` + property `prop` | |
| `@attr({ mode: "boolean" }) prop` | boolean attribute | Changes what `no-incompatible-attr-config` expects |
| `@attr({ mode: "fromView" }) prop` | attribute, read-only from the DOM's side | |
| `@attr({ converter }) prop` | attribute whose DOM type is the converter's | Type comes from the converter's signature when resolvable, `any` otherwise |
| `@observable prop` | property | No attribute |
| `@volatile get prop()` | property | Getter — the declaration form fast-analyzer skips entirely |
| `attributes: [...]` in the definition | attributes | `(AttributeConfiguration \| string)[]`; no decorator involved |
| JSDoc `@attr` / `@prop` | attribute / property | For members that cannot be seen, e.g. set by a mixin |
| JSDoc `@fires` | event | An optional leading `{Type}` is the detail type — text, not a checked type |
| JSDoc `@slot` | slot | The **only** source of slot names — `no-unknown-slot` has nothing else |
| JSDoc `@csspart` / `@cssprop` | CSS part / custom property | |
| `$emit("name", detail)` | event | Name from arg 0, `detail` type from arg 1; see §3.2 |
| `declare $events: { … }` | events | Names and detail types read through the checker; `void` = no detail |
| `HTMLElementEventMap` augmentation | events, on every tag | Global by construction, like the interface itself; see §3.2 |

All of these apply to property declarations, **get/set accessors**, and members declared on any class
in the inheritance chain.

### 3.1 Inheritance

Walk `checker.getBaseTypes()` from the component class up to but not including `FASTElement`,
collecting members at each level. A member declared lower in the chain shadows one declared higher;
the shadowing declaration is what go-to-definition and rename target.

Mixins — `class X extends SomeMixin(FASTElement)` — arrive as an intersection or a synthesised base
type. Where the checker gives us a declaration, we use it; where it does not, the member is still
registered from the type, with no declaration and therefore no go-to-definition. Half a fact is
better than none, and the `origin` field records which it is.

This is the single largest gap against fast-analyzer, which reads exactly one class body.
As implemented, the chain walk follows explicit class declarations (capped at
16 levels) and stops at fast-element; a mixin whose base the checker cannot
hand back as a class declaration contributes nothing — the accepted half-fact
is narrower than designed, and `origin: 'inherited'` marks what came from
above.

### 3.2 Events

An event is found by the *type of the `$emit` receiver*, not by where the call is written. The
idiomatic place to raise one is the template, not the class body: `x.$emit("tab-add")` on the host,
`c.parent.$emit("tab-select", …)` from inside a `repeat` item template — where `c.parent` is the only
route back to the host. Neither is spelled `this.$emit` and neither is lexically inside the class, so
a class-body walk finds nothing and every such event is reported unknown at its listener.

So the file is indexed once — every `$emit` call in it, keyed by the class declaration its receiver's
type names — and each component takes the entries for its own chain. Typing the receiver is also what
keeps a neighbouring `otherEl.$emit(…)` off this component; a receiver whose type names no class in
the chain is dropped rather than guessed at.

The index covers one file. A base class in another file still contributes its `this.$emit` calls
(walked directly), but an event that class raises from its own template is out of reach and wants
`@fires` or `$events`.

Three per-component sources, then, and they **merge** rather than compete: the first to name an event
owns the fields it fills, and a later one fills what is still empty. `@fires` prose on an event the
class also emits keeps the emit's detail type and gains the description, which is what someone
writing both meant. Precedence within a class: the declared `$events` map (a real type behind every
name), then the emit index, then the class's JSDoc.

A fourth source belongs to no component. `declare global { interface HTMLElementEventMap { … } }` is
how a library types its events for `addEventListener`, so a project that has augmented it has already
said what it dispatches — and said it of every `HTMLElement`, which is what the interface means.
Taken at face value: the names are accepted on any tag, exactly as `globalEvents` in the config is,
but with a declaration behind them, so hover shows the detail (a `CustomEvent<T>` unwrapped to `T`)
and go-to-definition lands on the entry. The merged map is read once per program alongside the
tag-name map (§2.3) and lands in the same synthetic file; `lib.dom`'s own entries are skipped, being
`fast-html-data`'s job already.

### 3.3 Types

A member's type crosses to Rust as an **interned id**, never as a structure
([architecture.md §3.1](architecture.md#31-what-rust-receives)). Rust needs to know that two members
have the same type only to deduplicate; every real question about a type goes back to the plugin as a
binding fact.

## 4. What Rust does with the facts

The registry is keyed by tag name, with an entry per contributing file so that removing a file
removes exactly its contribution. Merge order, highest confidence first:

1. Components declared in the program (`origin: decorator | define`)
2. JSDoc-declared members on those components
2b. Components known only through `HTMLElementTagNameMap` (`origin: tagNameMap`, §2.3) — a
   declaration for the same tag replaces them outright rather than merging
3. VS Code custom data (`fastElementUltra.customHtmlData`, `html.experimental.customData`)
4. `globalTags` / `globalAttributes` / `globalEvents` — "assume this exists, check nothing" — and the
   program's `HTMLElementEventMap` augmentation (§3.2), which is the same claim with a declaration
   behind it
5. Built-in HTML/SVG data from `fast-html-data`

A lower level never overrides a higher one; it fills gaps. Two files declaring the same tag name is
a real condition — a duplicate registration is a runtime error in FAST — and is reported by
`no-duplicate-tag-name` ([rules.md](rules.md#no-duplicate-tag-name)).

## 5. What fast-analyzer covers today

For comparison, and as the checklist Phase 2 is done against. Sources:
`flavors/fast-element-analyzer.ts` (250 lines) and `ts-lit-plugin.ts`'s `extractTagNameFromClass`.

| Construct | fast-analyzer | RFC 011 |
| --- | :---: | :---: |
| `@customElement("my-tag")` | ⚠️ rename path only | ✅ |
| `@customElement({ name: "my-tag" })` | ✅ | ✅ |
| `@customElement({ name: CONST })` | ❌ | ✅ |
| `MyEl.define(…)` / `FASTElement.define(…)` | ❌ | ✅ |
| Decorator identified by resolved symbol | ❌ — matches `escapedText === "customElement"` | ✅ |
| `@attr` on a property | ✅ | ✅ |
| `@attr({ attribute })` | ✅ | ✅ |
| `@attr({ mode })` / `@attr({ converter })` | ❌ | ✅ |
| `@observable` on a property | ✅ | ✅ |
| `@attr` / `@observable` on an accessor | ❌ | ✅ |
| `@volatile` | ❌ | ✅ |
| `attributes: [...]` in the definition | ❌ | ✅ |
| Inherited members | ❌ | ✅ |
| Mixin members | ❌ | ⚠️ where the checker gives them |
| JSDoc `@slot` / `@fires` / `@csspart` / `@cssprop` | ⚠️ only via WCA, which does not recognise FAST | ✅ |
| `this.$emit("name")` | ✅ name only | ✅ name + `detail` type |
| `shadowOptions: null` → light DOM | ❌ | ✅ |
| `template` / `styles` → which template belongs to which component | ❌ | ✅ (§6) |
| Second component model (`web-component-analyzer`) merged over the top | ✅ | ❌ — removed |

## 6. Template source types

Knowing which type `x` has at a point in a template is half of what this tool does, and it is not a
per-file property. It is per *template*, and `repeat` changes it
([corpus.md §3](../research/corpus.md#3-constructs-the-corpus-exercises-that-the-design-must-handle)):

```ts
html<CsvGrid>`
  …
  ${repeat(x => x.menu.items, html<MenuItem, CsvGrid>`
      <button @click="${(item, c) => c.parent.pick(item)}">   ← item: MenuItem, c.parent: CsvGrid
  `)}
`
```

So each `VirtualDocument` carries its own `sourceTypeId` and `parentTypeId`, taken from the tag
expression's type arguments. Three cases:

| Written | `TSource` | Consequence |
| --- | --- | --- |
| `html<CsvGrid>` | `CsvGrid` | Members resolvable; TypeScript also checks the expression bodies |
| `html<MenuItem, CsvGrid>` | `MenuItem` | Members resolve against `MenuItem`; `c.parent` against `CsvGrid` |
| ``html` ` `` | `any` | **Nothing is checked, by TypeScript or by us.** `no-untyped-template` |

When a component's definition names a template — `@customElement({ name, template })` — the link is
recorded both ways: the template knows its component, and the component knows which template
declares its markup. That is what lets a rename of `CsvGrid.hasHeader` reach the `:prop` bindings and
the `${ref('…')}` strings in a *different file*, which is the corpus's actual layout
(`element.ts` and `template.ts` are separate modules).

## 7. Invalidation

By file. `upsertFile(fileName, …)` replaces that file's whole contribution; `removeFile` drops it.
A component whose members are spread across a base class in another file is re-derived when either
file changes, because both files' `ComponentFact`s name the same tag.

The registry is not incremental below file granularity, and should not become so without a
measurement saying it must — tsserver already hands us change notifications at file granularity, and
a finer model would be inventing precision the host does not have.
