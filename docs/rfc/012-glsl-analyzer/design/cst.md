# Design: the GLSL CST and its parser

Owner: Phase 3 (P3-01). Normative for the shape of `glsl_syntax::cst` and the contract
the parser holds to. The shape itself is settled by
[decision 0006](../decisions/0006-flat-cst-arrays.md); this document is the inventory and
the rules.

## 1. What the parser is handed, and what it must promise

Input is a [`Preprocessed`](../../../../crates/wgsl-shader/glsl-syntax/src/preprocessor/mod.rs):
the **live** token stream, macros already expanded, trivia already removed, every token
carrying a byte span into real source per [decision 0003](../decisions/0003-preprocessor-provenance.md).
Directives and inactive branches never reach the parser; they stay reachable through
`Preprocessed::directives` and `Preprocessed::inactive`.

Three promises, each with a test that enforces it:

| Promise | Enforced by |
| --- | --- |
| **Never panics.** Any byte sequence, any prefix of any file, any nesting depth. | `corpus_parse` under `catch_unwind`; the prefix-fuzz property in `recovery.rs` |
| **Loses no token.** Every `TokenId` in `0..tokens.len()` is a leaf exactly once, in order. Unparseable runs go into `Error` nodes. | `cst::every_token_is_a_leaf_exactly_once`, asserted again per corpus file |
| **Round-trips the source.** `tree.pieces(pp, source)` tiles `[0, source.len())` and concatenates back to the source byte for byte. | `cst::the_tree_round_trips_the_source`, asserted again per corpus file |

## 2. The tree

```rust
pub struct SyntaxTree {
    nodes: Vec<Node>,          // preorder; index 0 is the SourceFile root
    children: Vec<Child>,      // one contiguous slice per node
    pub diagnostics: Vec<SyntaxDiagnostic>,
}

pub struct Node {
    pub kind: NodeKind,
    pub span: ByteSpan,        // source bytes, joined over the written tokens below it
    pub parent: NodeId,        // the root is its own parent
    pub tokens: (TokenId, TokenId), // the half-open stream range this node covers
    pub end: NodeId,           // one past the last descendant — the subtree is [id, end)
    children: (u32, u32),      // range into SyntaxTree::children
}

pub enum Child { Node(NodeId), Token(TokenId) }
```

Consequences worth stating:

- **Node ids are preorder.** `parent < child` always; "is `b` inside `a`" is
  `a.0 <= b.0 && b.0 < tree.node(a).end`. That is the check the outline, folding and
  every "innermost node at offset" query wants.
- **Token ranges are contiguous.** The parser consumes strictly left to right, so a node
  covers `tokens.0 .. tokens.1` with no holes. `node_at(offset)` can therefore binary
  search when it needs to.
- **Spans are source spans, not stream spans.** A node whose tokens all came from a
  macro body has the invocation's span; a node that covers no written token at all gets
  an empty span pinned to the nearest written neighbour, so it is never bogus.

### 2.1 Losslessness without stored trivia

`pieces(pp, source)` walks the leaves in order and emits:

- `PieceKind::Token(id)` for each leaf whose `Origin::is_written()` holds and whose span
  starts at or after the last byte already covered;
- `PieceKind::Gap` for everything between them — leading whitespace, comments, every
  `#` directive line, every inactive branch, and the `NAME (` … `)` scaffolding of a
  macro invocation whose replacement the leaves stand for.

Tokens that are *not* written (`Origin::MacroBody`, and synthesised `#`/`##` results) are
skipped, as is a written token whose span was already covered — which happens when a
macro body uses the same parameter twice, or uses its parameters out of order. Coverage
stays exact in both cases because the surrounding gap already spans those bytes.

This is why nothing needs to be stored: the lexer already proved (Phase 2's corpus gate)
that its token stream tiles the source, and `pieces` is that same tiling seen through the
subset of it the tree claims.

## 3. Node kinds

The inventory for GLSL 4.60 §9, plus the ES and legacy variances. Grouped as the enum is.

**Structure** — `SourceFile`, `Error`.

`Error` is the recovery node. It appears wherever the parser gave up, holds the tokens it
skipped, and carries at least one diagnostic pointing at its start.

**Declarations** — `Declaration`, `Declarator`, `Initializer`, `InitializerList`,
`FunctionDecl`, `ParameterList`, `Parameter`, `StructSpec`, `FieldList`, `FieldDecl`,
`InterfaceBlock`, `PrecisionDecl`, `QualifierDecl`, `EmptyDecl`, `Name`, `Attribute`.

| Kind | Covers |
| --- | --- |
| `Declaration` | `layout(…) uniform mat4 a, b[2] = …;` — a qualifier list, a type, and one or more declarators |
| `QualifierDecl` | the type-less forms: `invariant gl_Position;`, `layout(local_size_x = 64) in;`, `precise;` |
| `PrecisionDecl` | `precision highp float;`, legal at file scope and inside a body |
| `InterfaceBlock` | `layout(std140) uniform Camera { … } camera[2];` — block name, `FieldList`, optional instance `Declarator` |
| `StructSpec` | `struct Name { … }` wherever a type specifier may appear, named or not |
| `FieldList` | the `{ … }` of a struct or a block; children are `FieldDecl`s |
| `FunctionDecl` | prototype and definition alike — the presence of a `CompoundStmt` child is the difference |
| `Name` | a *declared* identifier: a function's, a declarator's, a block's, a parameter's. Uses are `NameExpr` |
| `Attribute` | a `[[unroll]]` group. Not core GLSL — `GL_EXT_control_flow_attributes` — but it prefixes ordinary statements and declarations, so the grammar steps over it either way |

**Types and qualifiers** — `QualifierList`, `LayoutQualifier`, `LayoutItem`,
`SubroutineQualifier`, `TypeSpec`, `ArraySpec`.

`TypeSpec` wraps the type token (or a `StructSpec`) plus any C-style array suffix
(`float[4] x`); a GLSL-style suffix (`float x[4]`) belongs to the `Declarator`. One
`ArraySpec` per `[ … ]`, so `mat4 m[2][3]` has two.

**Statements** — `CompoundStmt`, `DeclStmt`, `ExprStmt`, `EmptyStmt`, `IfStmt`,
`ElseClause`, `SwitchStmt`, `CaseLabel`, `WhileStmt`, `DoWhileStmt`, `ForStmt`,
`Condition`, `ReturnStmt`, `BreakStmt`, `ContinueStmt`, `DiscardStmt`.

`Condition` exists because §9 lets `if`/`while`/`for` headers declare
(`while (bool ok = next())`); it holds either an expression or a one-declarator
declaration. `CaseLabel` covers `default:` too — the keyword is in its tokens.

**Expressions** — `CommaExpr`, `AssignExpr`, `CondExpr`, `BinaryExpr`, `UnaryExpr`,
`PostfixExpr`, `CallExpr`, `ArgumentList`, `IndexExpr`, `FieldExpr`, `ParenExpr`,
`NameExpr`, `LiteralExpr`.

The precedence table is §5.1, parsed by climbing:

```
comma  ,                                    (lowest)
assign = += -= *= /= %= <<= >>= &= ^= |=    right associative
cond   ?:                                   right associative
|| ^^ && | ^ & ; == != ; < > <= >= ; << >> ; + - ; * / %
unary  ++ -- + - ~ !                        prefix
postfix [] () . ++ --                       (highest)
```

**Call, constructor and index are not distinguished here.** `vec4(…)`, `f(…)` and
`S[2](…)` are all `CallExpr`; `a[i]` is always `IndexExpr` even when `a` names a type.
Syntax cannot tell a constructor from a call without a symbol table, and guessing would
put a lie in the tree. Phase 4 decides, with the `CallExpr`'s callee child in hand.

## 4. Ambiguity: declaration or expression?

At file scope every external declaration is a declaration, so the question only arises
inside a body (and in a `for` initialiser or a `Condition`). The signal is the one the
language guarantees: **two adjacent identifiers cannot occur in a GLSL expression.**

The lookahead is pure — it never consumes — and says "declaration" when, after skipping
any leading qualifiers:

1. the statement starts with `precision` or `struct`; or
2. at least one qualifier was skipped and the next token is `;`, or an identifier
   followed by `,` or `;` (the `invariant a, b;` form); or
3. the next token is an identifier, and after any `[ … ]` suffixes the token after it is
   an identifier.

Rule 3 is what makes `float x;`, `float[4] x;` and `mat4 m[2];` declarations while
`f(x);`, `a[0] = 1;` and `sample = 2.0;` stay expressions — including when the leading
word is a qualifier keyword the file is using as a name, which pre-1.30 sources do.

## 5. Recovery

Recovery's one move is **close what is open, put the tokens you skip in an `Error`, and
resume at a boundary**. What counts as a boundary depends on which question was asked,
and the distinction is the whole difference between a useful outline and a lost file:

| Situation | Move |
| --- | --- |
| **"You forgot a `;`."** `float b` ⏎ `float c;` | Skip only what cannot *begin* anything — stray punctuation, `$`, `@`. Stop at the first word. The next word is almost always the next declaration, and eating it would be the worst possible answer. |
| **"I cannot read this line at all."** `Texture2D <float4> t : register(t0);` | Skip to the `;` (consumed), the `}` (left for the enclosing block), or the first point that *looks like a declaration* — a word followed by a word, the one thing that cannot happen in an expression. Groups are stepped over whole. Reporting once beats reporting on every token. |

Both are bounded by end of stream. The rest:

- Unclosed `(`, `[` and `{` at end of stream are tolerated: the node closes where the
  tokens ran out and one `GLSL0105` **warning** names the opener. This is the state a
  file spends most of its keystrokes in, and it is not an error.
- A half-typed member line (`vec3 nor`, `float ;`) produces a `FieldDecl` with the parts
  that are there and a diagnostic for the part that is not — never a discarded block.
- Every loop that consumes has a no-progress guard that forces one token into an `Error`
  rather than spinning.
- Recursion is capped (`MAX_DEPTH = 64`, `GLSL0110`). An arena cannot overflow the stack,
  but a recursive-descent parser can, and "never panics" has to include "never aborts".

## 6. Diagnostics

`GLSL0100`–`GLSL0199` is the parser's range; the preprocessor keeps `GLSL0001`–`GLSL0099`
and Phase 4 takes `GLSL0200` onward.

| Code | Meaning |
| --- | --- |
| `GLSL0100` | expected a specific token (`;`, `)`, `:` …) |
| `GLSL0101` | expected an identifier |
| `GLSL0102` | expected a type |
| `GLSL0103` | expected an expression |
| `GLSL0104` | a token that cannot start what is expected here; skipped |
| `GLSL0105` | a group opened and never closed |
| `GLSL0106` | malformed `layout(…)` contents |
| `GLSL0107` | malformed array specifier |
| `GLSL0108` | a line at file scope that is not a declaration at all |
| `GLSL0109` | the file ended in the middle of a construct |
| `GLSL0110` | nesting limit reached; the rest of the construct is an `Error` |

Severity is `Error` throughout except `GLSL0105` and `GLSL0109`, which are **warnings**:
a file that simply stops is a file being typed, not a file with a bug, and the editor
should not paint it red between two keystrokes.

Every code has a seeded fixture that produces it (`recovery::every_parser_diagnostic_has_a_source_that_produces_it`).
A code nothing can emit is a code nobody can act on.

## 7. Outline extraction (P3-08)

`outline::outline(&SyntaxTree, &Preprocessed, source) -> Outline` walks the tree once and
produces the shapes `wgsl-lsp-core` already consumes — `Symbol { name, kind, name_span,
full_span, scope, detail, parent, children }` and `Reference { span, is_declaration,
is_member }` — with `SymbolKind` matching `wgsl_syntax::tree::SymbolKind` name for name.
The types live here; the wiring into the server is Phase 5 (P5-01), and the heuristic
walk in `wgsl-syntax` is not touched by this phase.

References are bytes the user wrote, from three sources: tokens written straight or
passed through a macro argument; **one** reference per macro *invocation*, spanning the
macro's name rather than the replacement it stood for; and the name in each `#define`,
which never reaches the token stream because the preprocessor consumed the directive.
That is what makes "find every use of `SCALE`" answerable at all — the old walk saw the
name because it never expanded anything.

Parity is a test, not a promise: for each of the extension's `examples/*.{vert,frag,comp}`
the fixture lists every `(kind, name, name_span)` the old walk produces — transcribed,
not imported, because a gate that called the old walk would go green the day the old walk
broke — and the new outline must contain all of them, with the same `detail` strings and
the same scope spans. It may produce more: a struct declared inside a body is a `Struct`
here and was a mislabelled `Local` there.
