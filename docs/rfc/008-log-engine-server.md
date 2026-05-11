# RFC 008: `log-engine-server` — Out-of-Process Log Engine for Zed, CLI, and Beyond

**Status**: Draft
**Date**: 2026-05-11
**Depends on**: [RFC 007](007-log-viewer-large-files.md) (specifically §4.1's `crates/log-engine` carve-out and §5.4's persistent index file format)
**New crates**:
  - `crates/log-engine-server` (binary — JSON-RPC over stdio)
  - `crates/log-engine-cli` (binary — one-shot CLI verbs)
**New extensions / consumers**:
  - `extensions/zed-log-viewer` (Zed extension — talks to the server)
  - *(VSCode extension may optionally migrate; not required — see §9.2)*

---

## 1. Motivation

RFC 007 split the parsing / indexing / matching logic into
`crates/log-engine`, a transport-agnostic Rust crate. The VSCode
extension consumes it through a thin `wasm-bindgen` adapter
(`crates/log-parser`). That delivers large-file support inside
VSCode but leaves the engine **trapped behind a single editor's
extension model**.

The same engine should power:

1. **Zed.** No WASM-extension log viewer exists today. Zed
   extensions can spawn host processes, so an out-of-process server
   is the natural integration point.
2. **CLI use.** `lnav`, `klogg`, `glogg`, `less +F`, `tail | grep`
   are all things people reach for. A `logcat`-style binary built on
   the same engine reuses 100 % of the indexing/matching code and
   the persistent index cache (§4.5 below), so a file pre-indexed
   from the CLI opens instantly in VSCode and vice versa.
3. **Future editors.** Helix, Neovim (via a shim), Sublime, JetBrains
   — anything that can spawn a subprocess and speak JSON-RPC over
   stdio.

The cost of doing this once, properly, is that downstream consumers
become **trivial adapters** (≤300 LoC each) rather than full re-
implementations of the indexing pipeline. The engine and the index
cache are shared across all of them.

This RFC proposes:

- `crates/log-engine-server` — a long-lived binary that wraps
  `log-engine`, accepts JSON-RPC requests over stdio, and streams
  results back. Sessions are identified by id; one server multiplexes
  many open files.
- `crates/log-engine-cli` — a small companion binary with `index`,
  `window`, `filter`, `search` subcommands, designed to be useful
  from a shell pipeline and to pre-warm the shared index cache.
- `extensions/zed-log-viewer` — a Zed extension that spawns the
  server and renders log windows in a Zed buffer-like view.
- A versioned wire protocol (§3) frozen on the v1 release.
- A binary distribution story (§7) that ships per-platform artefacts
  via GitHub Releases, with editor extensions either bundling the
  matching binary or fetching on first run.

## 2. Goals and Non-Goals

### Goals

1. **One engine, many adapters.** Every consumer of `log-engine`
   either links the crate (Rust users) or talks to the server. No
   downstream consumer reimplements indexing, anchor scanning, ANSI
   parsing, or the on-disk index format.
2. **Index cache parity.** A `.idx` file written by VSCode (§5.4 of
   RFC 007) is loadable by the server, and vice versa, byte-for-
   byte. Pre-indexing from the CLI accelerates a subsequent open in
   any editor.
3. **Streaming-first protocol.** Filter, search, and indexing all
   stream incremental progress. Cancellation is sub-100 ms.
4. **Crash-isolated.** A regex pathological case crashes the server
   process, not the editor. The editor respawns; persistent state is
   recovered from the cache.
5. **Editor-agnostic protocol.** Nothing in the wire schema mentions
   VSCode, Zed, or any specific editor. The Zed extension is the
   first consumer; a future "VSCode native-process mode" could
   speak the same protocol.
6. **Sub-100 ms cold start.** Server startup → first window for a
   small file in <100 ms on a warm-cache index. Cold-cache index
   builds at the same rate as the in-process worker (≥1 GB/s on
   modern SSDs; bound by disk).
7. **Trivially debuggable.** JSON-RPC over stdio means
   `cat <<EOF | log-engine-server` reproduces any bug; logs go to
   stderr and can be tee'd.

### Non-Goals

- **Replacing the VSCode extension's WASM path.** Web-host support
  (vscode.dev, Codespaces browser) requires WASM. The server is
  *additional*, not a replacement. §9.2 sketches an opt-in native
  mode.
- **Speaking LSP.** The full Language Server Protocol vocabulary —
  `textDocument/*`, completions, hovers, diagnostics, semantic
  tokens — is built around editing source code. A log viewer needs
  none of it. We borrow LSP's wire framing (`Content-Length:`
  headers + JSON body) for tooling familiarity (`vscode-jsonrpc`,
  `tower-lsp`-style scaffolding) and nothing else.
- **A TUI.** `logcat` is a one-shot CLI, not an `lnav`-style
  full-screen viewer. A TUI built on the same engine is a separate
  project.
- **Network transport.** v1 is local stdio only. Remote use cases
  go through the editor's own remote-development story (VSCode
  Remote, Zed SSH).
- **Authentication / multi-user.** Single user, single host.
- **Live tail (`-f`).** Same boundary as RFC 007 §2 — out of scope
  for v1, but the protocol leaves room for `session/follow`.

## 3. Wire Protocol

### 3.1 Framing

Identical to LSP's framing — `Content-Length: N\r\n\r\n` followed by
exactly `N` bytes of UTF-8 JSON. This is *not* an endorsement of LSP
semantics; it is reuse of an extremely well-supported parser layer.
Existing JSON-RPC libraries in TypeScript (`vscode-jsonrpc`), Rust
(`jsonrpc-stdio-server`, `tower-lsp` after stripping LSP types),
Python (`pylsp-jsonrpc`), and Go (`jsonrpc2`) handle this framing
out of the box.

### 3.2 Methods

All methods are JSON-RPC 2.0. Notation: `→` is request, `←` is
response, `↪` is server-initiated notification.

#### `initialize`

```jsonc
→ { "method": "initialize", "params": {
      "clientName": "zed-log-viewer/0.1.0",
      "indexLocation": "globalStorage" | "adjacent" | "directory",
      "indexDirectory": "/optional/abs/path",
      "indexAnchorStride": 1024,
      "renderAnsi": true
    }}
← { "result": {
      "serverVersion": "0.1.0",
      "protocolVersion": 1,
      "capabilities": {
        "filter": true,
        "search": true,
        "tailFollow": false,        // future
        "bisectionTailJump": true   // RFC 007 §5.3
      }
    }}
```

`initialize` is required before any session method. Calling a
session method before `initialize` returns `error.code = -32002` (LSP-
compatible "server not initialized").

#### `session/open`

```jsonc
→ { "method": "session/open", "params": { "path": "/abs/path/to.log" }}
← { "result": {
      "sessionId": "s7",
      "fileSize": 10737418240,
      "totalLines": null,        // null until index complete
      "indexComplete": false,
      "indexCacheHit": true | false
    }}
```

After `session/open` the server emits `index/progress` notifications
until indexing completes (immediate if `indexCacheHit = true`).

#### `session/window`

```jsonc
→ { "method": "session/window",
    "params": { "sessionId": "s7", "start": 1000, "count": 200 }}
← { "result": {
      "start": 1000,
      "lines": [
        { "html": "...", "text": "..." },
        ...
      ]
    }}
```

Synchronous (no streaming). Returns within a few ms once the index
has covered `start..start+count`. If the index hasn't reached that
range yet and bisection is disabled, the server blocks until it has
or returns `error.code = 1001` ("range beyond indexed region")
depending on a `params.blocking` flag.

#### `session/filter`

```jsonc
→ { "method": "session/filter",
    "params": {
      "sessionId": "s7",
      "requestId": "f-42",
      "rules": [{ "name": "...", "pattern": "...", "regex": true,
                  "caseSensitive": false, "color": "..." }]
    }}
← { "result": { "accepted": true }}                       // immediate

↪ { "method": "filter/partial",
    "params": {
      "sessionId": "s7",
      "requestId": "f-42",
      "scannedBytes": 1073741824,
      "totalBytes": 10737418240,
      "hits": [
        { "line": 1234, "ruleIndex": 0 },
        ...
      ]
    }}
↪ { "method": "filter/done",
    "params": { "sessionId": "s7", "requestId": "f-42",
                "totalHits": 12450, "truncated": false }}
```

Hits stream in 30 Hz batches (same coalescing as RFC 007 §7.4).
Cancellation via `session/cancel`. Capped at 1 M hits per request
with `truncated: true` when exceeded.

#### `session/search`

Same shape as `session/filter`, with `params.query`, `params.regex`,
`params.caseSensitive`. Notifications are `search/partial` / `search/done`.

#### `session/cancel`

```jsonc
→ { "method": "session/cancel",
    "params": { "sessionId": "s7", "requestId": "f-42" }}
← { "result": { "cancelled": true | false }}
```

`cancelled: false` means the request had already completed.

#### `session/close`

```jsonc
→ { "method": "session/close", "params": { "sessionId": "s7" }}
← { "result": null }
```

Drops the open `fd`, releases the in-memory index, persists any
unsaved cache. In-flight requests for that session are cancelled.

#### `shutdown` / `exit`

LSP-style. `shutdown` returns `null`; `exit` is a notification that
terminates the process. Editors are expected to send `shutdown`
before `exit`; the server tolerates a bare `exit`.

### 3.3 Notifications from server

- `index/progress` — `{ sessionId, lines, bytes, complete }` during
  indexing.
- `filter/partial`, `filter/done` — see above.
- `search/partial`, `search/done` — see above.
- `session/invalidated` — `{ sessionId, reason: "fileChanged" }`
  when an open file's mtime/size changes underneath us. Editor
  decides whether to reopen.
- `log/message` — `{ level, text }` for diagnostic logging when the
  client sets a verbosity flag at `initialize` time.

### 3.4 Errors

Standard JSON-RPC error codes plus a small extension table:

| Code  | Meaning                                          |
|-------|--------------------------------------------------|
| -32700 | Parse error                                     |
| -32600 | Invalid request                                 |
| -32601 | Method not found                                |
| -32602 | Invalid params                                  |
| -32603 | Internal error                                  |
| -32002 | Server not initialized                          |
| 1001   | Range beyond indexed region (non-blocking mode) |
| 1002   | Session not found                               |
| 1003   | File no longer accessible (open `fd` failed)    |
| 1004   | Regex compile failed (returns `data.message`)   |

Compile errors include the offending pattern and a one-line message
in `data` so editors can surface them inline.

## 4. Server Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ log-engine-server (one process per editor workspace)         │
│                                                              │
│  stdin  ──► JsonRpcDecoder ──► Dispatcher ──► tokio::spawn   │
│                                              one task per    │
│                                              request         │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────┐          │
│  │ SessionMap           │  │ Index cache I/O      │          │
│  │ id → SessionState    │  │ (RFC 007 §5.4 fmt)   │          │
│  │   - Arc<LogEngine>   │  └──────────────────────┘          │
│  │   - open fd          │                                    │
│  │   - in-flight tasks  │  ┌──────────────────────┐          │
│  └──────────────────────┘  │ rayon thread pool    │          │
│           │                │ (filter/search scan) │          │
│           │                └──────────────────────┘          │
│           ▼                                                  │
│  Notifier (mpsc) ──► JsonRpcEncoder ──► stdout               │
└──────────────────────────────────────────────────────────────┘
```

Key choices:

- **`tokio` runtime, multi-threaded.** Filter/search passes are
  CPU-bound; we want all cores. JSON-RPC handling is trivially
  async.
- **`rayon` for the inner scan.** A filter pass divides the file
  into N-MB chunks and processes them in parallel; per-line
  matching within a chunk is sequential. Order-preserving result
  collection via `rayon::iter::ParallelIterator::collect_into_vec`
  keyed on chunk index.
- **`memmap2` for indexing.** mmap'ing the file lets the kernel
  page in chunks on demand and lets multiple filter passes share
  the mapping. Falls back to `pread` on failures (network FS, very
  large files on 32-bit hosts).
- **One `Notifier` channel per process.** The serial encoder is the
  natural backpressure point — if the editor stops draining stdout,
  the channel fills and worker tasks block on `send()`. We use
  `tokio::sync::mpsc::channel(256)` rather than unbounded.
- **Per-request cancellation via `tokio_util::sync::CancellationToken`.**
  `session/cancel` looks up the token in the session and triggers
  it; long-running scan loops `select!` on it between chunks.

### 4.1 SessionState lifetime

```rust
struct SessionState {
    path: PathBuf,
    fd: File,                          // kept open for window reads
    mmap: Option<Mmap>,                // Some on local FS
    engine: Arc<log_engine::Index>,    // anchor table + metadata
    tasks: HashMap<RequestId, CancellationToken>,
    last_used: Instant,
}
```

Sessions are dropped when (a) the editor calls `session/close`, or
(b) `last_used` exceeds an idle timeout (default 5 min) **and** no
in-flight requests remain. The latter prevents a forgetful editor
from leaking sessions across days of uptime.

### 4.2 Idle process exit

If the server has zero sessions for 60 s (configurable via
`--idle-exit`), it exits cleanly. Editors respawn on the next
request. The persistent index cache means respawn is cheap.

## 5. Client-Side Adapters

### 5.1 Zed extension (`extensions/zed-log-viewer`)

Zed extensions today are WASM (Rust → wasm32-wasi → Zed extension
host). They cannot themselves do unrestricted I/O, but they **can
spawn host commands** via the extension API and pipe stdio. Our
extension does exactly that:

```rust
// extensions/zed-log-viewer/src/lib.rs (sketch)
struct LogViewerExtension { /* spawns and tracks the server process */ }

impl zed::Extension for LogViewerExtension {
    fn language_server_command(...) -> Result<Command> {
        Ok(Command {
            command: bin::resolve("log-engine-server"),
            args: vec!["--stdio".into()],
            env: Default::default(),
        })
    }
    // ... wire JSON-RPC client to read/write stdio,
    //     register a Zed UI surface for log windows
}
```

Implementation choices that the Zed extension owns (not the server):

- Whether to render in Zed's editor surface or a dedicated panel.
- ANSI color → Zed theme color mapping (the server returns HTML;
  the Zed extension may prefer the `text` field plus a side-channel
  list of styled spans — see §3.2 open question 3).
- Filter UI, search UI, virtual scroll. Zed has its own buffer
  primitives — the extension reuses them rather than reimplementing
  a webview.

`extensions/zed-log-viewer` is a separate workspace from the
existing VSCode extensions. It is built and packaged via
`zed-extension-tool`, not pnpm.

### 5.2 CLI adapter (`crates/log-engine-cli`)

A small binary, **not** a TUI:

```
$ logcat index /var/log/big.log
indexed 10.0 GB / 12480501 lines in 4.8s → cache 0.95 MB
$ logcat window /var/log/big.log --line 9000000 --count 50
... 50 lines ...
$ logcat filter /var/log/big.log --regex 'ERROR.*timeout' --json
{"line":314159,"text":"...","ruleIndex":0}
{"line":314162,"text":"...","ruleIndex":0}
...
$ logcat search /var/log/big.log 'request_id=abc' | head
```

Subcommands:

| Subcommand | Purpose |
|------------|---------|
| `index`    | Build/refresh the index cache for a file. Useful as a pre-warm step in CI logs. |
| `window`   | Print a line range. Default = head; `--line N` jumps. `--no-color` strips ANSI from output. |
| `filter`   | Apply rules (from `--rules-file` JSON or `--regex/--name/--color` flags) and stream matches. |
| `search`   | Single-pattern search; reads cache if present. |
| `cache`    | `cache list`, `cache clear`, `cache info <path>`. |

Output formats: human (default, with ANSI when stdout is a TTY),
`--json` (NDJSON), `--csv`. The filter/search output formats are
the protocol's JSON shape, lightly flattened — anything that reads
the server's JSON can also pipe `logcat` output.

`logcat` writes to and reads from the same `.idx` cache as
`log-engine-server` and the VSCode extension (RFC 007 §5.4). A
`logcat index` run before opening the file in VSCode means the
editor sees `indexCacheHit: true` and renders instantly.

### 5.3 VSCode native-process mode (opt-in, future)

Out of scope for v1, sketched here for protocol design. A
`logViewer.useNativeServer: true` setting would have the VSCode
extension spawn `log-engine-server` instead of loading the WASM
adapter. Tradeoffs already covered in [RFC 007 §11.5](007-log-viewer-large-files.md#L795-L835).

### 5.4 Future adapters

Helix (similar story to Zed — spawn process), Sublime (Python
extension API can spawn), JetBrains (Kotlin extension), Neovim (Lua
extension can use `vim.lsp.start_client` over stdio with a custom
schema). All ≤300 LoC of glue.

## 6. Index Cache Sharing

The on-disk index format defined in [RFC 007 §5.4.1](007-log-viewer-large-files.md#L307)
is owned by `log-engine` (not by either consumer), which means the
server reads and writes the **same files** as the VSCode extension.
Cross-consumer pre-warming is real:

- `logcat index *.log` in a CI job → developers open any of those
  files in VSCode or Zed and skip the index scan.
- A VSCode user who indexed a 10 GB log yesterday and switches to
  Zed today reuses yesterday's index.

Settings honored by all three consumers (server, CLI, VSCode):

- `indexLocation` (`globalStorage` | `adjacent` | `directory`)
- `indexDirectory` (when `directory`)
- `indexAnchorStride`

The CLI's "global storage" path is platform-conventional: `$XDG_CACHE_HOME/log-engine/index/`
on Linux, `~/Library/Caches/log-engine/index/` on macOS,
`%LOCALAPPDATA%\log-engine\index\` on Windows. The VSCode
extension's `globalStorageUri` resolves to a different directory by
default — so cross-tool sharing requires either:

- **Both** consumers using `indexLocation: "adjacent"`, or
- A user setting `indexLocation: "directory"` to a shared path
  (e.g. `~/.cache/log-engine/index`) in both VSCode settings and
  the CLI.

The default is "no cross-tool sharing for free" — same-tool
re-opens always hit the cache, but cross-tool sharing is an opt-in
configuration. We document this rather than try to magic it.

## 7. Distribution

### 7.1 Per-platform binaries

`log-engine-server` and `log-engine-cli` are built for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`
- `aarch64-pc-windows-msvc`
- `x86_64-pc-windows-msvc`

Binary size: stripped release builds are estimated ~5 MB each
(`tokio` + `regex` + `memmap2` are the bulk).

Built via GitHub Actions matrix on push to `release/log-engine-*`
tags. Artefacts published to:

- `crates.io` (`log-engine-server`, `log-engine-cli`) — for
  `cargo install`.
- GitHub Releases — tarballs per platform, signed with cosign.
- (Future) Homebrew tap, AUR, scoop.

### 7.2 Editor extension bundling

Two approaches; we recommend **B**:

**A. Bundle the matching binary in each platform-specific
extension.** VSCode-style: publish `wx-vsce-log-viewer-darwin-arm64`,
`...-linux-x64`, etc. Each VSIX is small (~6 MB). Marketplace
auto-installs the right one.

**B. Fetch on first activation.** Extension ships ~50 KB of
loader code. On first use, it downloads the matching binary from
GitHub Releases into the extension's `globalStorage` directory,
verifies the sha256, makes it executable, and caches it. Updates
fetch a new binary when the extension version bumps. Smaller
install footprint, but requires network on first use and adds the
trust question (the loader must verify against a hardcoded sha256
table, regenerated per release).

Recommend **B** for Zed and **A** for VSCode (if VSCode ever opts
in via §5.3) — the VSCode marketplace already does platform-
specific extensions natively, so we'd be working against the
infrastructure to fetch separately. Zed has no equivalent as of
2026, so fetching is the path of least resistance.

### 7.3 Discovery

Editors locate the binary by, in order:

1. The `serverPath` setting (absolute path), if set.
2. `$XDG_DATA_HOME/log-engine/log-engine-server` (or platform
   equivalent) — the cached download location.
3. `$PATH` — `which log-engine-server`.
4. Bundled-with-extension fallback (mode A).

If all fail, the editor surfaces a one-time install prompt
("Download log-engine-server from GitHub Releases? [y/N]") and
proceeds.

## 8. Testing Strategy

### 8.1 Engine tests stay in `log-engine`

Per RFC 007 §1a, the engine carries its own tests. Nothing about
the server changes that.

### 8.2 Protocol golden tests

`crates/log-engine-server/tests/protocol/` holds a battery of
`.txt` fixtures, each containing a script of `Content-Length:`
framed JSON-RPC messages and the expected server output. Tests run
the server as a subprocess, pipe the script, diff stdout. Adding a
new method = adding a fixture.

### 8.3 Cancellation timing

A test that opens a 1 GB synthetic log, starts a slow regex filter,
sends `session/cancel` after 50 ms, and asserts the `filter/done`
notification arrives within 100 ms with `cancelled: true`.

### 8.4 Index cache compatibility

A test in `log-engine` writes a `.idx` via the encoder, then loads
it via the decoder. A separate test in `crates/log-engine-server`
ensures the server can load a cache file written by the CLI in the
prior step. A separate VSCode-side test (per RFC 007's phase 3)
ensures the inverse direction works.

(Per the saved feedback in
[memory/feedback_test_location.md](/Users/will/.claude/projects/-Users-will-Project-github-xuwaters-vscode-extensions/memory/feedback_test_location.md):
all verification code lives as in-tree crate tests — no scratch
node/shell scripts in /tmp.)

### 8.5 Crash recovery integration test

Spawn the server, send `session/open` for a file containing a regex
that the server is configured to abort on (test-only flag), assert
the editor-side detects exit, respawns, and re-opens cleanly with
`indexCacheHit: true`.

### 8.6 Manual cross-tool

```
$ logcat index test/big.log         # writes ~/.cache/log-engine/...
$ code test/big.log                 # opens instantly, no scan
$ zed test/big.log                  # also instant
```

Three-way smoke test before each release.

## 9. Open Questions

1. **`html` vs styled-spans in the protocol.** The server currently
   returns `{ html, text }` per line, mirroring the WASM adapter.
   HTML is fine for a webview but awkward for Zed (which would have
   to strip and re-style). An alternative: return `{ text, spans:
   [{start, end, fg, bg, bold, …}] }` and let each adapter render.
   Cost: more work in the server, but it's the right shape for
   non-web consumers. Lean toward the spans format for v1, with an
   `initialize` flag to select.
2. **Auto-update.** Should `log-engine-server` self-update from
   GitHub Releases, or do editors do it? Editor-driven is simpler;
   self-update means a single binary upgrade serves many editors
   at once. Defer to v1.1.
3. **Telemetry.** None in v1. If we add it later: opt-in,
   anonymous, off by default, only reports query latencies and
   error codes — never log content.
4. **Protocol versioning.** v1 freezes the methods listed in §3.2.
   Additive changes (new optional params, new notifications) bump
   the minor `protocolVersion`; breaking changes bump major.
   Clients negotiate via `initialize` and downgrade behaviour for
   unknown capabilities.
5. **Sandboxing.** The server reads files the user can read; that's
   the same trust boundary as the editor. Should we add a
   `--restrict-paths` flag for shared/CI environments? Useful but
   not v1.
6. **Live tail.** The `tailFollow` capability bit in §3.2 is `false`
   in v1 but reserved. Adding it later: a `session/follow` request
   that streams `index/progress` notifications past the previous
   EOF as the file grows.
7. **Should the CLI gain a TUI subcommand?** `logcat tui` calling
   into a `crossterm`/`ratatui` viewer is appealing but is its own
   project. Out of v1.

## 10. Phased Plan

| Phase | Deliverable | Done when |
|-------|-------------|-----------|
| 0 | This RFC accepted | Reviewer sign-off; protocol shape frozen for v1 |
| 1 | `crates/log-engine-server` skeleton | Crate compiles; `initialize` / `shutdown` / `exit` work; protocol golden test framework in place |
| 2 | Session lifecycle | `session/open` / `session/window` / `session/close`; `index/progress` notifications; integration test against a 1 GB fixture |
| 3 | Filter + search streaming | Both methods work end-to-end with cancellation; cancellation latency test passing |
| 4 | `crates/log-engine-cli` | `index`, `window`, `filter`, `search`, `cache` subcommands; cross-tool cache compatibility test green |
| 5 | Distribution: GH Actions matrix + Releases | Tagged releases produce six platform binaries with sha256 manifest; cosign signatures verified |
| 6 | `extensions/zed-log-viewer` | Open a 1 GB log in Zed; scroll, filter, search work; published to Zed extension registry |
| 7 | Polish | Auto-update story (open question 2); styled-spans format if chosen (open question 1); protocol v1.1 if needed |

Phases 1–6 are the v1 cut. Phase 7 is post-launch.

## 11. Risks

- **Binary distribution overhead.** Six platforms × maintenance
  forever. Mitigated by: GH Actions matrix (no manual cross-compile),
  `cargo install` as a fallback for unsupported platforms,
  publishing to crates.io so users with a Rust toolchain are
  self-serve.
- **Protocol churn.** Once Zed users adopt this, breaking the
  protocol breaks them. Versioning (open question 4) is the
  mitigation but discipline matters more than mechanism.
- **Index cache cross-tool drift.** A bug in the encoder/decoder
  that affects only one consumer corrupts caches for all of them.
  Mitigated by: cache version field rejects unknown formats,
  encoder/decoder live in `log-engine` (single source of truth),
  cross-tool compatibility test (§8.4) gates releases.
- **Stdio buffering surprises.** Editors that fail to drain stdout
  cause backpressure. The bounded `mpsc` channel (§4) makes this
  visible (tasks block on send), but a misbehaving client could
  hang itself. Document the contract: clients must drain
  notifications continuously.
- **Per-platform CI cost.** Six runners on every push to a release
  branch. Acceptable on GitHub-hosted runners; revisit if we hit
  Actions minutes limits.

## 12. Summary

Take the engine that RFC 007 carved out, wrap it in a long-lived
binary that speaks JSON-RPC over stdio, and expose it to any editor
or CLI tool that can spawn a subprocess. The wire protocol is
deliberately editor-agnostic and deliberately not LSP — it's
purpose-built for byte-range queries and streaming filter/search
results. A companion CLI (`logcat`) provides shell-pipeline access
and pre-warms the cache. A Zed extension is the first non-VSCode
consumer; the same engine, the same index files, the same regex
matcher serve all of them.

The VSCode extension is unaffected by default — it keeps its WASM
path and its web-host support. If a future user complains that
warm-cache filter passes are slow in VSCode, an opt-in
`logViewer.useNativeServer` flag (§5.3) can switch to the binary
without changing any protocol or any cache file.

`log-engine` is the source of truth; `log-parser` (WASM),
`log-engine-server` (binary), and `log-engine-cli` (binary) are
adapters of equal rank.
