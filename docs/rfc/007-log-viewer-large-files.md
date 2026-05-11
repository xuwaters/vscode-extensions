# RFC 007: Log Viewer — Large File Support via Streaming Index + Windowed Render

**Status**: Draft
**Date**: 2026-05-11
**Affected components**:
  - `extensions/log-viewer` (host + webview)
  - `crates/log-parser` (WASM bridge)
**Supersedes / extends**: the implicit "load up to `logViewer.maxFileSizeBytes` (default 8 MB), truncate the rest" behaviour in [editorProvider.ts:132-147](extensions/log-viewer/src/editorProvider.ts#L132-L147).

---

## 1. Motivation

The current log-viewer is a "small-file viewer with ANSI". The host
reads the whole document via `TextDocument.getText()`, hands the full
string to a `LogIndex` constructor in WASM
([lib.rs:30-34](crates/log-parser/src/lib.rs#L30-L34)), and ships
**every parsed line** as `{ html: string[], text: string[] }` to the
webview ([lib.rs:46-67](crates/log-parser/src/lib.rs#L46-L67),
[editorProvider.ts:140-146](extensions/log-viewer/src/editorProvider.ts#L140-L146)).
Anything larger than `logViewer.maxFileSizeBytes` (8 MB by default) is
silently halved and the user sees a "truncated" badge — i.e. the
viewer refuses to look at the back half of the file.

Real production logs routinely run hundreds of MB or several GB:

- A pod's stdout collected over a week.
- A Gradle / Bazel build log with verbose subprocess output.
- A long-running test run with tracing turned up.
- Captured network traces, ffmpeg debug output, `RUST_LOG=trace`.

The user wants to open these files and **navigate to the end**, jump
around, search, filter — the things you'd do in `less +G` — without
reading them into the V8 heap.

This RFC proposes a complete change of strategy: instead of "parse
everything up front, render everything", we **stream-index** the file
on disk to learn where every line begins, then **render only the
window the user is looking at**. The viewer stops being bounded by
RAM and starts being bounded by disk-read throughput for one-time
indexing.

## 2. Goals and Non-Goals

### Goals

1. **Open files of arbitrary size.** A 10 GB log opens, scrolls, and
   jumps to the bottom. There is no truncation badge.
2. **Bounded memory.** Steady-state memory is `O(visible lines + index
   anchors + match results)`, not `O(file size)`. Concretely: under
   200 MB resident for a 10 GB log with default settings.
3. **Time-to-first-paint stays fast.** The viewer renders the head of
   the file (first ~200 lines) within the same time budget as today
   (~100 ms) regardless of total file size. Indexing the rest happens
   in the background.
4. **Jump-to-end works before indexing finishes.** A user pressing
   `End` (or scrolling to the bottom) on a 10 GB file does not wait
   for a full sequential scan — see §5.3.
5. **Filter and search degrade gracefully.** They run as background
   passes that stream results in. The UI shows progress. Cancelling a
   pass is immediate.
6. **Indexes are reusable.** Re-opening the same file (when neither
   size nor mtime has changed) skips indexing entirely by reading a
   cached index from disk.
7. **No regressions for small files.** Files under a configurable
   threshold (default 4 MB) keep using the existing whole-document
   path — the old behaviour is the fast path.
8. **No new runtime dependencies in WASM.** The byte-level work moves
   from the WASM module to Node.js (extension host) where streaming
   file I/O is native; WASM continues to do ANSI parsing and regex
   matching on byte slices it is handed.

### Non-Goals

- **Editing.** The viewer is read-only. We don't need to maintain
  the index across user edits to the file in VSCode.
- **Tail follow / live append.** Useful, but a separate feature
  (RFC future). The architecture below accommodates it cleanly
  (append-only re-indexing past the previous EOF), but the user's
  ask is "open big files", not "tail -f".
- **Full-text search index.** No precomputed substring index. Search
  is a linear scan over a memory-mapped file with regex; 10 GB at
  ~2 GB/s of mmap'd regex throughput is ~5 s — acceptable, with
  progress.
- **Network / remote-FS support beyond what `fs.read` already does.**
  If `vscode.workspace.fs` returns a stream we can read (local,
  WSL, Codespaces, SSH-FS), it works. Pure HTTP custom resolvers are
  out of scope for v1.
- **Replacing the existing custom editor with a binary editor.** We
  keep the `CustomTextEditorProvider` registration so the file-open
  flow is unchanged. We do, however, **bypass `TextDocument.getText()`**
  for files above the small-file threshold and read directly from disk
  via `fsPath` — see §6.1 for the constraint this places on the
  document scheme.

## 3. The User's Proposal, and Why We Should Do Better

The user proposed:

> 1. Scan the file once, build an index of byte offsets for every line.
> 2. Load only the visible area + a few extra lines into memory for
>    rendering.

This is the right shape. Two refinements turn it from "works for 100
MB" into "works for 10 GB":

### 3.1 A dense per-line index is itself huge

A 10 GB log with average line length ~80 bytes has ~125 M lines. A
`Vec<u64>` of byte offsets for every line is **1 GB**. We just moved
the problem instead of solving it.

**Refinement: sparse anchors.** Store one byte offset every `N` lines
(e.g. `N = 1024`). Memory for the index becomes 125 M / 1024 × 8 B =
~1 MB. To find line `k`:

1. Jump to anchor `floor(k / N)`.
2. Scan at most `N - 1` newlines forward from that offset.

Scanning 1 KB to locate a line inside a window is free; SIMD newline
search (e.g. `memchr`) does ~10 GB/s. The cost of going from "a line
number" to "the bytes of that line" stays sub-millisecond at any
practical file size.

There is a real tradeoff here: a denser index (smaller `N`) speeds
up `goto-line` but wastes RAM; a sparser index does the opposite.
Default `N = 1024` keeps the index under ~10 MB even for 100 GB
files. Configurable via `logViewer.indexAnchorStride`.

### 3.2 Indexing must not block the first paint

A naive "scan the whole file, then paint" loses the time-to-first-byte
property the user already enjoys. Indexing is sequential disk I/O, so
on cold cache 10 GB takes 5–30 s depending on the disk.

**Refinement: progressive indexing.** A worker thread (Node
`worker_threads`) walks the file from offset 0 forward, emitting
anchors as it goes. The host paints the head of the file from the
*partial* index immediately. The webview receives anchor batches and
can scroll into already-indexed territory at any time. A
"jump-to-end" gesture on a still-indexing file gets handled by §5.3.

### 3.3 The line-data format also has to change

Even with windowed rendering, we should not send "all parsed lines"
as a JSON array. The current `ParsedLines = { html: string[]; text:
string[] }` is encoded once for the whole file in
[editorProvider.ts:142](extensions/log-viewer/src/editorProvider.ts#L142).
For a windowed viewer we send only `{ start, lines: LineRecord[] }`
where `LineRecord` carries the rendered HTML for one line.

### 3.4 Don't pay to index twice

The most common case for "large log file" is "I'm investigating a
specific incident, I'll re-open this file ten times today". An index
that lives only in memory for the lifetime of the editor is wasted
work on every reopen.

**Refinement: persistent index cache.** Cache to
`<globalStorage>/index/<sha1(fsPath)>-<size>-<mtime>.idx`. Cache
invalidation is exact (size + mtime change ⇒ regenerate) and the
cache file is small (1 MB per 10 GB of source). Eviction is LRU
capped at 100 MB.

### 3.5 Search and filter need a streaming model

The current `match_filters` and `search` walk every line in WASM
([lib.rs:74-110](crates/log-parser/src/lib.rs#L74-L110)). For a
windowed viewer that doesn't have all lines in memory, this collapses
— we'd have to read the whole file into WASM just to evaluate a
filter. **Refinement**: filter and search become host-side streaming
passes (see §7).

The combined design — sparse anchored index + progressive worker +
persistent cache + windowed render + streaming filter/search — is
what this RFC proposes.

## 4. High-Level Architecture

```
┌────────────────────────────────────────────────────────────────────────────────┐
│ Extension host (Node)                                                          │
│                                                                                │
│  ┌─────────────────────┐     ┌──────────────────────┐                          │
│  │ LogEditorProvider   │     │ Indexer worker       │  worker_threads          │
│  │  - opens .log file  │ ──► │  - mmap or read 4 MB │                          │
│  │  - manages session  │     │    chunks from disk  │                          │
│  │  - serves windows   │ ◄── │  - emits anchors     │                          │
│  │  - runs queries     │     │  - reports progress  │                          │
│  └─────────────────────┘     └──────────────────────┘                          │
│           │   ▲                                                                │
│  windowReq│   │ window {start, lineRecords[]}                                  │
│           ▼   │                                                                │
│  ┌──────────────────────┐    ┌──────────────────────┐                          │
│  │ Window cache         │    │ Index cache          │   <globalStorage>/index/ │
│  │  - LRU of rendered   │    │  - load+save *.idx   │                          │
│  │    line ranges       │    │  - hash by path/size │                          │
│  └──────────────────────┘    └──────────────────────┘                          │
│           │                                                                    │
│           │ slice bytes [start..end)  ──►  WASM render_slice(bytes) ──► HTML   │
└───────────┼────────────────────────────────────────────────────────────────────┘
            │   postMessage                                ▲
            ▼                                              │
┌────────────────────────────────────────────────────────────────────────────────┐
│ Webview                                                                        │
│                                                                                │
│  ┌──────────────────────┐    ┌──────────────────────┐                          │
│  │ Virtual scroll       │    │ Window manager       │                          │
│  │  - 1 row × lineCount │ ─► │  - request {start,N} │                          │
│  │  - scrollTop → range │    │  - dedupe/coalesce   │                          │
│  └──────────────────────┘    └──────────────────────┘                          │
│                                                                                │
│  ┌──────────────────────────────────────────────────────────────────────────┐  │
│  │ Match overlays (search hits, filter hits) drawn from sparse result sets │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────────┘
```

Three things move:

1. **WASM stops owning the file.** It owns line-level operations
   (ANSI parse, regex compile, regex match) on byte slices the host
   gives it. There is no `LogIndex::new(text: &str)` for large files.
2. **The host gains a worker.** Indexing runs off the main thread so
   the extension host stays responsive during a 10 GB scan.
3. **The webview scrolls a virtual range.** Today's DOM holds every
   line; tomorrow's holds at most a few hundred.

### 4.1 Engine / adapter split

A second axis of change, orthogonal to large-file support but enabled
by the same refactor: the parsing/indexing/matching logic is carved
out of `crates/log-parser` into a transport-agnostic engine crate
that any consumer — VSCode, a CLI, a future Zed extension, an
out-of-process server — can use without depending on `wasm-bindgen`.

```
crates/log-engine/                     # pure Rust, no wasm-bindgen
  ├── src/
  │   ├── lib.rs                       # public engine API
  │   ├── ansi.rs                      # moved from log-parser
  │   ├── filter.rs                    # moved from log-parser
  │   ├── index.rs                     # NEW: anchor scan, index file format
  │   └── render.rs                    # NEW: byte-slab → LineRecord
  └── tests/

crates/log-parser/                     # thin WASM adapter (existing crate, slimmed)
  └── src/lib.rs                       # wasm-bindgen wrappers around log-engine

crates/log-cli/                        # FUTURE — thin CLI adapter
  └── src/main.rs                      # `logcat`-like binary; phase 7+

crates/log-engine-server/              # FUTURE — thin JSON-RPC-over-stdio binary
  └── src/main.rs                      # for Zed and other out-of-process consumers
```

What lives where:

- **`log-engine`** owns the data structures (`Index`, `LineRecord`,
  `Match`, compiled rules), the on-disk index file format from §5.4,
  the byte-slab → line-record renderer, and the streaming scan
  helpers. No I/O — the engine takes byte slices and returns
  results. (One exception: it owns the index file *format* including
  the encode/decode functions; it does not own the file *I/O* —
  callers do that.)
- **`log-parser`** stays as the WASM adapter. Its surface shrinks to
  the three exports in §7.1 (`render_lines`, `match_lines`,
  `search_lines`), each a `wasm-bindgen` thin wrapper that
  borrow-decodes the JS arrays into `&[u8]` / `&[u32]` and calls
  into `log-engine`.
- **`log-cli`** and **`log-engine-server`** don't exist in v1. They're
  named here so the engine API stays honest about what reuse looks
  like; building either is a follow-up.

Why this matters for the RFC: it keeps the VSCode large-file work
focused (we ship WASM, no per-platform binaries, no IPC), while not
trapping the engine inside `wasm-bindgen` types. The engine API is
designed for the harder of its two consumers (a future native server
that needs `Read + Seek`, threading, async cancellation), and the
WASM adapter takes a constrained subset.

The out-of-process path is sketched in §11.5 and deferred to a
follow-up RFC.

## 5. Indexing

### 5.1 What an "index" is

```ts
// types.ts (new)
export interface LineIndex {
  fileSize: number;              // total bytes
  totalLines: number;            // count once indexing finishes; partial otherwise
  stride: number;                // anchor every `stride` lines (default 1024)
  // Anchors[i] is the byte offset of line `i * stride`.
  // Length grows during indexing; final length = ceil(totalLines / stride).
  anchors: BigUint64Array;
  // Set to true when the worker has reached EOF.
  complete: boolean;
}
```

`BigUint64Array` (not `Uint32Array`) because file offsets exceed
4 GB. `BigInt` cost is paid only when crossing the boundary; lookups
inside the host can `Number(...)` whenever the value fits in a safe
integer.

### 5.2 The indexing algorithm

```
worker:
  fd = open(path, 'r')
  buf = Buffer(4 MB)
  fileOffset = 0
  lineNum    = 0
  carry      = 0           // bytes since last newline that crossed a chunk boundary
  anchors    = [0n]        // line 0 starts at byte 0

  loop:
    n = fd.readSync(buf, 0, buf.length, fileOffset)
    if n == 0: break
    for nlOff in memchr_all(buf, 0..n, b'\n'):
      nextLineStart = fileOffset + nlOff + 1
      lineNum += 1
      if lineNum % stride == 0:
        anchors.push(BigInt(nextLineStart))
        if anchors.length % 4096 == 0:
          postMessage({ type: 'progress', anchors: anchors.slice(lastSent), bytes: nextLineStart })
    fileOffset += n

  postMessage({ type: 'complete', totalLines: lineNum + 1, anchors: tail, fileSize: fileOffset })
```

`memchr_all` is a tight loop using `Buffer.indexOf(0x0a, ...)` — Node's
native impl is SIMD on x86\_64 and ARM64. Empirically this hits
~3 GB/s on a warm SSD, i.e. an order of magnitude faster than disk.
The bottleneck on cold cache is `read()`, not the parse.

We do **not** detect line endings beyond `\n`. Lone `\r` (old Mac)
ends are deliberately unsupported in v1 — vanishingly rare in modern
logs and the cost of supporting them is a per-byte branch in the
hot loop.

### 5.3 Jump-to-end without waiting

A user who hits `End` on a freshly opened 10 GB file should not wait
for a 5 s sequential scan. The trick: the worker can be told to
**bisect to the tail** in parallel with the head-to-tail scan.

When the host receives a "scroll to byte ≥ X" request and the index
is incomplete past X, it sends a `seek-and-anchor` job to the worker:

1. `lseek` to `X`, scan forward to the next `\n` to align.
2. Read 4 MB chunks until `min(X + window, EOF)`, emitting anchors
   with **unknown line numbers** (just byte offsets at every Nth
   newline).
3. Return a *floating index segment*: `{ startByte, anchors[], lines:
   approximate }`.

The webview can render those anchors immediately (it just doesn't know
their absolute line number — it shows them as "near offset 9.8 GB"
until the head-to-tail scan catches up and stitches them).

This is a known pattern from `klogg`, `lnav`, and `glogg`. It is not
free conceptually — the UI has to display "line N (or thereabouts)" —
but it is the only way to give sub-second tail-jump on cold cache.

For v1 we can ship without bisection (acceptable: tail jump on cold
cache shows a progress bar for a few seconds) and add it as phase 4
in §11. The architecture must not preclude it.

### 5.4 Persistent index cache

#### 5.4.1 File format

Binary, little-endian. The header carries enough metadata that a
stale cache file can be rejected without a full read:

```
+-----------------+
| magic "LGIX"    |  4 B
| version u32     |  4 B
| stride u32      |  4 B
| total_lines u64 |  8 B
| file_size u64   |  8 B    — must match source file size at load time
| mtime_ms u64    |  8 B    — must match source mtime at load time
| path_hash [16]B | 16 B    — sha1(absoluteFsPath) prefix; sanity check
| anchor_count u64|  8 B
| anchors[]: u64  |  anchor_count * 8 B
+-----------------+
```

Loading: open, read header, reject on any mismatch (bad magic,
unsupported version, file_size/mtime drift, path-hash mismatch),
mmap or read the anchor array.

#### 5.4.2 Where the index lives — three modes

The right location depends on the user's environment, and no single
default is correct for everyone. The setting
`logViewer.indexLocation` chooses between three modes:

| Mode             | Path of the `.idx` file                                       |
|------------------|---------------------------------------------------------------|
| `globalStorage`  | `<context.globalStorageUri>/index/<key>.idx`  *(default)*     |
| `adjacent`       | `<dirname(log)>/.<basename(log)>.idx`                         |
| `directory`      | `<logViewer.indexDirectory>/<key>.idx`                        |

`<key>` for non-adjacent modes is
`sha1(absoluteFsPath).slice(0, 16) + '-' + size + '-' + mtimeMs`.
Path hash + size + mtime together make the cache invalidation exact:
a file truncated and rewritten with the same mtime has a different
size; a file rotated under the same name lives at a different
inode-but-same-path and the size/mtime difference catches it.

##### `globalStorage` (default)

`<context.globalStorageUri>/index/`. Out of the way, never write-
permission issues, easy to evict centrally. Loses the cache when the
extension is uninstalled. Recommended default — it never breaks and
never surprises the user with files in their log directories.

##### `adjacent` to the log file

`.<basename>.idx` next to the log itself, e.g. `build.log` →
`.build.log.idx`. Pros: the cache "follows" the log if the user
moves the file; deleting the log naturally orphans the index;
trivially discoverable. Cons:

- **Permission failures.** `/var/log/`, read-only mounts, files
  inside Docker volumes, files served by a remote FS without write
  access — all common, all fail. The implementation must catch
  `EACCES` / `EROFS` / `EPERM` on the create attempt and **silently
  fall back to `globalStorage`** for that file, with a one-time info
  toast (rate-limited per session) explaining what happened.
- **Pollution.** Writes a hidden file into someone else's directory.
  `git status` won't show it (leading dot), but `ls -a` will, and
  cloud-sync tools (Dropbox, iCloud, OneDrive) will sync it. We
  document this clearly in the setting description.
- **Filename collisions.** A pre-existing `.foo.log.idx` written by
  some other tool would be loaded and then rejected by header check
  (magic mismatch ⇒ regenerate-and-overwrite). Acceptable risk;
  worth a one-line note in the setting docs.
- **Source file moves.** Renaming `build.log` to `build-2026-05-11.log`
  doesn't move the index — at next open under the new name we
  rebuild. That's fine.

##### Custom `directory`

`logViewer.indexDirectory` (string, absolute or `~`-prefixed path).
Useful for users who want a single shared cache directory across
machines (synced via Syncthing, etc.) or who just don't want the
extension touching `globalStorage`. The directory is created on
first use; if creation fails, falls back to `globalStorage` with the
same one-time toast.

#### 5.4.3 Settings

```jsonc
"logViewer.indexLocation": {
  "type": "string",
  "enum": ["globalStorage", "adjacent", "directory"],
  "default": "globalStorage",
  "description": "Where to store the persistent line-index cache for large log files. 'globalStorage' is the safe default. 'adjacent' writes a hidden .<name>.idx next to each log file (may fail on read-only locations and is visible to cloud-sync tools). 'directory' uses a custom path set by logViewer.indexDirectory."
},
"logViewer.indexDirectory": {
  "type": "string",
  "default": "",
  "description": "Custom directory for index cache files. Used only when logViewer.indexLocation = 'directory'. Supports a leading '~/'. Created on first use."
}
```

Per-workspace overrides Just Work via the standard VSCode settings
mechanism — a workspace can pin `adjacent` for a project where the
log directory is known to be writable, while the user's global
default stays `globalStorage`.

#### 5.4.4 Eviction

Eviction applies only to the `globalStorage` and `directory` modes —
**not `adjacent`**, because adjacent files are co-located with their
source and there's no "directory we own" to budget. Removing a
random user-adjacent `.log.idx` would surprise the user.

For the centralised modes: when the index directory exceeds 100 MB
(`logViewer.indexCacheBudgetMB`, default 100), drop the oldest-`atime`
files until under the budget. Run on activation, not on every open.

A `Log Viewer: Clear Index Cache` command clears all three locations
the extension might own (current `globalStorage`, current `directory`,
and any adjacent `.idx` files written during this session — tracked in
memory). It does **not** scan the filesystem to delete adjacent
files it didn't create; doing so would risk hitting unrelated
`*.idx` files.

#### 5.4.5 What we don't cache

We **do not** cache rendered HTML — line content can change palette
when the user edits ANSI rendering settings, and HTML is cheap to
regenerate from raw bytes.

## 6. Reading file bytes

### 6.1 Why we bypass `TextDocument`

`TextDocument.getText()` returns the entire file as a JS string,
materialised in V8's heap. That cap (around ~512 MB string size,
realistically much less before things get unhappy) is the underlying
reason today's viewer truncates. There is no streaming read on
`TextDocument`.

For files above the small-file threshold (default 4 MB), the host
**reads from disk directly** using `fs.openSync(uri.fsPath, 'r')` and
`fs.read` for windowed slices. Two consequences:

1. The document URI must be `file://`. Untitled, virtual, and remote
   URIs that have no `fsPath` fall back to the small-file path (which
   loads via `getText()`). For files that have neither — e.g. a 1 GB
   stream from a custom virtual FS — we surface a single-message
   webview saying "this scheme isn't supported for files larger than
   N MB".
2. Edits made in another editor *do not* reflect in the log viewer
   automatically (since we read the on-disk version). That is the
   right behaviour — log files are produced by other processes, not
   typed into VSCode — but we should listen for `fs.watch` mtime
   changes and offer a "reload" affordance. Shipping `fs.watch`
   integration in v1 is optional; for v1 we re-read on the user
   pressing a "Reload" button.

### 6.2 Slicing a window

To render lines `[start, end)`:

1. `anchorIdx = floor(start / stride)`
2. `byteStart = anchors[anchorIdx]`
3. `byteEnd   = end < totalLines ? anchors[ceil(end / stride)] : fileSize`
4. `fs.read(fd, buf, 0, byteEnd - byteStart, byteStart)`
5. Walk `\n`s in the buffer to recover the per-line byte slices for
   exactly `[start, end)`.
6. Pass each line's slice (a `Uint8Array` view, no copy) to WASM
   `render_slice` for ANSI → HTML conversion.

A "window" of 200 lines × ~120 B = ~24 KB. Even with a stride of
1024 (so we read up to ~120 KB to cover a 200-line window), this is
a single sub-millisecond pread.

The host keeps a tiny **window cache** (LRU, 16 entries × 200 lines)
so a short up-and-down scroll doesn't re-pread.

### 6.3 mmap?

`fs.read` is fine for v1. mmap (via `mmap-io`, native) would let the
worker scan without explicit chunk reads and remove the carry-buffer
logic, at the cost of a native dep that has to ship per-platform
binaries. Defer.

## 7. Rendering, filtering, searching

### 7.1 Render path (WASM)

WASM exports change shape. Today:

```rust
// crates/log-parser/src/lib.rs (current)
LogIndex::new(text: &str) -> LogIndex
LogIndex::all_lines_json() -> String
LogIndex::render_range(start, end) -> String
```

New:

```rust
// Stateless. The host owns file bytes and their indexing.
#[wasm_bindgen]
pub fn render_lines(bytes: &[u8], line_breaks: &[u32]) -> String {
    // bytes: a contiguous slab covering one or more whole lines.
    // line_breaks: positions of '\n' inside `bytes`, plus a sentinel = bytes.len().
    // Returns a JSON array of { html, text } records, one per line.
}

#[wasm_bindgen]
pub fn match_lines(bytes: &[u8], line_breaks: &[u32], rules_json: &str) -> Vec<u8> {
    // Same shape: per-line first-matching-rule index (1-based).
}

#[wasm_bindgen]
pub fn search_lines(bytes: &[u8], line_breaks: &[u32], query: &str, regex: bool, case_sensitive: bool) -> Vec<u32> {
    // Returns local indices (within the slab) of matching lines.
}
```

These three replace the stateful `LogIndex` for the large-file path.
The small-file path can keep using `LogIndex` unchanged — see §8 for
the dispatch.

This shifts ownership cleanly: **the host knows about the file, WASM
knows about lines.** The byte slabs WASM receives never need to live
longer than one call.

### 7.2 Streaming filter

The host runs filter passes on the indexer worker. Pseudo:

```ts
async function* runFilter(rules: FilterRule[]) {
  const compiled = wasm.compileRules(JSON.stringify(rules));
  let off = 0;
  while (off < fileSize) {
    const n = await readChunk(off, CHUNK_SIZE);
    const lineBreaks = findNewlines(buf, 0, n);
    const hits = wasm.matchLinesCompiled(compiled, buf.subarray(0, n), lineBreaks);
    // hits is per-line; translate each local hit to a global line number via index.
    yield translateHits(off, lineBreaks, hits);
    off += n;
  }
}
```

The webview receives `{ type: 'filterPartial', from, to, hits[] }`
batches as they arrive and updates the gutter / overlay
incrementally. A new filter pass cancels the in-flight one (the worker
checks an `AbortSignal` between chunks).

Filter results are stored as a `Uint32Array` of matching line
numbers, plus a `Uint8Array` of rule indices keyed by position in the
result array. Memory: 1 M hits × (4 + 1) B = 5 MB. We cap at 1 M
hits and surface "more matches than shown" in the gutter.

For "only-matching" filter mode, the webview's virtual scroll uses
the *result* array as its row source (`row i → results[i]`) instead
of the raw line index.

### 7.3 Streaming search

Identical to filter but with a single regex and no "first matching
rule" tagging. Results are Just Line Numbers. Reported with
`{ found, scanned, fileSize }` so the UI can show a percentage.

### 7.4 Cancellation & responsiveness

Indexing, filtering, and searching are all worker tasks with
`AbortController`-style cancellation. The worker checks the signal
between chunks (every 4 MB ⇒ at most a few ms of latency to cancel).
The host coalesces `progress` postMessages to ≤ 30 Hz to avoid
flooding the webview.

## 8. Small-file fast path

Files smaller than `logViewer.streamingThresholdBytes` (default 4
MB) skip the entire pipeline and use today's path:

- `document.getText()` →
- `new wasm.LogIndex(text)` →
- single bulk `allLinesJson()` →
- non-virtualised webview render.

The dispatch lives in `LogEditorProvider.resolveCustomTextEditor`:

```ts
const stat = await vscode.workspace.fs.stat(document.uri);
const useStreaming =
  stat.size > thresholdBytes &&
  document.uri.scheme === 'file';
if (useStreaming) {
  this.openStreaming(document, webviewPanel);
} else {
  this.openInMemory(document, webviewPanel);  // = current code path
}
```

This means: nothing about today's working flows changes for a 100 KB
log; nothing about it changes for a 200 KB log. The new code only
activates when the file is actually big.

## 9. Webview changes

### 9.1 Virtual scroll

Today's webview lays out all `<div>`s for all lines. New: a single
spacer `<div>` of height `lineCount × lineHeight`, with a positioned
inner container that holds only the visible window.

Two complications worth flagging:

1. **`wordWrap`.** Wrapping turns "fixed line height" into "variable
   line height", which breaks the `top = lineNum × lineHeight`
   formula. Two options:
   - **A. Disable wrap in streaming mode for v1.** Acceptable: `less`
     behaves the same, and wrapping a 100 k-character line is a
     pathological case anyway.
   - **B. Per-window measured layout.** The webview measures the
     wrapped height of each rendered window and records it; jumps to
     unmeasured regions land approximately and re-anchor on render.
     The "approximate" scrollbar is a user-visible compromise.

   v1 chooses A; B is a phase-7 polish.

2. **Scrollbar accuracy.** With variable wraps it's approximate; with
   fixed lines it's exact.

### 9.2 Message protocol additions

```ts
// host → webview
| { type: 'streamInit'; totalLines: number; stride: number; fileSize: number;
    indexProgress: { lines: number; bytes: number; complete: boolean }; ... }
| { type: 'window'; start: number; lines: LineRecord[] }
| { type: 'indexProgress'; lines: number; bytes: number; complete: boolean }
| { type: 'filterProgress'; scannedBytes: number; hitCount: number }
| { type: 'filterDone'; totalHits: number }

// webview → host
| { type: 'requestWindow'; start: number; end: number }
| { type: 'cancelFilter' }
| { type: 'reload' }
```

Existing messages (`init`, `update`, `setState`, etc.) remain for the
small-file path.

### 9.3 Filter UI

Filter and search panels work the same — they show hit counts
incrementally, with a "scanning…" indicator until done. A new
abort-on-input behaviour: typing into the search box while a search
is running cancels the previous one and starts a new one when the
input settles for 200 ms.

## 10. Open Questions

1. **Encoding.** v1 assumes UTF-8 (the dominant log encoding). Logs
   with mixed encodings will display replacement characters in the
   small-file path today, and the same in the streaming path
   tomorrow. Detecting BOM or shift-JIS is a separate concern.
2. **Memory budget exposure.** Should `logViewer.maxIndexAnchors` be
   exposed, or should we treat it as an implementation constant? Lean
   toward implementation constant (only configurable for power
   users via `logViewer.indexAnchorStride`).
3. **Worker per-document vs pool.** v1: one worker per opened
   streaming document. Cheap because workers are cheap; simple
   lifetime. Consider a pool if users routinely open ten 1 GB files
   simultaneously.
4. **mmap for the worker.** Real speed win for the indexing scan, but
   a native dep. Defer to a follow-up RFC if profiling shows reads
   dominate.
5. **`fs.watch` integration.** Manual reload in v1; auto-detect-and-
   prompt in v2.
6. **Tail-follow.** Out of scope, but the design supports it: extend
   the index past the previous EOF when watch fires, append anchors
   only.
7. **Index location default.** Settled on `globalStorage` per §5.4.2 —
   it never fails on read-only paths and never pollutes the user's
   directories. `adjacent` and a custom `directory` are opt-in for
   users who prefer co-location or a shared cache. Open question:
   should the first-open of a large file from a writable directory
   prompt "store index alongside this file?" (one-time, dismissable)
   to surface the choice? Probably no for v1 — too noisy.

## 11. Phased Plan

| Phase | Deliverable | Done when |
|-------|-------------|-----------|
| 0 | This RFC accepted | Reviewer sign-off; license/threading questions answered |
| 1a | **`crates/log-engine` carved out of `log-parser`** (§4.1) | New crate compiles standalone (no `wasm-bindgen` dep); existing tests for ANSI/filter move with the code and still pass; `log-parser` re-exports through it |
| 1b | Stateless render/match/search API on the engine | `log_engine::render_lines`, `match_lines`, `search_lines` operate on byte slabs + line-break arrays; `log-parser` exposes them via `wasm-bindgen`; crate tests cover synthetic slabs |
| 2 | Indexer worker | Standalone worker thread indexes `data/fixtures/big.log` (synthetic 1 GB) in <2 s on warm cache; emits anchor batches; cancellable; tested in `extensions/log-viewer/src/indexer.test.ts` |
| 3 | Persistent index cache | Re-opening the same fixture skips indexing; cache eviction respects 100 MB budget; tested with hash collisions; index file format defined in `log-engine::index` so future consumers share it byte-for-byte |
| 4 | Streaming render path + virtual-scroll webview | Open a 1 GB log, scroll to end, jump to line N — all sub-second after first paint; word-wrap disabled in this mode |
| 5 | Streaming filter + search | Filter hits stream in for the same 1 GB file; cancellation interrupts within ~50 ms |
| 6 | Bisection tail-jump (§5.3) | Pressing `End` on a cold-cache 10 GB file paints the tail in <500 ms |
| 7 | Polish | Word-wrap in streaming mode (per-window measured); `fs.watch` reload prompt; mmap exploration |

Phases 1a–5 are the v1 cut. Phases 6–7 are post-launch. Phase 1a
(the engine carve-out) is non-negotiable in the v1 cut: it is what
keeps the engine API honest and unblocks any future out-of-process
consumer (§11.5) without a second refactor.

### 11.5 Future: out-of-process server (deferred to RFC 008)

`crates/log-engine` is designed so a thin `crates/log-engine-server`
binary can wrap it for editors that prefer out-of-process — primarily
**Zed** (no WASM extension model today, native `process::Command`
plugins instead), but also CLI usage and any Rust-friendly editor
that is not VSCode.

The shape, sketched here for the engine API to be designed against
but **not built in this RFC**:

- **Transport**: JSON-RPC 2.0 over stdio. *Not* LSP — LSP's
  vocabulary (`textDocument/*`) doesn't fit "give me bytes
  N..M of file F" or "stream filter hits from offset X". We use
  LSP's wire framing (`Content-Length:` headers) for tooling
  familiarity and nothing else.
- **Methods**: `session/open(path)`, `session/window(id, start, n)`,
  `session/filter(id, rules)` (streams `filter/partial`
  notifications), `session/search(id, q)`, `session/cancel(token)`,
  `session/close(id)`. Mirrors the in-process host API one-to-one.
- **Lifecycle**: editor spawns one server per workspace; server
  multiplexes multiple open files; crashes are caught by the editor
  and the server is respawned. The persistent index cache (§5.4) is
  shared between in-process and out-of-process consumers — the file
  format is defined in `log-engine` precisely so a `.idx` written by
  the VSCode extension can be loaded by the server, and vice versa.
- **Distribution**: per-platform binaries (darwin-arm64/x64,
  linux-arm64/x64, win-arm64/x64) published as a separate cargo
  binary release; editor extensions pull the matching one or accept
  a `serverPath` setting.

Why **not** in this RFC:

- VSCode-targeted users get nothing from the binary — WASM works
  there, the marketplace already accepts our extension, and per-
  platform binaries would require splitting one VSIX into six.
- Real cost (lifecycle, IPC, packaging) for no incremental user
  value until a second editor consumer exists.
- Engine API can be designed to support this without building it.

The trigger to write RFC 008 is concrete: somebody (us or a
contributor) wants to ship a Zed plugin or a `logcat` CLI. Until
then, `log-engine` sitting in the workspace is sufficient — its
mere existence demonstrates the engine is reusable and de-risks the
follow-up.

## 12. Risks

- **Disk-IO bound progress.** A network-attached file (SMB, NFS) at
  50 MB/s makes a 10 GB index a 3-minute scan. The progress bar must
  make this visible; we should not hide it. Bisection (phase 6)
  partially mitigates by giving the user something to look at within
  seconds of opening.
- **Anchor drift on rewritten files.** If a process truncates and
  rewrites the file in place between an open and a scroll, anchors
  point at garbage. Detect via a periodic `fstat` size/mtime check
  on the open fd; on mismatch, drop the index and reload.
- **Webview message storm.** A naive indexer that posts every anchor
  individually can saturate the webview message queue. The 30 Hz
  coalescing in §7.4 is mandatory, not optional.
- **Sandbox constraints in remote development.** `vscode.workspace.fs`
  is the supported abstraction; `fs.openSync` only works for `file://`.
  This is documented in §6.1 and gracefully degraded.

## 13. Summary

Replace "load the whole file into V8 then into WASM" with **"index
once on a worker, render windows on demand"**. The user's two-step
sketch is right; refining the index to sparse anchors (§3.1),
running it progressively on a worker (§3.2), persisting it across
sessions (§3.4), and turning filter / search into streaming passes
(§3.5) are what turn it from "100 MB capable" into "10 GB capable".
Small files keep their fast path unchanged. WASM stops owning the
file and starts being a stateless line-renderer; the host owns disk
I/O, indexing, and windowing. The truncation badge goes away.

Alongside the user-visible work, the parsing/indexing/matching logic
is carved out of `crates/log-parser` into a new transport-agnostic
`crates/log-engine` crate (§4.1). The VSCode extension stays on the
WASM adapter — no per-platform binary, no IPC, web-host support
preserved. A future `crates/log-engine-server` (§11.5, deferred to
RFC 008) can wrap the same engine for Zed and CLI consumers without
re-doing the work, sharing the on-disk index format byte-for-byte.
