# FAST Element Ultra

FAST Element templates, checked. The `html``` and `css``` tagged templates of
[`@microsoft/fast-element`](https://github.com/microsoft/fast) are strings as far
as TypeScript is concerned; this extension makes them a language surface —
diagnostics, completion, hover, go-to-definition, find-all-references, rename,
quick fixes, closing tags and folding — with your project's own types answering
the questions only they can answer.

Nothing to configure and nothing to start: open a TypeScript file with a FAST
template in it and the templates are checked alongside the code around them.

## What it does

**Diagnostics** — 26 rules describing FAST's semantics, not lit's:

- Structure: unknown tags, unclosed tags (custom elements are never
  self-closed), unknown attributes/properties/events/slots with
  did-you-mean suggestions, invalid tag and attribute names, duplicate
  registrations (a runtime error in FAST).
- FAST's own traps: **`no-non-reactive-binding`** catches `${x.foo}` written
  where `${x => x.foo}` was meant — both compile, one is frozen forever.
  `no-invalid-directive-binding` knows `when`/`repeat`/`render` are content
  directives and `ref`/`slotted`/`children` are element directives.
  **`no-invalid-directive-target`** checks the string inside `ref('…')`
  against the template's source type. `no-slot-without-shadow-root` knows
  `shadowOptions: null` means `<slot>` and `::part` do nothing.
  `no-untyped-template` flags a component template with no `html<T>`.
  `no-implicit-prevent-default` (opt-in) knows a handler that does not
  return `true` cancels the event's default action.
- Types, answered by the compiler: event bindings must be callable, a boolean
  bound to an attribute sets the string `"false"`, an object sets
  `"[object Object]"`, and every binding is checked against the declared
  member type — with FAST's attribute-removal semantics (null/undefined
  remove the attribute) modelled, not patched around.
- `css``` templates are validated by VS Code's own CSS language service.

**The component model** covers what FAST actually ships: all three
registration forms — including `@customElement({ name: SOME_CONST })`, the
form this repo's own extensions use — plus `HTMLElementTagNameMap` for the
elements no registration can be read from (see
[Custom elements](#custom-elements)); `@attr` in every mode, on properties
and accessors, `@observable`, `@volatile`, `attributes: [...]` in the
definition, members inherited through the class chain, `$emit(...)` events
with their detail types (see [Custom events](#custom-events)), and JSDoc
`@slot`/`@fires`/`@csspart`/`@cssprop`.

**IDE features** the compiler cannot provide: completion *inside*
`ref('…')`/`slotted('…')`/`children('…')` string arguments with the source
type's members; renaming a member updates its `:prop` bindings and its
`ref('…')` strings across files, in both directions; go-to-definition from a
template attribute to the `@attr` member; find-all-references from a tag to
every template that uses it.

**Suppression**: a `@ts-ignore` (or `@fast-ignore`) comment on the previous
line silences a template diagnostic.

## Custom elements

A tag is known when the extension can find where it was defined. There are two
ways to say so — one for a component this project registers, one for a
component whose registration is unreadable — and they are tried in that order:
a real registration always wins, since its facts are exact.

**1. Register it.** All three of FAST's forms are read, and the tag name comes
from the *type checker* rather than off the syntax, so a constant works as
well as a literal — including one imported from another module, which is the
form this repo's own extensions use:

```ts
import { TAB_BAR_TAG } from './tags';   // export const TAB_BAR_TAG = 'tab-bar';

@customElement({ name: TAB_BAR_TAG, template, styles })
export class TabBar extends FASTElement {}
```

The other spellings register the same tag — `@customElement('tab-bar')`,
`TabBar.define({ name: 'tab-bar', template })`, and
`FASTElement.define(TabBar, 'tab-bar')` (register it once, though:
`no-duplicate-tag-name` reports the second, as FAST throws on it at runtime).

The decorator is identified by resolved symbol, not by name: a renamed import
(`import { customElement as element }`) is FAST's, and your own local function
called `customElement` is not. Same test for `@attr`, `@observable`,
`@volatile` and the directives. A name that is not a string-literal type — a
template literal, a concatenation, a `defineFoo()` call — leaves the class
registered with its members, so hover and rename still work, but with no tag
name to match a template against; give it form 2.

**2. Augment `HTMLElementTagNameMap`.** The standard-DOM route, and the only
one that reaches into an installed package:

```ts
declare global {
  interface HTMLElementTagNameMap {
    'fui-toaster': Toaster;
  }
}
```

Every dashed entry whose type reaches `FASTElement` becomes a known tag,
pointing at the class it names — which is everything a template needs. The map
is read from the whole program, `.d.ts` files included, so a design system that
registers behind its own `defineToaster()` wrapper — tag built at runtime from
a prefix and a base name, no literal anywhere to find — still gets completion,
hover and go-to-definition, with nothing to configure. Three consequences of
the class usually being someone else's: no diagnostics are reported inside it;
a `.d.ts` keeps `position: ToastPosition` but not the `@attr` that made it an
attribute, so when a class carries no FAST decorators at all every public
member is offered as a property and the attribute-shaped ones as attributes
too; and `no-missing-import` is skipped, the augmentation being ambient.
An entry typed as plain `FASTElement`, naming no class of its own, registers
the tag with global attributes and nothing else.

Worth adding for your own components as well — it is what types
`document.createElement('tab-bar')` and `querySelector('tab-bar')` as your
class instead of `HTMLElement`. `no-missing-element-type-definition` points out
every registered component that lacks the entry; it is off by default, so turn
it on to find them:

```jsonc
"fastElementUltra.rules": { "no-missing-element-type-definition": "warning" }
```

**Neither.** For a tag defined somewhere nothing can read — a third-party
bundle, a runtime registration — `fastElementUltra.globalTags` accepts names
everywhere and checks nothing about them, and `fastElementUltra.customHtmlData`
describes tags in VS Code's custom-data format, attributes and slots included.

## Custom events

`no-unknown-event` only knows the events it can find, so `@my-event` on a
component that never declared one is reported as a typo. There are four ways
to say what an event is, and they combine — a detail type from one and prose
from another end up on the same event.

**1. Just emit it.** Every `$emit` in the file counts, wherever it is written,
and the *type of the receiver* decides whose event it is. The idiomatic FAST
spellings all work: `this.$emit(…)` in a method, `x.$emit(…)` in the host's
own template, and `c.parent.$emit(…)` from inside a `repeat` item template —
where `c.parent` is the only way back to the host:

```ts
const tabTemplate = html<Tab, TabBar>`
  <div @click="${(x, c) => c.parent.$emit('tab-select', { id: x.id })}"></div>
`;

const template = html<TabBar>`
  ${repeat((x) => x.tabs, tabTemplate)}
  <button @click="${(x) => x.$emit('tab-add')}"></button>
`;
```

Both events are found, `tab-select` carrying `{ id: number }` as its detail —
nothing to annotate. The reach stops at the file: a base class in another
file contributes its `this.$emit` calls, but an event that class raises from
*its* template needs one of the next two forms.

**2. Declare the map.** The explicit contract, and the only form the compiler
itself checks. `declare` emits no field, so this costs nothing at runtime:

```ts
@customElement({ name: 'tab-bar', template })
export class TabBar extends FASTElement {
  declare $events: {
    /** A tab was chosen. */
    'tab-select': { id: number };
    'tab-close': { id: number };
    'tab-add': void;
  };
}
```

Names and detail types are read through the type checker, so a named
interface (`declare $events: TabBarEvents`) works the same way, `void` means
"no detail", and go-to-definition on `@tab-select` lands on the line that
declares it. Reach for this when the event is part of the component's public
API, or when it is raised somewhere the emit scan cannot see — a mixin, a
helper, a controller.

**3. JSDoc `@fires`.** The documentation form, for a component whose events
are described in prose anyway. A `{Type}` in braces sets the detail type shown
on hover and completion (it is text, not a checked type):

```ts
/**
 * @fires {{ id: number }} tab-select - A tab was chosen.
 * @fires tab-add - The "+" button was pressed.
 */
@customElement({ name: 'tab-bar', template })
export class TabBar extends FASTElement {}
```

`@event` is accepted as a synonym, and `@attr`/`@prop` take a `{Type}` the
same way.

**4. Augment `HTMLElementEventMap`.** The standard-DOM route, and the one
worth taking when `addEventListener` should know about the event too:

```ts
declare global {
  interface HTMLElementEventMap {
    /** A tab was chosen. */
    'tab-select': CustomEvent<{ id: number }>;
  }
}
```

The augmentation is read from the whole program — your own files, and the
`.d.ts` of any package you installed — so a design system that ships one gets
its events recognised with nothing to configure. Note what the interface
actually says, though: it belongs to *every* `HTMLElement`, so the name is
accepted on any tag rather than tied to the component that raises it. Hover
and go-to-definition work from the entry, and a `CustomEvent<T>` is shown by
its detail `T`, the same as the other three forms. `lib.dom`'s own entries are
left alone — those are already known.

Prefer 2 or 3 when the event belongs to one component. Reach for this when the
element's consumers call `addEventListener` as often as they bind `@event`, or
when the library already ships the augmentation.

For an event that is genuinely global but typed nowhere — one a library
dispatches on anything — `fastElementUltra.globalEvents` accepts it everywhere
without any declaration.

## What it deliberately does not do

- **No lit support.** `.prop=` is lit; `:prop=` is FAST; three of seven
  binding forms mean different things in the two libraries, and guessing
  produces confident, wrong diagnostics. Install lit-plugin for lit code —
  both can run at once, each analyzing only its own templates.
- **No second compiler.** Templates are analyzed by TypeScript's own language
  server, against the program you already have — your `tsconfig.json`, your
  path aliases, your `node_modules` — in the same pass as TypeScript's own
  diagnostics. There is no second process to start, watch or wait for.
- **`html.partial(…)`** defeats analysis by construction; a template using it
  is marked as not analyzed rather than half-checked.

## Settings

All under `fastElementUltra.*`:

| Setting | Default | |
| --- | --- | --- |
| `disable` | `false` | Turn the analyzer off |
| `strict` | `false` | Every rule moves to its stricter default |
| `logging` | `off` | Plugin logging into the TS Server log |
| `dontShowSuggestions` | `false` | Strip “did you mean …?” tails |
| `htmlTemplateTags` / `cssTemplateTags` | `["html"]` / `["css"]` | Extra tag spellings (must still resolve to fast-element) |
| `maxProjectImportDepth` / `maxNodeModuleImportDepth` | `-1` / `1` | Reach of `no-missing-import` |
| `globalTags` / `globalAttributes` / `globalEvents` | `[]` | Assume these exist; check nothing about them |
| `customHtmlData` | — | VS Code custom-data files or objects; merged with `html.experimental.customData` |
| `rules.<id>` | `default` | Per-rule `off` / `warning` / `error` |

## Commands

- **FAST Element Ultra: Analyze Workspace FAST Templates** — run the full rule
  set over every FAST file TypeScript knows about and report into the Problems
  panel.
- **FAST Element Ultra: Clear Workspace Analysis Results.**

## Coming from lit-analyzer

The rule set began as lit-analyzer's and diverged wherever FAST's semantics
differ. If you already know lit-plugin's rule ids, this is what changed —
three rules no longer exist, three were renamed, and lit-analyzer's
deprecated aliases are gone. Settings live under `fastElementUltra.*`;
`lit-plugin.*` settings are not read.

| lit-analyzer | here |
| --- | --- |
| `no-nullable-attribute-binding` | **removed** — FAST removes an attribute on null/undefined; the rule was a false positive by construction |
| `no-legacy-attribute` | **removed** — Polymer's `foo$=` syntax |
| `no-invalid-boolean-binding` | **removed** — a dead id, implemented by nothing |
| `no-incompatible-property-type` | `no-incompatible-attr-config` — checks `@attr({ mode, converter })`, FAST's analogue of `@property({type})` |
| `no-property-visibility-mismatch` | `no-attr-visibility-mismatch` — only the “private @attr” direction survives; FAST has no `@internalProperty` |
| `no-invalid-directive-binding` | same id, FAST's directive table |
| `no-unknown-event` | now **on by default** (`warning`) — FAST's `$emit` gives a definite event list |
| `securitySystem`, `skip*` aliases, `externalHtmlTag*` | removed |

New rules with no lit-analyzer equivalent: `no-non-reactive-binding`,
`no-invalid-directive-target`, `no-slot-without-shadow-root`,
`no-duplicate-tag-name`, `no-untyped-template`, `no-implicit-prevent-default`.

## Supported TypeScript

Analysis runs against the TypeScript your workspace uses — including a
workspace-local version in `node_modules` — versions **5.5 up to (not
including) 8**. Outside that range the analyzer stands down: it logs why and
leaves TypeScript untouched.

The same goes for anything it cannot answer. If the analyzer fails, what you
are left with is TypeScript exactly as it behaves without this extension,
never a broken editor.

## Privacy and safety

Nothing reaches the network: no CDN, no remote data, no telemetry of any kind.
Your code is analyzed on your machine and stays there, and everything works the
same offline as online.

The extension writes nothing on its own. The only changes to your files are the
quick fixes and renames you accept, applied as ordinary editor edits you can
undo.
