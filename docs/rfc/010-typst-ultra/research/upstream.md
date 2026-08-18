# RFC 010 — What We Owe Upstream

[P4-16](../tasks/phase-4-polish.md), and the honest half of
[decision 0001](../decisions/0001-unmodified-upstream-typst.md). That record
says that when a feature needs something upstream does not expose, the options
are, in order:

1. rebuild it from what *is* exposed,
2. cut the feature and record the gap,
3. **propose the export upstream**.

Forking is not on the list. This page is the accounting: everywhere the
implementation hit the published API's edge, what was done about it, and what a
patch to typst would look like.

**The headline is that the edge was hit exactly once in the whole
implementation.** That is the strongest evidence available that 0001 was the
right call — the published surface really does carry a language server.

---

## 1. `NamedItem::name` and `::span` are `pub(crate)`

**Where.** `typst_ide::named_items` is how the server resolves a local binding:
it walks the scope chain and hands each candidate to a callback as a
`NamedItem`. Both references and rename need the callback to answer "is this the
name I am looking for, and where is it defined?"

**The problem.** `NamedItem`'s accessors are crate-private:

```rust
// typst-ide-0.15.1/src/matchers.rs:191, :208
impl<'a> NamedItem<'a> {
    pub(crate) fn name(&self) -> &'a EcoString { … }
    pub(crate) fn span(&self) -> Span { … }
}
```

So the one thing a caller of `named_items` inevitably wants to do with a
`NamedItem` is the thing it cannot do.

**What we did — option 1.** The *variants* are public, and they carry the same
information:

```rust
pub enum NamedItem<'a> {
    Var(ast::Ident<'a>),
    Fn(ast::Ident<'a>),
    Module(&'a EcoString, Span, Option<&'a Module>),
    Import(&'a EcoString, Span, Option<&'a Value>),
}
```

So four lines rebuild the accessor
([`references.rs`](../../../../crates/typst/typst-lsp-core/src/features/references.rs)):

```rust
fn describe<'a>(item: &NamedItem<'a>) -> (&'a EcoString, Span) {
    match item {
        NamedItem::Var(ident) | NamedItem::Fn(ident) => (ident.get(), ident.span()),
        NamedItem::Module(name, span, _) | NamedItem::Import(name, span, _) => (name, *span),
    }
}
```

**Cost of the workaround: four lines, and a maintenance hazard.** If upstream
adds a fifth variant, our match stops compiling — which is the good failure — but
if it changes what `name()` returns for an existing variant, our copy silently
disagrees. That is the reason to propose the export rather than keep the copy.

**The proposal.** Change two `pub(crate) fn` to `pub fn`. No new API surface, no
new types, no semantic change — the methods already exist and are already
correct; they are simply not reachable. A one-line diff each, and it removes the
only place in this project that reimplements upstream logic.

Worth pairing with a sentence in `named_items`' documentation, since the
signature `impl FnMut(NamedItem) -> Option<T>` currently invites a callback that
cannot inspect its argument.

---

## 2. Things that look like gaps and are not

Recorded because a future reader will otherwise re-investigate them.

| Suspected gap | Reality |
| --- | --- |
| Preview↔source sync needs a compiler patch | It does not. `jump_from_click` / `jump_from_cursor` are public and are exactly the right shape. Tinymist's `no-content-hint` fork solves a different problem, created by its own approach |
| `PagedDocument` is not re-exported by `typst` | It is in `typst-layout`, which is published. One extra dependency line, not a gap |
| HTML export is unreachable | Reachable, behind `Feature::Html` — experimental upstream, and gated the same way for `typst-cli`. We build a second `Library` with the feature on rather than enabling it globally, which is a supported use of a public builder |
| `ParamInfo` has no `docs` | `to_native()` gives `&NativeParamInfo`, which does. Closures genuinely have no parameter docs — that is not a missing export, it is a missing thing |
| `CastInfo` has no `Display` | It has `walk`, which is enough to render a signature. Arguably a nicety worth proposing, but nothing is blocked |

---

## 3. Features cut rather than worked around — option 2

From [proposal.md §10](../proposal.md#10-known-limitations-from-the-no-fork-constraint),
now confirmed by the implementation rather than predicted:

| Feature | Status |
| --- | --- |
| Signature help with **evaluated** argument values | Cut. Declared parameters, types, and defaults are implemented; live values need evaluation-time introspection, which is what tinymist's patches buy. Not proposed upstream — it is a large surface, and the 90% case works without it |
| Rename across packages | Cut, and refused explicitly with a message naming the package. Package sources are read-only; this is correct behaviour rather than a limitation |
| "Find all references" for standard-library items | Cut. Would need a whole-universe index, which is a project, not an export |

None of these turned out to be load-bearing during implementation, which is what
§10 predicted.

---

## 4. Status

**One proposal, ready to send.** The `NamedItem` accessors are a two-line change
with no design questions attached, which makes them a good first contribution.
The remaining items in §3 are deliberate cuts, not requests.
