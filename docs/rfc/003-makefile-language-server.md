# RFC 003: Makefile IntelliSense via a Rust Language Analyzer

**Status**: Draft
**Date**: 2026-04-22
**Extension name**: `wx-vsce-makefile` (single extension — grammar + analyzer together)
**Rust crate**: `crates/makefile-analyzer`

---

## 1. Motivation

`Makefile` is one of the oldest build languages in active use and is everywhere:
C/C++ projects, Go repos, documentation generators, embedded build systems,
CI glue. Despite that ubiquity, the VSCode ecosystem for editing Makefiles is
thin — the canonical grammar ships inside `vscode/extensions/make`, and the
de facto "outline" story is whatever the grammar's TextMate rules happen to
expose. There is no Rust-backed, zero-install analyzer comparable to what the
proto3 extension (RFC 002) already provides for Protocol Buffers.

Gaps in the current landscape:

- **No built-in outline.** VSCode's bundled Makefile grammar highlights but
  does not drive a `DocumentSymbolProvider`, so the breadcrumbs and
  `Ctrl+Shift+O` picker are empty.
- **Existing third-party extensions either shell out to `make` (brittle,
  needs a GNU Make on PATH, runs user code)** or rely on regex-based outline
  heuristics that break on realistic files with conditionals, `define`
  blocks, or `$(shell …)` expansions.
- **No shared analyzer between the editor and any future CLI checker.**
  Teams who want to validate Makefiles in CI end up writing ad-hoc shell
  scripts.

**Why Rust.** This repo already ships two Rust-powered analyzers
(`crates/wgsl-analyzer` for WGSL and `crates/proto3-analyzer` for proto3),
both compiled to WebAssembly and loaded into the VSCode extension host.
That pipeline is proven and cheap to extend:

- `wasm-pack build --target nodejs` → single artifact across all platforms,
  including `vscode.dev` with a future `--target web` pass.
- Hand-written lexer + parser live in one crate, share the `SpanTable` and
  `ByteSpan` types that RFC 002 established.
- The `wasm_api` surface is `JSON in, JSON out` — the TypeScript host code
  is thin and follows the same shape the protobuf extension already uses.

**Shape of `wx-vsce-makefile`.** One extension owns everything:

- Registers the `makefile` language id, the `Makefile` / `.mk` / `GNUmakefile`
  filename associations, the grammar, and bracket / comment configuration.
- Bundles the Rust analyzer via `wasm-pack` and registers a
  `DocumentSymbolProvider`, a `FoldingRangeProvider`, and a diagnostic
  collection for basic parse-level errors.
- Activates on `onLanguage:makefile`.
- Graceful degradation: if the WASM bundle is missing (fresh dev checkout
  before `pnpm run build:wasm`), the grammar still works and the analyzer
  providers log a warning and no-op.

## 2. Design Goals

1. **Syntax highlighting** for GNU Make: targets, variable assignments,
   recipe lines, function calls (`$(shell …)`, `$(patsubst …)`, …),
   automatic variables (`$@`, `$<`, `$^`, …), conditionals, `define` /
   `endef` blocks, includes, comments, and escaped line continuations.
2. **Document symbols / outline** — hierarchical tree: top-level rules
   (targets) with their recipes, variable assignments, `define` blocks,
   `include` directives, and conditional blocks. Breadcrumb-friendly.
3. **Folding ranges** — fold recipe bodies, `define ... endef` blocks, and
   `ifeq / ifdef ... endif` blocks.
4. **Basic diagnostics** — unterminated `define` (missing `endef`),
   unclosed conditional (missing `endif`), recipes outside any rule,
   recipe lines that use spaces instead of tabs.
5. **Reasonable performance** — a 5000-line Makefile parsed + document
   symbols in <15 ms on a modern laptop. The parser is single-pass and
   line-oriented so this is well inside budget.
6. **Zero external prerequisites** — works immediately after install, with
   no requirement to install `make`, `bmake`, or anything else.
7. **Deferred** for a later phase: go-to-definition on variable references,
   find-all-references, hover over built-in functions, rename. The initial
   feature set is deliberately narrow to match the user's stated "syntax
   highlight + outline in LSP server" goal; §9 calls out the phased plan.

## 3. High-Level Architecture

Identical topology to the protobuf extension (RFC 002 §3):

```
┌──────────────────────────────────────────────────────────────────┐
│ VSCode Extension Host (Node.js)                                   │
│                                                                    │
│   extension.ts                                                     │
│     ├─ activate(context)                                           │
│     ├─ register DocumentSymbol / FoldingRange / Diagnostics        │
│     │   providers (thin wrappers around the analyzer)              │
│     └─ WorkspaceWatcher ─ fs.watch on **/Makefile, **/*.mk         │
│                                                                    │
│   ┌────────────────────────────────────────────────────────────┐  │
│   │   AnalyzerBridge (TypeScript)                               │  │
│   │     - require('./wasm/makefile_analyzer.js')                │  │
│   │     - wraps a single long-lived Analyzer handle             │  │
│   │     - marshal/unmarshal (JSON over wasm-bindgen)            │  │
│   └──────────────────┬─────────────────────────────────────────┘  │
│                      │                                              │
│                      ▼                                              │
│   ┌────────────────────────────────────────────────────────────┐  │
│   │   makefile-analyzer (Rust, compiled to WASM)                │  │
│   │                                                              │  │
│   │   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │  │
│   │   │ Line-oriented│→ │ AST +        │→ │ Features:    │     │  │
│   │   │ lexer        │  │ source-map   │  │ symbols,     │     │  │
│   │   │              │  │ (per-file)   │  │ folding,     │     │  │
│   │   └──────────────┘  └──────────────┘  │ diagnostics  │     │  │
│   │                                        └──────────────┘     │  │
│   │                                                              │  │
│   │   Exposed API (JSON in, JSON out):                           │  │
│   │     - Analyzer::new()                                        │  │
│   │     - update_file(uri, source)                               │  │
│   │     - remove_file(uri)                                       │  │
│   │     - diagnostics(uri)                                       │  │
│   │     - document_symbols(uri)                                  │  │
│   │     - folding_ranges(uri)                                    │  │
│   └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

### 3.1 Transport

Same as RFC 002: **WASM loaded in-process** via `wasm-pack --target nodejs`.
One artifact, no per-platform builds, no native binary download.

## 4. Key Components

### 4.1 Rust crate (`crates/makefile-analyzer`)

#### 4.1.1 Lexer

Line-oriented tokenizer (`src/lexer.rs`). Makefile syntax is structured
around physical lines — a recipe line *must* start with TAB, assignments
and rules *must* start in column 0, line continuations (`\\\n`) join
logical lines — so a line-oriented tokenizer is both simpler and a better
fit than a stream of individual punctuation tokens.

The lexer emits a `Vec<LogicalLine>` where each entry carries:

- The joined text (after processing `\\\n` continuations).
- The kind (`Recipe`, `Directive`, `Assignment`, `Rule`, `Comment`, `Blank`),
  determined from the first non-whitespace character and a small set of
  keyword matches.
- A `ByteSpan` covering the full physical extent of the line in the
  original source.

The lexer never panics — malformed inputs become `LogicalLine::Unknown`
and surface as best-effort diagnostics in the parser.

#### 4.1.2 Parser

Recursive-descent over `LogicalLine`s, one function per directive kind
(`parse_rule`, `parse_assignment`, `parse_define`, `parse_conditional`,
`parse_include`). Nesting (conditional → rule → recipe, `define` blocks)
is tracked with a small explicit state stack; unclosed blocks produce a
diagnostic at EOF rather than silently swallowing the rest of the file.

Every AST node owns a `ByteSpan` on its declaration and an additional
`name_span` where relevant (e.g. the target name in a rule), so the
`DocumentSymbol` / `FoldingRange` providers can hand VSCode exact
selection ranges without re-scanning the source.

```rust
// crates/makefile-analyzer/src/ast.rs

pub struct File {
    pub items: Vec<Item>,
    pub span: ByteSpan,
}

pub enum Item {
    Rule(Rule),
    Assignment(Assignment),
    Define(Define),
    Include(Include),
    Conditional(Conditional),
    Directive(Directive),   // export/unexport/override/vpath/...
}

pub struct Rule {
    pub targets: Vec<Identifier>,
    pub prerequisites: Vec<Identifier>,
    pub is_double_colon: bool,
    pub is_pattern: bool,       // contains '%' in any target
    pub is_phony: bool,         // set by a prior .PHONY declaration
    pub recipe_lines: Vec<RecipeLine>,
    pub span: ByteSpan,
    pub name_span: ByteSpan,    // first target's span
}

pub struct Assignment {
    pub name: Identifier,
    pub op: AssignOp,
    pub value: String,
    pub span: ByteSpan,
}

pub enum AssignOp { Recursive, Simple, Immediate, Conditional, Append, Shell }

pub struct Define {
    pub name: Identifier,
    pub op: Option<AssignOp>,   // define VAR := ...
    pub body: String,
    pub span: ByteSpan,
    pub name_span: ByteSpan,
}

pub struct Include {
    pub paths: Vec<String>,
    pub optional: bool,  // `-include` / `sinclude`
    pub span: ByteSpan,
}

pub struct Conditional {
    pub kind: ConditionalKind,  // Ifeq/Ifneq/Ifdef/Ifndef
    pub condition: String,
    pub then_branch: Vec<Item>,
    pub else_branch: Vec<Item>,
    pub span: ByteSpan,
}
```

#### 4.1.3 Features

- **`features::document_symbols`** — traverses `File::items` and produces
  a hierarchical `DocumentSymbol` tree:
  - `Rule` → `SymbolKind::Method` (targets feel like named procedures).
    Pattern rules and `.PHONY` targets get a distinguishing `detail`
    string.
  - `Assignment` → `SymbolKind::Variable`, with `detail` set to the
    assignment operator (`=`, `:=`, etc.) and a truncated value preview.
  - `Define` → `SymbolKind::Constant`.
  - `Include` → `SymbolKind::File` (shown in outline but collapsed by
    default at the VSCode level).
  - `Conditional` → `SymbolKind::Namespace`, children are the then/else
    branches.

- **`features::folding`** — emits `FoldingRange`s for:
  - Rule bodies (from the rule declaration to the last recipe line).
  - `define ... endef` blocks.
  - `if... ... endif` blocks.

- **`diagnostics`** — Phase 1 catalog:

| Code      | Severity | Message template                                      |
|-----------|----------|-------------------------------------------------------|
| `MAKE001` | Error    | Recipe line uses spaces where a tab is required       |
| `MAKE002` | Error    | Recipe line appears outside of any target rule        |
| `MAKE003` | Error    | `define` block is not closed — expected `endef`       |
| `MAKE004` | Error    | Conditional is not closed — expected `endif`          |
| `MAKE005` | Error    | `endef` without matching `define`                     |
| `MAKE006` | Error    | `endif` / `else` without matching conditional         |
| `MAKE007` | Warning  | Assignment to an automatic variable (`$@`, `$<`, …)   |

### 4.2 Extension host (TypeScript)

Follows the protobuf extension's shape, scaled down to the feature set:

```
extensions/makefile/src/
├── extension.ts             # activate() wiring
├── analyzer.ts              # AnalyzerBridge around the WASM module
├── diagnostics.ts           # refreshDiagnostics pump
├── types.ts                 # shared types mirroring the Rust JSON
└── providers/
    ├── documentSymbol.ts
    └── foldingRange.ts
```

Activation is `onLanguage:makefile`. Providers register directly against
`vscode.languages.registerDocumentSymbolProvider` / `registerFoldingRangeProvider`
— no `vscode-languageclient` (the analyzer is in-process WASM).

## 5. File Structure

```
vscode-extensions/
├── crates/
│   ├── proto3-analyzer/
│   ├── wgsl-analyzer/
│   └── makefile-analyzer/          ← new
└── extensions/
    ├── protobuf/
    ├── wgsl-shader/
    └── makefile/                   ← new
```

### 5.1 Rust crate layout

```
crates/makefile-analyzer/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── wasm_api.rs              # #[wasm_bindgen] surface (Analyzer struct)
│   ├── vfs.rs                   # Workspace, FileUri
│   ├── parse.rs                 # Pipeline: lex → parse → ParsedFile
│   ├── lexer.rs                 # Line-oriented tokenizer
│   ├── parser.rs                # Recursive-descent over LogicalLine
│   ├── ast.rs                   # Typed AST nodes with ByteSpan
│   ├── spans.rs                 # ByteSpan, LineCol, SpanTable
│   ├── diagnostics.rs           # MakeDiagnostic, checks, codes
│   └── features/
│       ├── mod.rs
│       ├── document_symbols.rs
│       └── folding.rs
└── tests/
    └── snapshot.rs              # insta-powered AST and outline goldens
```

### 5.2 Extension layout

```
extensions/makefile/
├── package.json
├── tsconfig.json
├── tsdown.config.mts
├── language-configuration.json
├── syntaxes/
│   └── makefile.tmLanguage.json
├── wasm/                        # wasm-pack output (git-ignored)
│   ├── makefile_analyzer.js
│   ├── makefile_analyzer.d.ts
│   └── makefile_analyzer_bg.wasm
└── src/
    ├── extension.ts
    ├── analyzer.ts
    ├── diagnostics.ts
    ├── types.ts
    └── providers/
        ├── documentSymbol.ts
        └── foldingRange.ts
```

## 6. `package.json` Sketch

```jsonc
{
  "name": "wx-vsce-makefile",
  "displayName": "Makefile",
  "description": "Makefile editing — syntax highlighting and outline powered by a Rust analyzer compiled to WebAssembly.",
  "version": "0.1.0",
  "private": true,
  "publisher": "weixu",
  "engines": { "vscode": "^1.96.0" },
  "categories": ["Programming Languages"],
  "activationEvents": ["onLanguage:makefile"],
  "main": "./dist/extension.js",
  "contributes": {
    "languages": [
      {
        "id": "makefile",
        "aliases": ["Makefile", "makefile", "GNU Make"],
        "extensions": [".mk", ".mak", ".make"],
        "filenames": ["Makefile", "makefile", "GNUmakefile"],
        "configuration": "./language-configuration.json"
      }
    ],
    "grammars": [
      {
        "language": "makefile",
        "scopeName": "source.makefile",
        "path": "./syntaxes/makefile.tmLanguage.json"
      }
    ],
    "configuration": {
      "title": "Makefile",
      "properties": {
        "makefile.diagnostics.enabled": {
          "type": "boolean",
          "default": true,
          "description": "Emit diagnostics for unterminated define blocks, unclosed conditionals, and recipe lines that use spaces instead of tabs."
        }
      }
    }
  },
  "scripts": {
    "build:wasm": "cd ../../crates/makefile-analyzer && wasm-pack build --target nodejs --out-dir ../../extensions/makefile/wasm --out-name makefile_analyzer",
    "build": "tsdown",
    "clean": "rm -rf dist wasm",
    "typecheck": "tsc --noEmit",
    "watch": "tsdown --watch",
    "package": "pnpm run build:wasm && pnpm run build && vsce package --no-dependencies --allow-missing-repository",
    "vscode:prepublish": "tsdown --minify",
    "test": "vitest run",
    "test:watch": "vitest"
  }
}
```

## 7. `Cargo.toml` Sketch

```toml
[package]
name = "makefile-analyzer"
version = "0.1.0"
edition = "2021"
description = "Makefile (GNU Make) language analyzer — parser, outline, diagnostics — compilable to WASM for VSCode"
license = "MIT"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
wasm-bindgen = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
console_error_panic_hook = "0.1"

[package.metadata.wasm-pack.profile.release]
wasm-opt = false
```

The dependency footprint is smaller than proto3-analyzer's because we do
not need cross-file symbol resolution or a well-known-types bundle.

## 8. Grammar Coverage

The TextMate grammar lives at `extensions/makefile/syntaxes/makefile.tmLanguage.json`
and covers:

- Comments (`# ...`).
- Variable assignments, distinguishing the six operators (`=`, `:=`, `::=`,
  `?=`, `+=`, `!=`) so themes can colour them distinctly.
- Rule headers (`target: prereqs`), including double-colon rules and
  pattern rules (`%.o: %.c`).
- Recipe lines (tab-prefixed), with embedded shell substitution,
  automatic variables (`$@`, `$<`, `$^`, `$*`, `$?`, `$+`, `$|`), and
  escape sequences.
- Function calls (`$(shell …)`, `$(patsubst …)`, `$(foreach …)`, plus the
  full set of GNU Make built-ins).
- Directives: `include`, `-include`, `sinclude`, `define`, `endef`,
  `ifeq`, `ifneq`, `ifdef`, `ifndef`, `else`, `endif`, `export`,
  `unexport`, `override`, `vpath`, `private`, `undefine`.
- String literals and escaped line continuations.

The analyzer does not rely on the grammar — it lexes from raw source —
but the grammar is the first-line experience for users who have the
extension installed.

## 9. Implementation Phases

### Phase 1 — Syntax + Outline + Folding (this RFC's target)

- `crates/makefile-analyzer` skeleton: lexer, parser, AST with
  `ByteSpan`, `wasm_api`.
- Document symbols for rules, variables, defines, includes.
- Folding ranges for recipe bodies, `define`, and conditionals.
- Basic diagnostics: `MAKE001`–`MAKE006`.
- TextMate grammar covering the feature list in §8.
- `extensions/makefile` with `activate()`, `DocumentSymbolProvider`,
  `FoldingRangeProvider`, diagnostic collection.

**Acceptance:** Opening a Makefile highlights the syntax, populates the
outline view with targets and variables, folds recipe bodies, and
surfaces red squiggles for an unterminated `define`.

### Phase 2 — Definition + Hover (future)

- Cross-reference index over variable assignments.
- Go-to-definition on `$(VAR)` references.
- Hover on built-in functions showing the GNU Make docstring.
- Find-all-references.

### Phase 3 — Completion + Rename (future)

- Completion for GNU Make functions and automatic variables.
- Completion for variable names from assignments in scope.
- Safe rename of a variable across a workspace.

## 10. Prior Art

| Project | URL | What it does well | What's missing |
|---|---|---|---|
| Built-in VSCode `make` grammar | [github.com/microsoft/vscode/tree/main/extensions/make](https://github.com/microsoft/vscode/tree/main/extensions/make) | Stable syntax highlighting | No outline, no diagnostics |
| `ms-vscode.makefile-tools` | [marketplace.visualstudio.com/items?itemName=ms-vscode.makefile-tools](https://marketplace.visualstudio.com/items?itemName=ms-vscode.makefile-tools) | Build / launch / IntelliSense for C/C++ projects | Focuses on *running* make, not on editing the Makefile itself |
| `vscode-makefile-lang` | various community forks | Extended highlighting | Regex-only, no LSP |

## 11. Open Questions

1. **BSD make vs GNU make.** The analyzer targets GNU Make's grammar
   (which is the dominant dialect). BSD `bmake` has a divergent
   conditional syntax (`.if` / `.endif`) that is *not* covered. Do we
   add an opt-in `makefile.dialect` setting later? Probably yes in
   Phase 3.
2. **Pattern rule vs static pattern rule disambiguation.** Both forms are
   recognised, but right now we don't distinguish them in the outline.
   Open question whether to surface the distinction.
3. **Multi-line variable values** (continued with `\\` at EOL). The
   parser joins them into a single logical line, but the outline's
   "value preview" currently truncates to the first physical line.
   Acceptable trade-off for v1.
