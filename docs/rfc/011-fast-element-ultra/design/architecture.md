# Architecture

**Status**: living — implemented. Deviations from the original design are
marked inline; the two that matter are containment
([0011](../decisions/0011-containment-is-the-plugins-try-catch.md)) and the
expression metadata that travels with each virtual document (§3.1).

How the three layers fit together, what crosses each boundary, and in what coordinate space.

---

## 1. Process and module model

```
VS Code extension host                    tsserver (node)
┌────────────────────────────┐            ┌──────────────────────────────────────┐
│ dist/extension.js          │            │  ts.server.Project                   │
│                            │            │  ┌────────────────────────────────┐  │
│  activate()                │ configure  │  │ node_modules/                  │  │
│   ├ read fastElementUltra.*├───────────▶│  │  wx-fast-element-tsplugin/     │  │
│   ├ colour provider        │  Plugin()  │  │   index.js  ← create(info)     │  │
│   ├ analyze command        │            │  │   fast_analyzer_wasm.js        │  │
│   └ status item            │            │  │   fast_analyzer_wasm_bg.wasm   │  │
└────────────────────────────┘            │  └────────────────────────────────┘  │
        │                                 │            ▲          │              │
        │ vscode.typescript-language-     │  decorated │          │ engine calls │
        │ features → getAPI(0)            │  Language  │          ▼              │
        └─────────────────────────────────┤  Service   └──── WASM instance       │
                                          └──────────────────────────────────────┘
```

The extension host process contains **no analysis**. It reads settings, forwards them with
`api.configurePlugin(pluginId, config)`, draws colour swatches, and runs the workspace-analysis
command. Everything else happens inside tsserver.

Inside the plugin, one `WebAssembly.Module` is compiled at module scope and shared; instances are
per `ts.server.Project` so that two projects cannot see each other's registries. Whether that is the
right granularity is [open question 5](../decisions/README.md#open-questions).

### 1.1 Failure containment

A Rust panic inside tsserver is a much worse event than a Rust panic inside an extension: it takes
TypeScript's own features with it. Three layers guard against it — **corrected
by [0011](../decisions/0011-containment-is-the-plugins-try-catch.md)** after
testing the real artifact showed `catch_unwind` catches nothing under
`panic = "abort"` on wasm32:

1. **The plugin wraps every engine call in try/catch** (`SafeEngine.guard`) —
   a panic surfaces as a JS `RuntimeError` out of the glue, and this is the
   layer that catches it. Recoverable engine errors (bad JSON, unknown
   document) never throw at all: the Rust adapter converts them to a null
   result plus `lastError()`.
2. The plugin counts throws per engine. On the second, the engine is marked poisoned and dropped —
   a trapped instance's memory is not trustworthy.
3. `decorateLanguageService` falls through to the undecorated method when a decorated one
   throws — fast-analyzer's `wrapTryCatch` shape, kept. A poisoned
   engine therefore degrades to "TypeScript, unmodified", not to a broken editor.

This is [0006](../decisions/0006-wasm-inside-tsserver.md)'s side of the bargain: the WASM gets to
live in the hot process only because it cannot take the process down. The
whole chain is tested against the built artifact by deliberately panicking it
(`debugPanic`, `test/smoke.test.ts`).

## 2. Data flow for one diagnostic pass

```
getSemanticDiagnostics(fileName)
  │
  ├─ 1. TS: has this file changed? if not, return the cached result
  │
  ├─ 2. TS: walk the SourceFile
  │      ├ tagged templates with tag in htmlTemplateTags / cssTemplateTags
  │      ├ class declarations that register an element  → component facts
  │      └ imports                                      → dependency facts
  │
  ├─ 3. TS → Rust: upsert components and dependencies for this file
  │
  ├─ 4. TS → Rust: analyze(virtualDocument)
  │      Rust: parse → resolve tag/attr/event against the registry → run 19 rules
  │      Rust → TS: { diagnostics[], bindingFacts[] }
  │
  ├─ 5. TS: answer bindingFacts with ts-simple-type → more diagnostics
  │
  ├─ 6. TS: css`` documents → vscode-css-languageservice → no-invalid-css
  │
  └─ 7. TS: map every span to the SourceFile, merge, apply severities, return
```

Steps 3 and 4 are the only boundary crossings. Step 5 runs entirely in TypeScript and produces no
further call into Rust — that is the point of [0002](../decisions/0002-rust-engine-typescript-oracle.md).

Position-based features (completion, hover, definition) skip steps 5 and 6 and pass an offset into
step 4's tree instead.

## 3. The virtual document

Both sides address template text in one coordinate space. It is lit-analyzer's, and it is worth
keeping because it removes an entire class of bug: there is no mapping table, because offsets are
identical on both sides.

Each `${…}` is replaced by a run of `_` of **exactly the same length as the source text it replaces**,
carrying the expression's index in base 36:

```
source:      html`<a href=${url}>${text}</a>`
             ─────┬────────────────────────┬─
                  templateStart            templateEnd
substituted:      <a href=__0_>_____1_</a>
```

Properties that make this work:

- **Length preservation** → `sourceOffset = templateStart + documentOffset`, always.
- **The substitution is simultaneously a legal attribute name, a legal unquoted attribute value, and
  legal text content**, so a placeholder parses in any position FAST allows one. The corpus uses
  attribute-name position (`<div ${ref('tableEl')}>`), which is the case that rules out most simpler
  substitutions.
- **The index is recoverable from the text**, so the parser can attribute a placeholder to an
  expression without a side table — though we pass one anyway, because reading it back out of the
  text is a parse of a parse.

Nested templates (`when(…, html`…`)`) are separate virtual documents, each with its own
`templateStart` and its own source type (§5). They are not spliced into the parent's text.

### 3.1 What Rust receives

```ts
interface VirtualDocument {
  id: string;              // fileName + '#' + templateStart
  fileName: string;
  templateStart: number;   // source-file offset of the first character after the backtick
  kind: 'html' | 'css';
  text: string;            // substituted text, length-preserved, UTF-16 offsets
  placeholders: Array<{
    index: number;
    start: number;         // covers `${` through `}` within `text`
    end: number;
    expr?: ExprInfo;       // see below — compiler knowledge, computed once
  }>;
  sourceTypeId: number | null;   // interned id of TSource
  parentTypeId: number | null;
  sourceTypeName: string | null;
  sourceMembers: SourceMember[] | null;  // TSource's properties: names, types, decl spans
  componentTag: string | null;   // when registration and template share a file
  typeArgInsertOffset: number | null;    // where the no-untyped-template fix inserts <T>
}
```

**A design addition that implementation forced**: each placeholder carries an
`ExprInfo` — the expression's syntactic kind, whether its type has call
signatures, whether it is a constant, whether it is a FAST directive (and
which, with the string argument's span), whether it is `html.partial`. This
is compiler knowledge computed once at upsert, in the same crossing — it is
what lets `no-non-reactive-binding` and the directive rules run in Rust
without callbacks, and it preserves the one-crossing property. Likewise
`sourceMembers`: the source type's member list travels with the document, so
`ref('…')` checking and completion never ask the host anything.

Type identity crosses the boundary as an **interned id**, never as a structure. Rust compares ids
for equality and hands them back when it needs a question answered; it never inspects a type. The
intern table lives in the plugin. One offset subtlety the design missed:
JavaScript speaks UTF-16 and the parser speaks UTF-8 bytes, so the engine
converts at its edge (`documents.rs`, `OffsetMap`) and every offset in the
protocol is UTF-16 — the plugin never converts anything.

## 4. The boundary protocol

Three call directions, all synchronous, all JSON via serde.

### 4.1 TS → Rust: registry updates

```ts
engine.upsertFile({
  fileName,
  components: ComponentFact[],   // §5
  dependencies: string[],        // resolved module specifiers this file imports
  documents: VirtualDocument[],
});
engine.removeFile(fileName);
engine.setConfig(config);        // rules, globals, custom HTML data, template tags
```

`upsertFile` is idempotent and replaces everything the file previously contributed. Invalidation is
by file, not by symbol: simpler, and a file is the unit tsserver hands us anyway.

### 4.2 TS → Rust: queries

```ts
engine.analyze(documentId): { diagnostics: Diagnostic[]; facts: BindingFact[] }
engine.completions(documentId, offset): CompletionItem[]
engine.completionDetails(documentId, offset, name): CompletionDetail | null
engine.quickInfo(documentId, offset): QuickInfo | null
engine.definition(documentId, offset): DefinitionTarget | null
engine.renameInfo(documentId, offset): RenameInfo | null
engine.renameLocations(target): Location[]
engine.references(target): Location[]
engine.codeFixes(documentId, start, end): CodeFix[]
engine.closingTag(documentId, offset): string | null
engine.foldingRanges(documentId): Range[]
engine.colors(documentId): ColorInformation[]
```

Every offset in, and every span out, is in the virtual document's coordinate space, so the plugin's
only translation is `+ templateStart`.

Implemented queries beyond the list above: `memberReferences` /
`tagReferences` / `memberRenameLocations` / `tagRenameLocations` (the
declaration-side entry points), `documentInfoAt` (position routing for the
plugin's own features — `ref('…')` completion lives inside a placeholder, so
the plugin computes it from `sourceMembers`), `severities` (the resolved
table, so it exists exactly once, in Rust), `fileDiagnostics`
(registry-level rules: duplicate and invalid tag names, reported at
registrations), and `parseTree` (the differential harness's window). The
engine exposes no `colors` — colour decorators are wholly the extension
host's (§6).

### 4.3 Rust → TS: binding facts

A rule that needs a type comparison does not compute an answer; it emits the question. The rule
engine collects them and `analyze` returns them alongside the diagnostics it *could* decide.

```ts
interface BindingFact {
  kind: 'attribute' | 'boolean' | 'property' | 'event' | 'content' | 'element';
  documentId: string;
  span: { start: number; length: number };
  tagName: string;
  memberName: string | null;      // the attribute/property/event being bound
  targetRef: TargetRef | null;    // which declaration declares it — resolved by Rust from the registry
  expressionIndex: number | null; // which ${…}, or null for a literal assignment
  literal: string | boolean | null;
  ruleId: string;                 // which rule asked
}
```

`targetRef` is what makes this work without a second lookup: Rust already resolved
`?disabled` on `<csv-grid>` to a specific member of a specific component, and it hands back the
identity of the declaration node the plugin gave it. The plugin turns that into a `ts.Node`,
takes its type, takes the expression's type, and calls `isAssignableToType` — the same code path
fast-analyzer uses today, unchanged in substance.

**One crossing per file, both ways.** No callbacks from Rust into JS: a `wasm-bindgen` import would
work, but it would make the engine's control flow depend on the host's, and it would make
`cargo test` need a mock oracle. Instead the engine is a pure function of what it has been told, and
what it cannot decide it declares.

## 5. Component facts

What TypeScript extracts and hands to the registry — the full shape is in
[component-model.md](component-model.md):

```ts
interface ComponentFact {
  tagName: string;              // resolved through the checker, so a `const` works
  declarationId: number;        // interned; the class declaration, for go-to-definition
  sourceTypeId: number;         // the instance type, for matching against html<T>
  attributes: MemberFact[];
  properties: MemberFact[];
  events: EventFact[];
  slots: SlotFact[];
  cssParts: NamedFact[];
  cssProperties: NamedFact[];
  hasShadowRoot: boolean;       // shadowOptions: null → false
  origin: 'decorator' | 'define' | 'jsdoc' | 'customData' | 'globalTags';
}
```

`origin` is carried so that a diagnostic can say *why* it believes a tag exists, and so that a
lower-confidence source (`globalTags`) never overrides a declaration.

## 6. What the extension host still does

Small, and all of it needs `vscode`:

| Concern | Why it cannot live in the plugin |
| --- | --- |
| Settings → `configurePlugin` | Only the extension host can read workspace configuration |
| Colour decorators | `vscode.DocumentColorProvider` has no tsserver equivalent |
| Workspace analysis command | Needs a progress UI and a `DiagnosticCollection` |
| Status item | Reports engine state: on, disabled, poisoned, or TS version out of range |

The colour provider is the one place where the extension host needs template knowledge, and it will
not get it from the engine — the engine lives in the other process. It re-derives template ranges
with the same substitution rules, which is duplication, but of ~40 lines rather than of the analysis.
The alternative — routing colours through tsserver — has no protocol.
