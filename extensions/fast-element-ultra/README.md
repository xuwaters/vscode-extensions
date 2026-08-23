# FAST Element Ultra

FAST Element template analysis inside TypeScript. The `html``` and `css```
tagged templates of [`@microsoft/fast-element`](https://github.com/microsoft/fast)
are strings as far as TypeScript is concerned; this extension makes them a
language surface — diagnostics, completion, hover, go-to-definition,
find-all-references, rename, quick fixes, closing tags and folding — with the
template engine written in Rust and compiled to WebAssembly, running inside
tsserver as a TypeScript server plugin, and the type checker answering the
questions only it can (RFC 011).

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
- **No second compiler.** Templates are analyzed inside tsserver, against the
  program you already have, in the same pass as TypeScript's own diagnostics.
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
  set over every FAST file tsserver knows and report into the Problems panel.
- **FAST Element Ultra: Clear Workspace Analysis Results.**

## Moving from `fast-plugin` (the lit-analyzer fork)

Settings live under `fastElementUltra.*` instead of `fast-plugin.*` and are
not migrated — three rules no longer exist, three were renamed, and the
deprecated aliases are gone:

| fast-plugin | here |
| --- | --- |
| `no-nullable-attribute-binding` | **removed** — FAST removes an attribute on null/undefined; the rule was a false positive by construction |
| `no-legacy-attribute` | **removed** — Polymer's `foo$=` syntax |
| `no-invalid-boolean-binding` | **removed** — a dead id, implemented by nothing |
| `no-incompatible-property-type` | `no-incompatible-attr-config` — checks `@attr({ mode, converter })`, FAST's analogue of `@property({type})` |
| `no-property-visibility-mismatch` | `no-attr-visibility-mismatch` — only the “private @attr” direction survives; FAST has no `@internalProperty` |
| `no-invalid-directive-binding` | same id, FAST's directive table |
| `no-unknown-event` | now **on by default** (`warning`) — FAST's `$emit` gives a definite event list |
| `securitySystem`, `skip*` aliases, `externalHtmlTag*` | removed |

New rules with no fast-plugin equivalent: `no-non-reactive-binding`,
`no-invalid-directive-target`, `no-slot-without-shadow-root`,
`no-duplicate-tag-name`, `no-untyped-template`, `no-implicit-prevent-default`.

## Supported TypeScript

The plugin runs against the workspace's TypeScript
(`enableForWorkspaceTypeScriptVersions`), versions **5.5 up to (not
including) 8**. Outside that range it logs why and leaves TypeScript
untouched.

## How it is put together

Four Rust crates (`crates/fast/`) compile to one 570 KB WASM artifact: a
tolerant template parser whose tree is differentially tested against parse5,
generated HTML/SVG/MathML data tables, the rule engine and every position
query, and a thin wasm-bindgen adapter. The TypeScript server plugin
(`node_modules/wx-fast-element-tsplugin/`, inside this extension) feeds it
component facts and virtual documents, and answers its binding facts with the
type checker. A Rust panic surfaces as a caught error, is counted, and on
repetition the engine is poisoned — the end state of any engine failure is
TypeScript, unmodified, never a broken editor.
