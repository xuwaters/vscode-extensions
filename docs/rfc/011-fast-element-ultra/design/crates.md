# Rust crates

**Status**: design, nothing built.

Four crates under `crates/fast/`, following the grouping-directory pattern `crates/typst/`
established. The root `Cargo.toml` needs explicit members for them, since `crates/*` cannot express a
nested group — the same workaround already documented there for typst.

```toml
members = [
  "crates/*",
  "crates/fast/fast-template-syntax",
  "crates/fast/fast-html-data",
  "crates/fast/fast-analyzer-core",
  "crates/fast/fast-analyzer-wasm",
  # …existing typst entries…
]
exclude = ["crates/typst", "crates/fast"]
```

---

## The dependency rule

```
fast-analyzer-wasm  ──▶  fast-analyzer-core  ──▶  fast-template-syntax
                                            └──▶  fast-html-data
```

Nothing below `fast-analyzer-wasm` may depend on `wasm-bindgen`, `js-sys` or `web-sys`. That is what
makes `cargo test` run the real engine on the host rather than a shim, and it is the repo's
established engine/adapter split.

`fast-analyzer-core` must also be a **pure function of what it is told**. It does not read files, does
not know about TypeScript, and cannot call back into the host — a question it cannot answer becomes
a binding fact in its output ([architecture.md §4.3](architecture.md#43-rust--ts-binding-facts)).
This is the property that makes the engine testable without mocking a type checker.

---

## `fast-template-syntax`

A tolerant tokenizer and tree for interpolated HTML.

```rust
pub fn parse(text: &str, placeholders: &[Placeholder]) -> Document;

pub struct Element {
    pub name: Span,                    // the tag name, for rename and definition
    pub open: Span,                    // <foo …>
    pub close: Option<Span>,           // </foo>, None when unclosed
    pub self_closing: bool,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
    pub kind: ElementKind,             // Html | Svg | MathML | Custom
}

pub struct Attribute {
    pub modifier: Option<Modifier>,    // : ? @   — the character, with its own span
    pub name: Span,                    // without the modifier
    pub value: Option<AttributeValue>, // Literal | Placeholder | Mixed(Vec<Part>)
    pub quote: Option<Quote>,
}
```

Requirements that rule out simply binding an existing parser:

- **Spans on everything**, including the modifier character separately from the name, so
  `?disabled` can be diagnosed as "the `?` is wrong" rather than "the attribute is wrong".
- **Unclosed tags stay unclosed.** An HTML5 parser auto-closes them; `no-unclosed-tag` needs to see
  what was written ([rules.md](rules.md)).
- **Placeholders are first-class**, in attribute-name, attribute-value and content position, and the
  tree records which expression index each came from.
- **Raw-text elements** (`<style>`, `<script>`, `<textarea>`, `<title>`) and **foreign content**
  (`<svg>`, `<math>`, where self-closing is legal) behave correctly. The corpus is full of inline
  SVG.
- **No allocation per node beyond the tree itself**, and no `String` copies — spans index into the
  caller's `&str`.

The choice to write this rather than bind `html5ever` or `swc_html_parser` is
[0003](../decisions/0003-own-template-parser.md), and it is gated on a differential test against
parse5 ([research/spikes.md, gate 2](../research/spikes.md#gate-2--does-our-parser-match-parse5)).

The crate takes plain text and a placeholder table, and knows nothing about tagged templates. That
is deliberate: it is what would let declarative `.html` templates reuse it
([research/fast-element.md §11](../research/fast-element.md#11-declarative-templates-out-of-scope)).

**No dependencies.**

## `fast-html-data`

Static knowledge about HTML itself.

```rust
pub fn element(name: &str) -> Option<&'static ElementData>;
pub fn global_attribute(name: &str) -> Option<&'static AttributeData>;
pub fn event(name: &str) -> Option<&'static EventData>;
pub fn is_void(name: &str) -> bool;
pub fn parse_custom_data(json: &str) -> Result<CustomData, Error>;
```

Built-in element, attribute and event tables, generated at build time from
`vscode-html-languageservice`'s and `@vscode/web-custom-data`'s JSON into a `phf` map, plus a loader
for user-supplied VS Code custom-data files.

Generating rather than vendoring the JSON means the data is a static table in the binary with no
parse at startup, and dropping two runtime JavaScript dependencies. The generator lives in the crate
and the generated file is committed, so a build does not need the npm packages present — same
arrangement as typst-ultra's embedded-language grammar.

**Dependencies**: `phf`. The generator additionally uses `serde_json`.

## `fast-analyzer-core`

The engine. Everything that is neither parsing nor static data.

| Module | Responsibility |
| --- | --- |
| `registry` | Components by tag name, per contributing file, with the merge order from [component-model.md §4](component-model.md#4-what-rust-does-with-the-facts) |
| `documents` | Parsed virtual documents, keyed by id, invalidated per file |
| `resolve` | Position → node → what it means: tag, attribute, modifier, value, placeholder, directive |
| `rules` | The 19 rules decided in Rust, plus fact emission for the 7 that are not |
| `ide` | Completion, quick info, definition, references, rename, code fixes, closing tag, folding, colours |
| `config` | Severities, `strict`, globals, template tags |
| `suggest` | Nearest-name suggestions |

The rule engine is a visitor over the tree with a shared context, as lit-analyzer's is —
that design is good and the port should keep its shape, because it makes each rule a small
independent thing.

**Dependencies**: `fast-template-syntax`, `fast-html-data`, `serde`, `strsim`.

`strsim` replaces `didyoumean2`. Match its behaviour on the fixtures rather than assuming — a
"did you mean" that suggests the wrong thing is worse than none.

## `fast-analyzer-wasm`

The adapter. As thin as it can be.

```rust
#[wasm_bindgen]
pub struct Engine { inner: fast_analyzer_core::Engine }

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)] pub fn new() -> Engine;
    pub fn upsert_file(&mut self, json: &str) -> Option<String>;
    pub fn remove_file(&mut self, file_name: &str);
    pub fn set_config(&mut self, json: &str) -> Option<String>;
    pub fn analyze(&mut self, document_id: &str) -> Option<String>;
    pub fn query(&mut self, json: &str) -> Option<String>;   // the position features
}
```

Every method body is wrapped in `catch_unwind`; a panic returns `None` with the message routed to the
plugin's log rather than crossing as an exception. `Option<String>` rather than `Result` because a
JS exception out of a `wasm_bindgen` call inside tsserver is exactly the event
[architecture.md §1.1](architecture.md#11-failure-containment) exists to prevent.

Payloads are JSON strings rather than `JsValue` graphs: one serialisation each way, no per-field
boundary crossing, and the same bytes are what `cargo test` feeds the core. If measurement says the
serialisation dominates ([research/spikes.md, budget 3](../research/spikes.md#budget-3--type-oracle-round-trip)),
the fallback is `serde-wasm-bindgen`, which is a change inside this crate only.

**Dependencies**: `fast-analyzer-core`, `wasm-bindgen`, `serde_json`, `console_error_panic_hook`.

Built with `wasm-pack build --target nodejs`, output into
`extensions/fast-element-ultra/wasm/`, and from there copied into the plugin directory that ships
inside the VSIX ([research/spikes.md, gate 1](../research/spikes.md#gate-1--can-the-plugin-be-packaged-at-all)).

---

## Testing

| Level | Where | What |
| --- | --- | --- |
| Unit | Each crate's `#[cfg(test)]` | Parser cases, rule cases, registry merge order |
| Differential | `fast-template-syntax/tests/` | Our tree against parse5's, over the corpus and adversarial inputs |
| Engine | `fast-analyzer-core/tests/` | Whole diagnostic passes over recorded `upsertFile` + `analyze` payloads |
| Integration | `extensions/fast-element-ultra` vitest | The real `.wasm`, driven by the plugin's own code |
| Corpus | CI | [research/corpus.md §4](../research/corpus.md#4-how-the-corpus-is-used) |

The engine-level tests are recorded payloads, not live TypeScript, which is the payoff for the
"pure function of what it is told" rule: a regression can be captured as a JSON file and replayed
without a compiler.

**Note**: `cargo fmt` is not run in this repo — the Rust crates are hand-formatted and there is no
`rustfmt.toml`.
