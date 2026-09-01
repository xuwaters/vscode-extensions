# 0006 — The CST is flat arrays over the expanded token stream

**Status:** Accepted (P3-01). Closes open question **q2**.

## Decision

`glsl-syntax`'s `SyntaxTree` is a **flat preorder arena of nodes plus one contiguous
child array**, not a rowan-style green/red pair:

```rust
pub struct SyntaxTree {
    nodes: Vec<Node>,      // preorder; node 0 is the SourceFile
    children: Vec<Child>,  // each node's children occupy one contiguous slice
    pub diagnostics: Vec<SyntaxDiagnostic>,
}
pub enum Child { Node(NodeId), Token(TokenId) }
```

Four rules come with it:

1. **The tree does not own its tokens.** A leaf is a `TokenId` into
   `Preprocessed::tokens`; every accessor that needs spelling takes `&Preprocessed`.
   That matches the crate contract in [architecture.md](../design/architecture.md)
   (`glsl-analysis` already receives both) and keeps a reparse from cloning ~8k
   `String`s per keystroke.
2. **Every token is a leaf exactly once, in stream order.** Whatever the parser cannot
   understand goes into an `Error` node rather than being dropped. This is what makes
   "lossless" a testable property instead of a claim.
3. **Losslessness against the *source* is a coverage function, not stored trivia.**
   `SyntaxTree::pieces` walks the leaves that [decision 0003](0003-preprocessor-provenance.md)
   marks as written (`Origin::Source`/`Origin::MacroArg`) and fills every byte between
   them with a `Gap` — directives, comments, whitespace, inactive regions and the
   punctuation of a macro invocation all land there. Concatenating the pieces returns
   the source byte for byte.
4. **Node ids are preorder**, so a subtree is the id range `[id, node.end)` and a
   parent id is always smaller than its children's.

## Why

- **House style.** `wgsl-syntax`, `fast-template-syntax` and the other analyzers in this
  repo all use flat arenas with index links; a green/red pair would be the only tree of
  its kind here, and the features that consume it (`wgsl-lsp-core`) are written against
  flat indices already.
- **wasm cost.** rowan-style trees pay for `Rc`/interning, node-cursor allocation on
  every traversal, and a second red layer per query. Two `Vec`s and a `u32` cursor cost
  none of that, and RFC 012 §8 budgets the whole crate family at +900 KB.
- **No incremental reuse to buy.** The performance posture is whole-file reparse per
  edit; green-tree subtree sharing pays off only for incremental relexing, which this
  RFC explicitly does not do.
- **Preorder ids are worth more than pointer identity here.** "Every node inside this
  function" is a range check, which is what the outline, folding and semantic-token
  walks all want.

The cost we accept: node kinds are a closed enum rather than an open language-agnostic
`SyntaxKind`, and a tree cannot be mutated after it is built. Neither matters — this
crate parses one language and rebuilds from scratch on every edit.

## Consequences

- The parser is written as a **flat event list** (`Open` / `Token` / `Close`) that a
  single build pass turns into the arena. Recovery is therefore "close the open nodes
  and start an `Error`", with no tree surgery.
- `SyntaxTree` accessors take `&Preprocessed`. A caller that has a tree without its
  `Preprocessed` has a bug, and the borrow checker says so.
- The round-trip gate (P3-07) is two assertions: `pieces` concatenates to the source,
  and the leaf `TokenId`s are exactly `0..tokens.len()` in order.
- Parser recursion is depth-capped (`GLSL0110`); an arena cannot overflow the stack but
  a recursive-descent parser can, and "never panics" includes never aborting.
