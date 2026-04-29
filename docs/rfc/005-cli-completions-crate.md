# RFC 005: `cli-completions` — Embedded Command-Line Completions Crate

**Status**: Accepted
**Date**: 2026-04-29
**Rust crates**:
  - `crates/cli-completions` (MIT — runtime + format)
  - `crates/cli-completions-data-fish` (GPL-2-or-later — fish snapshot + extractor)
**Initial consumer**: `crates/makefile-analyzer` (via `MakefileAnalyzer::complete(...)`)
**Data source**: vendored snapshot of [fish-shell](https://github.com/fish-shell/fish-shell) `share/completions/*.fish` (currently in `temp/fish-shell/share/completions/`, 1055 files, 6.8 MB, ~32 k `complete` directives)

---

## 1. Motivation

The Makefile extension (RFC 003) parses recipe lines but provides no
assistance with the *content* of those recipes — the shell commands the
recipe runs. A user typing:

```make
build:
\tcurl --
```

…sees nothing in the IntelliSense popup. We want the editor to show
`--abstract-unix-socket`, `--anyauth`, `--cacert`, … with their human
descriptions, and analogous behaviour for `git`, `docker`, `tar`, `ssh`,
and the long tail.

A naïve solution would hand-author flag tables for a few dozen commands.
That does not scale and rots fast. Three real-world data sources are
machine-readable enough to lift wholesale:

- **fish-shell completions** — `complete -c CMD …` lines, ~1000 commands,
  shipped with fish since 2005, kept current by an active community.
- **Fig / withfig autocomplete** — TypeScript modules, ~600 commands,
  MIT-licensed, but require evaluating JS at build time and dropping the
  generator callbacks.
- **carapace-bin** — Go specs, ~1000 commands, MIT-licensed, but the
  authoritative form is a running Go binary not a static spec file.

Fish wins on **breadth, freshness, and parsing simplicity** — the format
is line-oriented, every entry begins with the keyword `complete`, and
the field set is small (`-c`, `-s`, `-l`, `-d`, `-n`, `-a`, `-x`, `-f`,
`-r`, `-F`). It loses on **license**: fish is GPL-2 (see §11), and that
constraint shapes how the data may be redistributed.

This RFC proposes an independent crate, `cli-completions`, that:

1. **At build time**, ingests the vendored fish snapshot, extracts the
   static subset of every `complete` directive, and emits a single
   compact binary blob plus a generated Rust lookup table.
2. **At runtime**, exposes a small `query(command_path, prefix)` API
   that returns matching options with descriptions, with no I/O, no
   subprocess, no external dependencies, and no runtime config.
3. **Is consumed by** `makefile-analyzer` (and any future analyzer) as
   a plain Cargo dependency. The makefile crate adds one wasm-exposed
   method, `complete(uri, line, col) -> JSON`, that the VSCode
   extension wires into a `CompletionItemProvider`.

The result: zero-config IntelliSense for the contents of recipes, with
~1000 commands covered the moment the user installs the extension.

## 2. Goals and Non-Goals

### Goals

1. **Zero runtime config.** Install the extension → completions work.
   No `npm install` of spec files, no `pnpm sync`, no settings.
2. **Zero runtime dependencies.** The crate compiles with only
   `wasm-bindgen`/`serde` (which `makefile-analyzer` already pulls);
   the data is embedded via `include_bytes!`. No FFI, no I/O, no
   network.
3. **Broad coverage from day 1.** Whatever fish ships, we ship —
   targeting ≥95 % of fish files producing at least one usable entry.
4. **Deterministic builds.** A given fish snapshot produces a
   byte-identical generated artefact across machines.
5. **Reasonable size.** The packed blob fits in well under 5 MB after
   string interning; ideally <2 MB. (Raw input is 6.8 MB of `.fish`.)
6. **Reasonable speed.** A single completion query — `git remote ad`
   → all matching options — completes in <1 ms on a 5-year-old laptop.
7. **Reusable.** The crate has no makefile-specific code. A future
   shell-script extension, devcontainer file extension, etc., can
   depend on the same crate.
8. **Static-only.** Only the parts of fish completions that are pure
   data are extracted; dynamic generators (`-a "(__fish_git_branches)"`)
   are dropped. We never run shell subprocesses.

### Non-Goals

- **Dynamic value completion.** No live git branches, hostnames,
  filenames-in-current-dir, etc. Users get flag/option/subcommand
  names and descriptions, full stop. This boundary is what lets us
  promise "no I/O, no subprocess".
- **Bash/zsh completion ingestion.** Their formats are imperative
  shell code, not data. Out of scope.
- **Authoring new completions.** We are a redistributor. Upstreaming
  fixes to fish is the right path for missing entries.
- **Fuzzy matching, ranking, snippets.** The crate returns raw
  matches; the consuming editor's `CompletionItemProvider` decides
  presentation. (VSCode already does prefix and substring matching
  on `CompletionItem.label`.)
- **Per-project overrides.** Could be added later via a side-channel
  table in the consumer; not in v1.

## 3. High-Level Architecture

```
                Build-time (cargo build / xtask)
┌───────────────────────────────────────────────────────────────┐
│  temp/fish-shell/share/completions/*.fish   (vendored input)   │
│                          │                                     │
│                          ▼                                     │
│  crates/cli-completions/build/extract.rs                       │
│    1. Parse each .fish file                                    │
│    2. Keep static `complete` directives                        │
│    3. Resolve subcommand context from `-n` predicates          │
│    4. Intern strings (cmd names, descriptions)                 │
│    5. Emit data/completions.bin   (packed table)               │
│    6. Emit src/generated_index.rs (command-name → offset map)  │
└───────────────────────────────────────────────────────────────┘
                                │
                                ▼
                Compile-time (rustc)
┌───────────────────────────────────────────────────────────────┐
│  crates/cli-completions/src/lib.rs                             │
│    static DATA: &[u8] = include_bytes!("../data/completions.bin"); │
│    include!("generated_index.rs"); // fn lookup_command(...)   │
│                                                                │
│    pub fn query(path: &[&str], prefix: &str) -> Vec<Match>     │
└───────────────────────────────────────────────────────────────┘
                                │
                                ▼
                Runtime (wasm in VSCode)
┌───────────────────────────────────────────────────────────────┐
│  crates/makefile-analyzer/src/features/completion.rs           │
│    - Detect "we are inside a recipe line"                      │
│    - Tokenise the recipe shell text                            │
│    - Identify command + subcommand path + active partial token │
│    - Call cli_completions::query(...)                          │
│    - Format results as JSON for the bridge                     │
│                                                                │
│  crates/makefile-analyzer/src/wasm_api.rs                      │
│    + pub fn complete(uri, line, col) -> JsValue                │
└───────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌───────────────────────────────────────────────────────────────┐
│  extensions/makefile/src/providers/completion.ts               │
│    CompletionItemProvider { triggerCharacters: ['-', ' '] }    │
└───────────────────────────────────────────────────────────────┘
```

The crate is **a build script plus a runtime lookup**. There is no
network access, no filesystem access, no subprocess — at any phase
once the fish snapshot is committed.

## 4. Crate Layout

The work is split across **two crates** (rationale: §11):

```
crates/cli-completions/                       # MIT
├── Cargo.toml                                # license = "MIT"
├── LICENSE
├── README.md
├── src/
│   ├── lib.rs                                # public API surface
│   ├── format.rs                             # blob format constants, version, magic
│   ├── reader.rs                             # zero-copy decode of a blob
│   ├── matcher.rs                            # prefix matching over the entry list
│   ├── types.rs                              # CompletionEntry, MatchKind, EntryFlags, …
│   └── installer.rs                          # register a blob (data crate calls this)
└── tests/
    ├── reader_tests.rs                       # decode synthetic blobs
    └── matcher_tests.rs                      # prefix / subcommand-path matching

crates/cli-completions-data-fish/             # GPL-2-or-later
├── Cargo.toml                                # license = "GPL-2.0-or-later"
├── LICENSE                                   # full GPL-2 text
├── README.md                                 # snapshot info + attribution
├── build.rs                                  # invokes build::run()
├── build/
│   ├── mod.rs
│   ├── fish_lexer.rs                         # tokenise a .fish line
│   ├── fish_parser.rs                        # extract static `complete` directives
│   ├── predicate.rs                          # interpret `-n '__fish_…'` predicates
│   ├── encode.rs                             # pack interned table → bytes
│   └── snapshot.rs                           # walk vendored .fish files
├── data/
│   ├── fish-snapshot/                        # vendored .fish files (committed)
│   ├── fish-snapshot.toml                    # { upstream_commit, fetched_at, sha256 }
│   └── completions.bin                       # generated; committed for reproducibility
├── src/
│   └── lib.rs                                # `pub fn embedded() -> &'static CompletionsDb`
└── tests/
    ├── fixtures/                             # tiny synthetic .fish files
    ├── parser_tests.rs                       # parse .fish → directive list
    ├── encode_tests.rs                       # encode/decode roundtrip
    └── query_tests.rs                        # end-to-end against real curl/git data
```

`build.rs` is small — it just invokes `build::run()` and re-runs when
any `data/fish-snapshot/**` file changes. The bulk of the build code
lives in `build/` so it is reachable from unit tests via a normal
`#[path]` include.

**Dependency direction**: `cli-completions-data-fish` depends on
`cli-completions` (it constructs blobs in the format the runtime
crate defines) — never the other way around. This keeps the MIT
runtime free of any GPL data and lets future MIT data crates plug
in without touching the runtime.

## 5. Source Format: the Subset of Fish We Understand

Fish's `complete` builtin is documented at <https://fishshell.com/docs/current/cmds/complete.html>.
The full surface is large, but in `share/completions/*.fish` only a
handful of patterns appear at scale.

### 5.1 The directive shape we keep

```
complete -c CMD [-s S] [-l LONG] [-d DESC] [-n PREDICATE] [-a ARGLIST]
                [-x] [-f] [-r] [-F]
```

Field mapping into our internal `CompletionEntry`:

| Fish flag | Meaning                                            | Stored as                          |
|-----------|----------------------------------------------------|------------------------------------|
| `-c CMD`  | The command this entry belongs to                  | `command: InternId`                |
| `-s S`    | Short option (single char), e.g. `-s v` → `-v`     | `short: Option<u8>`                |
| `-l LONG` | Long option, e.g. `-l verbose` → `--verbose`       | `long: Option<InternId>`           |
| `-d DESC` | Human description (UTF-8, may contain spaces)      | `description: Option<InternId>`    |
| `-n PRED` | Gating predicate (see §5.2)                        | `subcommand_path: SmallVec<u8;4>`  |
| `-a LIST` | Static space-separated argument literals           | `arg_values: Option<&[InternId]>`  |
| `-x`      | Exclusive — option requires arg, no file fallback  | `flags: REQUIRES_ARG \| NO_FILES`  |
| `-f`      | No file-name completion                            | `flags: NO_FILES`                  |
| `-r`      | Requires an argument                               | `flags: REQUIRES_ARG`              |
| `-F`      | Force file completion (we record but don't act)    | `flags: FORCE_FILES`               |

A directive that produces nothing useful — neither short nor long
option nor static `-a` list — is dropped.

### 5.2 Predicate handling (`-n …`)

The `-n` flag gates entries on a fish boolean expression. We
recognise a fixed set of patterns; everything else is dropped (or
the entry is included unconditionally if the predicate is empty).

| Pattern                                              | Interpretation                              |
|------------------------------------------------------|---------------------------------------------|
| `__fish_use_subcommand`                              | Top-level only (no subcommand active)       |
| `__fish_seen_subcommand_from X [Y …]`                | Within subcommand X, Y, …                   |
| `not __fish_seen_subcommand_from X …`                | Anywhere *except* X, …                      |
| `__fish_<cmd>_using_command X [Y …]` (e.g. `__fish_git_using_command remote add`) | Within subcommand path X | Y … |
| `__fish_<cmd>_needs_command`                         | Top-level only                              |
| `__fish_seen_argument -l flag` / `-s s`              | Drop (depends on prior cli state we lack)   |
| `commandline -ct ...`, `string match …`, anything else | Drop entry                                |

For patterns we accept, the matched subcommand names become a
`subcommand_path: SmallVec<InternId; 4>` on the entry. A query for
`["git", "remote", "add"]` matches any entry whose `subcommand_path`
is the empty path or a prefix of the query.

`__fish_<cmd>_using_command` is a strong signal of nested
subcommands — for `git`, fish uses it to express `git remote add`,
`git submodule update`, etc. We implement this as a regex over the
predicate text rather than tracing into the command-specific helper
function, because the helper bodies are imperative shell that we
cannot statically evaluate.

### 5.3 What we deliberately drop

- Wrapping `if … end`, `function … end`, `set -l` — we ignore the
  surrounding control flow and only consume top-level `complete`
  calls. This is safe because each `complete` line is self-contained;
  the loss is that conditional entries (e.g. "only on Linux") get
  emitted unconditionally. Acceptable.
- Dynamic `-a "(some_function)"` argument generators — we keep the
  option entry but drop the `arg_values` field.
- `-w WRAPPED_CMD` (wrap completions of another command). Future
  work; v1 ignores it. (Affects ~30 files: `time`, `nice`, `xargs`,
  ssh-wrappers, …)
- `complete --erase`, `complete --do-complete`. Out of scope.

### 5.4 Quoting

We need a faithful enough fish-quoting tokenizer:

- Single quotes `'…'` — literal, with `\\` and `\'` escapes only.
- Double quotes `"…"` — `\$`, `\"`, `\\` escapes; we treat `$var`
  as opaque (replace with the literal `$var` since we cannot
  resolve it).
- Bareword — split on unescaped whitespace.
- Backslash continuation `\\\n` joins lines.

This is implemented in `build/fish_lexer.rs`. ~150 LOC. Tested
against ~20 hand-picked tricky lines from `git.fish`, `curl.fish`,
`tar.fish`.

## 6. Build-Time Pipeline

`build.rs` runs on every `cargo build` of the crate, but Cargo's
fingerprinting means it only re-executes when:

- Any file under `data/fish-snapshot/` changes, **or**
- The build code under `build/` changes, **or**
- The `cli-completions` crate version bumps.

### 6.1 Steps

1. **Walk** `data/fish-snapshot/*.fish`. For each file derive the
   default command name from the filename (`curl.fish` → `curl`),
   used as a fallback when a `complete` line omits `-c`.
2. **Lex + parse** each file into a stream of static `Directive`
   structs. Lines we cannot parse are counted but not fatal —
   the build emits a warning summary at the end:
   `cli-completions: 1055 files, 31840 directives kept, 528 dropped`.
3. **Resolve predicates** into subcommand paths.
4. **Intern strings.** Three pools: command names (~1000),
   long option names + arg literals (~30 k), descriptions (~25 k).
   Common substrings are *not* deduplicated beyond exact match;
   the size win from prefix dedup isn't worth the lookup cost.
5. **Sort** entries within each command for binary search:
   `(subcommand_path, long_or_short_name)`.
6. **Encode** into `data/completions.bin` (see §7).
7. **Generate** `src/generated_index.rs`, a single
   `static COMMAND_INDEX: &[(&str, u32)]` sorted by command name —
   the only "code" that depends on the data shape.

### 6.2 Reproducibility

- Iteration order is sorted (file walk is sorted by name; directives
  within a file preserve source order; within a command we sort by
  the tuple above).
- String interning is order-stable (insertion order of first sight).
- `data/completions.bin` is content-hashed in `fish-snapshot.toml`
  so CI can verify reproducibility:
  `sha256(completions.bin) == fish-snapshot.toml::expected_sha256`.

### 6.3 Snapshot management

The fish source lives in `data/fish-snapshot/` (committed) — *not*
in `temp/fish-shell/`. `temp/` is for one-off scratch and is
gitignored elsewhere in the repo; the RFC's first task is to move
the relevant subset into the crate.

To refresh:

```
cargo xtask sync-fish --commit <sha>
```

…which fetches the named fish-shell commit, copies
`share/completions/*.fish` into `data/fish-snapshot/`, refreshes
`data/LICENSE-fish` and `data/fish-snapshot.toml`, and runs
`cargo build -p cli-completions` to regenerate the blob. The
xtask is the *only* thing in the repo that talks to the network.

## 7. Encoded Format (`completions.bin`)

Optimised for **compact size**, **streaming decode without
allocation**, and **zero deserialisation library**. Everything is
little-endian, lengths are `u32`.

```
+----------------+
| MAGIC "CLIC"   |  4 bytes
| VERSION u32    |  4 bytes
| header_len u32 |  4 bytes — offset of cmd table
+----------------+
| String pool A  |  command names (NUL-separated)
| String pool B  |  long names + arg literals
| String pool C  |  descriptions
+----------------+
| Cmd table      |  fixed-size, sorted by name_offset
|   for each cmd:
|     name_off:  u32   into pool A
|     entries_off: u32 into Entry array
|     entries_len: u16
|     subcmd_tree_off: u32  (0 if flat)
+----------------+
| Entry array    |  packed CompletionEntry
|   for each:
|     short:    u8           (0 = none)
|     flags:    u8           (REQUIRES_ARG | NO_FILES | FORCE_FILES | HAS_ARG_VALUES)
|     path_len: u8           (subcommand depth, 0 = top-level)
|     long_off: u32          into pool B (0 = none)
|     desc_off: u32          into pool C (0 = none)
|     path_off: u32          into a u32 array of pool-B offsets
|     args_off: u32          into a length-prefixed u32 array of pool-B offsets
+----------------+
```

`CommandRef::query(path, prefix)` decodes lazily: it binary-searches
the command table, walks `entries_len` records, filters by
`subcommand_path` (prefix-of-query) and `prefix`, returning a
`Vec<Match<'static>>`. No heap allocations beyond the result vector.

Estimated size based on raw inputs:

| Component                | Estimate |
|--------------------------|---------:|
| Command name pool A      |    ~12 K |
| Long names + args pool B |   ~600 K |
| Descriptions pool C      |   ~1.4 M |
| Cmd table (1000 × 18 B)  |    ~18 K |
| Entry array (32 k × 19 B)|   ~620 K |
| **Total**                | **~2.7 MB** |

That is acceptable for a wasm-bundled file (gzipped to ~700 KB on
the wire). §9.3 discusses where the file lives.

## 8. Public Rust API

The runtime crate (`cli-completions`) defines the type and decoder.
Each data crate (e.g. `cli-completions-data-fish`) owns its own
`include_bytes!` blob and exposes an `embedded()` accessor.

```rust
// crates/cli-completions/src/lib.rs

pub struct CompletionsDb<'data> { /* refs into a &'data [u8] blob */ }

impl<'data> CompletionsDb<'data> {
    /// Construct from a packed blob produced by the encoder.
    /// Returns Err if magic / version don't match.
    pub fn from_bytes(blob: &'data [u8]) -> Result<Self, FormatError>;

    /// Does the database know anything about `command`?
    pub fn has_command(&self, command: &str) -> bool;

    /// Query for matches on a given subcommand path.
    ///
    /// `path[0]` is the top-level command, e.g. ["git", "remote", "add"].
    /// `prefix` is what the user has typed so far for the current token,
    /// e.g. "--ver" or "-v" or "" for "show me everything".
    pub fn query<'a>(
        &'a self,
        path: &[&str],
        prefix: &str,
    ) -> CompletionIter<'a, 'data>;
}
```

```rust
// crates/cli-completions-data-fish/src/lib.rs
use cli_completions::CompletionsDb;

static BLOB: &[u8] = include_bytes!("../data/completions.bin");

/// Lazily-decoded reference to the embedded fish-derived database.
pub fn embedded() -> &'static CompletionsDb<'static> {
    static DB: std::sync::OnceLock<CompletionsDb<'static>> = std::sync::OnceLock::new();
    DB.get_or_init(|| CompletionsDb::from_bytes(BLOB)
        .expect("baked-in blob must be valid"))
}

pub struct CompletionIter<'a> { /* lazy iterator */ }
impl<'a> Iterator for CompletionIter<'a> { type Item = CompletionMatch<'a>; … }

pub struct CompletionMatch<'a> {
    pub kind: MatchKind,           // Long, Short, ArgValue, Subcommand
    pub label: &'a str,            // "--verbose", "-v", "remote", "stable"
    pub description: Option<&'a str>,
    pub flags: EntryFlags,
}
```

Notes:

- All returned strings borrow from the static blob — zero copies, zero
  allocation per match.
- `CompletionIter` filters lazily so callers can `take(50)` on commands
  with thousands of options (`gcc.fish` is 800+ entries) without
  walking the whole entry list.
- `MatchKind::Subcommand` is emitted when `prefix` does not start with
  `-` and the database has nested commands at `path` — e.g. typing
  `git re` returns `remote`, `rebase`, `reset`, … from the subcommand
  index, not from option entries.

## 9. Integration with `makefile-analyzer`

### 9.1 Cargo wiring

```toml
# crates/makefile-analyzer/Cargo.toml
[dependencies]
cli-completions           = { path = "../cli-completions" }
cli-completions-data-fish = { path = "../cli-completions-data-fish" }
```

`cli-completions` itself depends on **nothing** outside the standard
library at runtime. `cli-completions-data-fish` depends only on
`cli-completions` at runtime; its `build.rs` may use a build-time
dep like `walkdir` for ergonomic file iteration — build-time deps
don't bloat the wasm bundle.

Because the makefile extension links a GPL-2 data crate, the
extension's binary distribution becomes GPL-2. See §11.

### 9.2 New analyzer feature

Add `crates/makefile-analyzer/src/features/completion.rs`:

```rust
pub fn completions(
    file: &ParsedFile,
    line: u32,
    col: u32,
) -> Vec<CompletionItem> {
    // 1. Locate the AST node at (line, col).
    // 2. If it is not inside a recipe line, return [].
    // 3. Slice the recipe text up to col; tokenise as a shell-ish line.
    // 4. Resolve (command, subcommand_path, active_token).
    // 5. Call cli_completions::embedded().query(path, active_token).
    // 6. Map to wasm-friendly CompletionItem JSON.
}
```

The recipe-line detector reuses the parser's existing line classifier
(see [parse.rs](crates/makefile-analyzer/src/parse.rs)) — recipes are
already a first-class node. The shell tokenizer is intentionally
naive: split on whitespace, respect single/double quotes, recognise
`|`, `&&`, `||`, `;` as command separators (so `make foo && curl --|`
queries against `curl` not `make`). It does **not** try to expand
make variables; `$(VAR)` is treated as opaque. This is fine for v1.

### 9.3 Where the data blob lives

Two options; we recommend **A** for v1:

**A. Embed in wasm.** `cli-completions` does
`static DATA: &[u8] = include_bytes!("../data/completions.bin");`.
The blob ends up in the wasm payload. With `wasm-opt` and gzip
transport, the makefile extension's `.wasm` grows from ~150 KB to
roughly 800 KB–1 MB. Acceptable for a one-time install, no IO
plumbing needed, no init-order concerns.

**B. Side-load.** The `.bin` ships next to the wasm in the extension's
`dist/`. The TS bridge reads it via `fs.readFile` at activation and
passes the bytes to `cli_completions::install_blob(&[u8])`. Smaller
wasm, but adds an init step and a code path that has to handle
"completions called before install". Worth doing only if (B) wasm
size becomes a real complaint.

### 9.4 New wasm export

```rust
// crates/makefile-analyzer/src/wasm_api.rs
use cli_completions_data_fish as fish;

#[wasm_bindgen]
impl Analyzer {
    pub fn complete(&self, uri: &str, line: u32, col: u32) -> JsValue {
        let db = fish::embedded();
        let items = features::completion::completions(self, uri, line, col, db);
        serde_wasm_bindgen::to_value(&items).unwrap()
    }
}
```

### 9.5 Extension-side wiring

```ts
// extensions/makefile/src/providers/completion.ts
class MakefileCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private bridge: AnalyzerBridge) {}
  provideCompletionItems(doc, pos) {
    return this.bridge.complete(doc.uri.toString(), pos.line, pos.character);
  }
}
```

Registered in [extension.ts](extensions/makefile/src/extension.ts) with
`triggerCharacters: ['-', ' ']` and `MAKEFILE_SELECTOR`.

## 10. Performance Targets and Sizing

| Metric                                            | Target          | Rationale |
|---------------------------------------------------|-----------------|-----------|
| Embedded blob size                                | < 3 MB          | Fits in wasm payload; ≤1 MB on the wire after gzip |
| `query("git remote add", "--c")` end-to-end       | < 1 ms          | Binary search + linear walk of one command's entries |
| Build-script time on cold cache                   | < 5 s           | 1055 small files; one full pass |
| Build-script time on warm cache                   | 0 (fingerprint) | No fish file changed → Cargo skips it |
| Wasm cold-start overhead                          | < 5 ms          | Just maps `include_bytes!` into linear memory |

Benchmarks live in `crates/cli-completions/benches/query.rs`
(criterion). Tracked but not gated in CI for v1.

## 11. License: GPL-2 Compatibility

> **This is the single biggest design constraint and must be resolved
> before code is written.**

fish-shell is GPL-2-or-later. Its `share/completions/*.fish` files
are part of the fish source distribution and inherit the project
license unless individually marked otherwise. (Spot-checking the
files in scope: none carry a separate license header.)

Embedding GPL-2 data in a Rust crate makes the *crate* a derivative
work, which means the crate must itself be distributable under GPL-2
or a compatible license — and any binary that statically links it
inherits the obligation. The `wx-vsce-makefile` extension is
currently MIT (see `extensions/makefile/LICENSE.md`). Shipping a
wasm bundle that links `cli-completions` would put the extension's
binary distribution in conflict with its source license.

### 11.1 Decision

**Two-crate split, agreed.**

```
crates/cli-completions/             — MIT             — runtime + blob format
crates/cli-completions-data-fish/   — GPL-2-or-later  — fish snapshot + extractor + blob
```

- The runtime crate carries no GPL data and stays MIT-licensed, so
  it is reusable from any future MIT-only consumer that wants to
  bring its own data backend (e.g. Fig specs, hand-curated, etc.).
- The data crate is GPL-2-or-later — the same terms fish itself
  uses — and bundles the fish source files alongside the generated
  blob. Anyone redistributing the data crate redistributes fish's
  GPL terms with it.
- The Makefile extension depends on both crates. Because the wasm
  artefact links the GPL-2 data crate, the **binary distribution of
  `wx-vsce-makefile` is GPL-2-or-later**. The extension's source
  files keep their MIT headers; only the combined binary changes.

Discarded options (kept for the record):

1. **Re-license the runtime crate as GPL-2.** Forces every future
   consumer of the runtime onto GPL-2 even if they bring an
   MIT-only data set. Strictly worse than the split.
2. **Switch source to Fig or carapace.** Discards the user's
   stated preference for fish.
3. **Upstream license exception.** Not pursued.

### 11.2 Attribution and packaging requirements

- `crates/cli-completions-data-fish/LICENSE` — full GPL-2 text,
  copied verbatim from `temp/fish-shell/COPYING`.
- `crates/cli-completions-data-fish/README.md` — names fish, links
  to upstream, records the snapshot commit and date pulled from
  `data/fish-snapshot.toml`.
- `extensions/makefile/LICENSE.md` — change to GPL-2-or-later (or
  add a "this VSIX is distributed under GPL-2-or-later because it
  includes fish-shell completion data" notice).
- `extensions/makefile/package.json` — `"license"` field updated
  to `"GPL-2.0-or-later"`; `"repository"` and marketplace listing
  text updated likewise.
- `extensions/makefile/THIRD_PARTY_NOTICES.md` — created if absent;
  add a fish-shell entry citing upstream + license + commit.
- The published VSIX must include both `LICENSE` files (the
  extension's and fish's) inside the bundle.

## 12. Testing Strategy

### 12.1 Unit tests inside `cli-completions`

- `parser_tests.rs` — feed crafted `.fish` snippets, assert the
  extracted directive list. Cover all of §5.1's flags, all of §5.2's
  predicate patterns, single/double/escape quoting, line continuation.
- `encode_tests.rs` — encode a synthetic dataset → decode → equality.
  Plus a snapshot test on the byte layout of a tiny fixture.
- `query_tests.rs` — load the *real* generated blob, run a fixed
  battery of queries and snapshot the output via `insta`. Examples:
  - `query(["curl"], "--ana")` → exactly `--anyauth`.
  - `query(["git", "remote", "add"], "")` → at least `--fetch`,
    `--track`, `--mirror`.
  - `query(["does-not-exist"], "")` → empty.

(Per the user's saved feedback in
[memory/feedback_test_location.md](/Users/will/.claude/projects/-Users-will-Project-github-xuwaters-vscode-extensions/memory/feedback_test_location.md):
all verification code lives as in-tree crate tests — no scratch
node/shell scripts in /tmp.)

### 12.2 Tests in `makefile-analyzer`

- `completion_tests.rs` — given a Makefile string and a cursor
  position, assert the top-N completion labels. Use `insta`
  snapshots.
- Negative tests: cursor on a target line, on a variable
  assignment, on a comment, on a blank line — all return `[]`.

### 12.3 Build determinism check

A CI step:

```
cargo build -p cli-completions
sha256sum crates/cli-completions/data/completions.bin \
  | grep "$(yq .expected_sha256 < crates/.../fish-snapshot.toml)"
```

Catches accidental nondeterminism early.

### 12.4 Manual integration

Run the makefile extension in the VSCode Extension Host
(`pnpm run watch` + F5). Type into a recipe line:

```
build:
\tcurl --
\tgit remote ad
\tdocker run --
```

Visually confirm the popup. Check the description text isn't
truncated, the items are sorted sensibly, `Tab` accepts.

## 13. Phased Plan

| Phase | Deliverable | Done when |
|-------|-------------|-----------|
| 0 | This RFC merged | Accepted (2026-04-29); license path = two-crate split (§11.1) |
| 1 | `cli-completions` skeleton | Crate compiles, empty database, public API stable, all tests in §12.1 wired |
| 2 | Fish snapshot vendored | `data/fish-snapshot/` populated; `cargo xtask sync-fish` works; LICENSE bundled |
| 3 | Build pipeline | `cargo build` produces `completions.bin`; build-determinism CI check green |
| 4 | Runtime decoder + query | `query(["curl"], "--an")` returns `--anyauth` end-to-end; criterion bench in place |
| 5 | Makefile integration | `MakefileCompletionProvider` registered; popup works in F5 host |
| 6 | Polish | Subcommand-tree (`git remote ad`); arg-value completion (`tar --format=`); description trimming |
| 7 | Future | Bash/zsh completion crate sibling; user-overrides; `-w` (wrapper) handling |

Each phase is independently mergeable. Phases 1–4 land in the crate;
phase 5 is a small change to the makefile extension; phase 6 is
incremental polish; phase 7 is post-v1.

## 14. Open Questions

1. **Wasm vs side-load (§9.3).** Default to embed; reconsider if
   bundle size becomes a real complaint.
2. **Snapshot refresh cadence.** Quarterly? Tied to fish releases?
   Suggest: "whenever a user reports a missing or wrong completion,
   plus at minimum once a year". Documented in the data crate's
   README.
3. **Per-workspace overrides.** Out of v1; if/when added, the
   override mechanism should be fish-syntax `.fish` files under
   `.vscode/completions/` so users can copy-edit upstream entries.

## 15. Summary

`cli-completions` (MIT) + `cli-completions-data-fish` (GPL-2)
transform the vendored fish snapshot into a 2–3 MB embedded blob;
the runtime exposes a single `query(path, prefix)` function with
no I/O and no runtime config; `makefile-analyzer` adds a
`complete()` wasm export; the extension registers a
`CompletionItemProvider`. After phase 5, typing `\tcurl --` in a
Makefile yields ~250 curl options with descriptions, `\tgit
remote ad` yields `add`, `\ttar --create --` yields tar's
create-mode flags, and the same applies to ~1000 other commands
at zero ongoing cost.

The makefile extension's binary distribution becomes
GPL-2-or-later as a consequence (§11.2). The runtime crate stays
MIT and reusable.
