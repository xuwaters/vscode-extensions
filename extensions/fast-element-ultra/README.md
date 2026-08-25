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
form this repo's own extensions use — `@attr` in every mode, on properties
and accessors, `@observable`, `@volatile`, `attributes: [...]` in the
definition, members inherited through the class chain, `this.$emit(...)`
events with their detail types, and JSDoc `@slot`/`@fires`/`@csspart`/`@cssprop`.
A library that registers its elements behind its own `define*` wrapper — where
the tag is built at runtime from a prefix and a base name, and there is no
literal to find — is picked up from its `HTMLElementTagNameMap` augmentation
instead, declaration files included, so an installed design system gets
completion, hover and go-to-definition too.

**IDE features** the compiler cannot provide: completion *inside*
`ref('…')`/`slotted('…')`/`children('…')` string arguments with the source
type's members; renaming a member updates its `:prop` bindings and its
`ref('…')` strings across files, in both directions; go-to-definition from a
template attribute to the `@attr` member; find-all-references from a tag to
every template that uses it.

**Suppression**: a `@ts-ignore` (or `@fast-ignore`) comment on the previous
line silences a template diagnostic.

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
