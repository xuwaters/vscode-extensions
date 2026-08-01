# RFC 009: Markdown Live Preview, Rewritten on a Rust/WASM Engine

**Status**: Implemented (Phases 1–3; Phase 4 task-checkbox toggle shipped as opt-in)
**Date**: 2026-08-01
**Extension name**: `wx-vsce-markdown-preview-ultra`
**Rust crate**: `crates/markdown-engine` (new)
**Supersedes**: [RFC 001](../_archive/001-markdown-preview-ultra-editor.md) — the Obsidian-style *editable* live preview. This RFC drops in-place editing entirely; the preview is a read-only renderer.
**Reference**: `temp/vscode-markdown-preview-enhanced` (Markdown Preview Enhanced 0.8.30, backed by `crossnote@0.9.31`)
**Affected components**:
  - `extensions/markdown-preview-ultra` (host + webview, rewrite)
  - `crates/markdown-engine` (new)

---

## 1. Motivation

The current extension (v0.3.1, ~1,050 LOC) works, but it is a first draft with structural limits:

1. **Everything renders in the webview.** The host ships the full document text on every debounced change and the webview rebuilds the entire DOM via `innerHTML` ([index.ts:49](extensions/markdown-preview-ultra/webview/index.ts#L49)). Every keystroke re-parses the whole document in the UI thread, discards all rendered state (mermaid SVGs survive only via a string cache, images may re-fetch, KaTeX re-typesets), and causes visible flicker plus scroll jitter on large documents.
2. **Scroll sync is one-way.** Editor → preview works via `data-line` anchors; scrolling the preview does nothing to the editor, and there is no way to jump from a rendered block back to its source line.
3. **The feature set is a fraction of what a daily-driver preview needs.** Compared to Markdown Preview Enhanced (MPE) we lack: a TOC, heading anchors, GitHub-style alerts/admonitions, emoji shortcodes, preview themes beyond "inherit VSCode colors", copy-code buttons, an image lightbox, zoom, and two-way navigation.
4. **The parsing stack is a pile of JS plugins.** markdown-it + five plugins + js-yaml, each with its own quirks, all bundled into the webview. There is no single place where "the document model" lives, which is exactly what made RFC 001's incremental-rendering ambitions hard to land.

MPE itself demonstrates both the target feature set and the trap to avoid. It wraps `crossnote` — 17.7 MB unpacked, ~85 runtime dependencies including React 18, **Monaco**, jQuery, puppeteer-core, sharp, jsdom, and a QuickJS sandbox — into a 4.2 MB webview bundle, with an untyped `{command, args}` message protocol that has accumulated a string of real CVEs (blind command dispatch, `eval` in diagram parsers, RCE via `config.js`). We want its *preview* feature set with roughly two orders of magnitude less machinery.

This repo already has the right pattern for that: nine extensions ship a Rust crate compiled to WASM with `wasm-pack --target nodejs`, loaded in the extension host, exposing a JSON-in/JSON-out API (see RFC 007 §4.1's engine/adapter split). This RFC applies that pattern to markdown: **parsing, transformation, sanitization, TOC extraction, and incremental block diffing move into a Rust crate; the webview becomes a thin patch-applier plus three JS post-processors (KaTeX, mermaid, syntax highlighting) that are inherently browser libraries.**

## 2. Goals and Non-Goals

### Goals

1. **Preview only.** The extension never edits the markdown document. (One narrowly-scoped, opt-in exception is discussed in §7.5: clicking a task-list checkbox.)
2. **MPE-grade rendering** for the features that matter day-to-day: KaTeX math, mermaid diagrams, syntax-highlighted code fences, GitHub-style alerts, emoji, footnotes, task lists, tables, front matter, TOC, heading anchors, dark/light theming.
3. **Rust engine as a WASM module** in `crates/markdown-engine`, following the repo's established build pipeline (`wasm-pack --target nodejs` → `extensions/markdown-preview-ultra/wasm/`, runtime `require` from the host).
4. **Incremental updates.** Keystroke-to-paint should touch only the blocks that changed. No full-DOM replacement, no flicker, no lost diagram/image state.
5. **Three view modes with frictionless switching**: Edit (editor only), Split (editor + preview side-by-side), Preview (preview occupies the editor's column). One command cycles them; a status-bar item shows and switches the mode.
6. **Two-way scroll sync** plus click-to-jump navigation in both directions.
7. **Security by construction**: HTML sanitized in Rust before it ever reaches the webview, strict CSP, a typed and validated message protocol.
8. **A small, curated settings surface** (~17 settings, not MPE's 74).

### Non-Goals

- **In-place / Obsidian-style editing** (RFC 001's core idea). Explicitly abandoned.
- **Export** of any kind: no PDF, HTML, ebook, pandoc, prince, puppeteer.
- **Code chunk execution** (`{cmd=true}`). A preview must never run document-controlled subprocesses.
- **Presentation mode** (reveal.js). Possible future RFC; nothing in this design blocks it.
- **Notebook features**: backlinks, graph view, `#tag` index, full-text search, wiki-link *resolution machinery* (a limited rendering-only wikilink option is discussed in §12).
- **External renderers and services**: PlantUML jar/server, D2 binary, Kroki, WebSequenceDiagrams, TikZ, vega/vega-lite, image-upload services. (Kroki as an opt-in is an open question, §12.)
- **MathJax.** KaTeX only, `$…$` / `$$…$$` delimiters only.
- **Alternate parser backends** (pandoc, etc.). One parser, one behavior.
- **Web extension (vscode.dev).** The `--target nodejs` WASM build requires a Node extension host. Accepted trade-off; every other extension in this repo has the same constraint.

## 3. What We Take from MPE — and What We Deliberately Don't

| MPE feature | Verdict | Notes |
|---|---|---|
| KaTeX math (`$`, `$$`) | **Adopt** | Engine marks math spans; webview typesets with KaTeX + cache (§7.2) |
| MathJax, configurable delimiters, codecogs | Drop | KaTeX-only keeps one rendering path |
| Mermaid | **Adopt** | Client-side, lazy-loaded, cached; theme follows preview theme (§7.2) |
| PlantUML / Kroki / vega / wavedrom / graphviz / tikz / d2 | Drop (Kroki: §12) | All need binaries, servers, or megabytes of webview JS |
| Code fence syntax highlighting (Prism, 25 themes) | **Adopt (highlight.js)** | Already in place; theme pairs with preview theme. Engine choice revisited in §12 |
| Two-way scroll sync with line→offset map + interpolation | **Adopt, improved** | comrak `sourcepos` gives us the map for free (§6.2) |
| TOC sidebar + `[TOC]` marker + heading anchors | **Adopt** | Heading tree extracted in Rust with GitHub-compatible slugs (§5.6) |
| Admonitions (`!!! note`) / GitHub callouts (`> [!NOTE]`) | **Adopt GitHub alerts**; `!!!` syntax is §12 | comrak supports alerts natively |
| Emoji shortcodes (`:smile:`) | **Adopt** | comrak `shortcodes` extension, rendered as Unicode (no twemoji assets) |
| Footnotes, task lists, tables, strikethrough, front matter | **Adopt** | All native comrak |
| Front matter as table/code-block | Adopt as the existing styled card, plus `hidden` |
| Task-list checkbox click → source toggle | Opt-in (§7.5) | The only write path, disabled by default |
| Image lightbox, zoom in/out/reset | **Adopt** | Small webview-side features, no dependencies |
| Preview themes (17) with light/dark pairing | **Adopt subset** | `auto` (VSCode variables, default) + `github-light`/`github-dark`; more in §12 |
| Custom CSS (`style.less`, `.crossnote` layering) | **Adopt simplified** | A `customCss: string[]` setting of workspace-relative plain-CSS files |
| `@import` / `![[transclusion]]` | Future (§12) | Needs file-graph watching; keep the engine hook in mind, don't build now |
| Wiki-link syntax rendering | Optional, default off (§12) | comrak has it; resolution stays trivial (relative path) |
| Code chunks, export, presentation, backlinks, graph, image upload, `#tag` | Drop | Out of scope per §2 |
| Single-preview-follows-editor + lock | **Keep** | Already implemented and good ([previewManager.ts:41](extensions/markdown-preview-ultra/src/previewManager.ts#L41)) |
| Multiple previews / "Previews Only" editor association | Drop | MPE's global `editorAssociations` mutation is exactly the kind of side effect we avoid |

## 4. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│ Extension Host (Node)                                               │
│                                                                     │
│  TextDocument ──onDidChangeTextDocument──▶ PreviewManager           │
│                                              │ debounce (~150ms)    │
│                                              ▼                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ crates/markdown-engine (WASM, wasm-pack --target nodejs)     │   │
│  │                                                              │   │
│  │  parse (comrak, sourcepos) ─▶ transform (mermaid fences,     │   │
│  │  math spans, alerts, URL rewrite) ─▶ sanitize (user HTML)    │   │
│  │  ─▶ per-block render + hash ─▶ diff vs previous render       │   │
│  │                                                              │   │
│  │  returns JSON: { seq, patches[], toc[], frontmatter }        │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                              │                      │
│                             postMessage (typed, validated)          │
│                                              ▼                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ Webview (thin)                                               │   │
│  │   patch applier ── swaps only changed top-level blocks       │   │
│  │   post-processors (changed blocks only):                     │   │
│  │     KaTeX (math spans, cached)                               │   │
│  │     mermaid (lazy chunk, SVG cache)                          │   │
│  │     highlight.js (code fences)                               │   │
│  │   UI chrome: TOC sidebar, copy-code, lightbox, zoom          │   │
│  │   scroll map (data-sourcepos → offsets) + 2-way sync         │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

The inversion vs today: **markdown → HTML happens in the host (WASM), not the webview.** The webview receives block-level patches, so its job shrinks to DOM surgery plus the three renderers that only exist as browser libraries. Compared with MPE (which also renders host-side but then replaces the entire preview DOM with the new HTML string on every update), the patch protocol is what buys flicker-free typing.

## 5. The Rust Engine (`crates/markdown-engine`)

A single crate with a `wasm_api` module, following the `proto3-analyzer` shape (bindings isolated in [wasm_api.rs](crates/proto3-analyzer/src/wasm_api.rs), pure-Rust core testable without WASM). No separate adapter crate: unlike `log-engine`, there is no second consumer on the horizon, and the split can be introduced later without breaking the extension-facing API.

### 5.1 Parser: comrak

[comrak](https://crates.io/crates/comrak) is the GitHub-flavored CommonMark implementation in Rust. It covers, natively, almost everything we currently assemble from five markdown-it plugins plus most of the MPE features we want:

- **Extensions**: tables, strikethrough, autolink, task lists, footnotes, front matter (`---` delimiter), math (`$`/`$$` dollar math), **alerts** (`> [!NOTE]` … GitHub callouts), header IDs with GitHub-compatible slugs, emoji shortcodes, description lists, multiline block quotes, wikilinks (off by default for us).
- **`sourcepos`**: emits `data-sourcepos="12:1-14:8"` on every element it generates — the scroll-sync map that MPE builds with a custom markdown-it plugin comes free.
- **Options mapped from settings**: `hardbreaks` (⇐ `breaks`), autolink (⇐ `linkify`), `smart` punctuation (⇐ `typographer`), raw-HTML passthrough (⇐ `html.enabled`, always routed through the sanitizer, §5.4).

CommonMark-vs-markdown-it fidelity differences exist (list-item edge cases, HTML block boundary rules). §11 pins them down with snapshot tests; the acceptance bar is "renders our own repo's markdown and MPE's test fixtures correctly", not bug-for-bug markdown-it compatibility.

### 5.2 Transform pass

After parsing, one AST walk performs:

1. **Mermaid fences** → replaced with an engine-generated container:
   `<div class="mermaid-container" data-sourcepos="…" data-mermaid-source="<urlencoded>">`. The webview owns actual SVG rendering (§7.2).
2. **Math spans** — comrak's math extension already emits `<span data-math-style="inline|display">` holding raw TeX; nothing to do beyond keeping the attribute through sanitization.
3. **Relative URL rewriting** — image `src` and link `href` classification (external / anchor / relative). Relative image sources are joined against the `baseHref` render option (the webview-resource URI of the document's directory) exactly as [markdown.ts:146](extensions/markdown-preview-ultra/webview/markdown.ts#L146) does in JS today. Relative link hrefs are left as-is and handled by the click→host `openLink` path.
4. **`[TOC]` marker** — a paragraph consisting solely of `[TOC]` becomes `<nav class="inline-toc">` rendered from the heading tree.

### 5.3 Per-block rendering and diffing

The unit of update is a **top-level AST node** (paragraph, heading, list, fence, table, blockquote, html block…). For each render:

1. Render every top-level node to an HTML string individually. Reference resolution (link definitions, footnote references) happens at parse time on the full document, so per-node rendering stays correct; footnote *definitions* render as one synthetic trailing block.
2. Hash each block's HTML (FxHash/xxhash).
3. Diff the new hash sequence against the previous render's (kept in the per-document session): Myers/patience diff over the hash sequences — documents are a few hundred blocks, so this is microseconds — emitting a compact patch script:

```jsonc
// RenderResult (engine → host → webview, JSON)
{
  "seq": 42,                      // monotonic per session; webview drops stale seqs
  "reset": false,                 // true → treat as full replace (first render, options changed)
  "patches": [
    { "op": "keep",    "count": 17 },
    { "op": "replace", "count": 2, "html": ["<p data-sourcepos=…>", "<ul …>"] },
    { "op": "insert",  "html": ["<h2 …>"] },
    { "op": "delete",  "count": 1 }
  ],
  "toc": [ { "level": 2, "text": "Install", "slug": "install", "line": 10, "children": [] } ],
  "frontmatter": { "raw": "title: …", "data": { "title": "…" } },   // or null
  "stats": { "parseMs": 3, "blockCount": 214 }
}
```

A typical keystroke produces `keep / replace(1) / keep` — one block crosses the wire and one DOM subtree is swapped. Hash-sequence diffing (rather than prefix/suffix trimming) matters because a single edit can legitimately touch two distant blocks (e.g. adding a footnote reference also changes the trailing footnotes block).

### 5.4 Sanitization

Raw HTML from the *document* is sanitized in Rust with [ammonia](https://crates.io/crates/ammonia) during the AST walk — `HtmlBlock` / `HtmlInline` node contents pass through an allowlist (structural tags, `details/summary`, `kbd`, media tags with `https:`/`data:`/webview-resource sources; no scripts, no event handlers, no inline styles beyond a safe subset). Engine-*generated* HTML (mermaid containers, math spans, alert boxes) is trusted and bypasses the sanitizer, which is why sanitization must run before, not after, block rendering.

Defense in depth, in order: Rust sanitizer → webview CSP (`default-src 'none'`, nonce'd scripts — kept from the current implementation, [previewManager.ts:408](extensions/markdown-preview-ultra/src/previewManager.ts#L408)) → typed message validation (§6.3). MPE's CVE history (webview → host command dispatch with unvalidated `any[]` args; `eval` in diagram parsers) is the cautionary tale for why the webview is treated as untrusted even though we authored it.

If ammonia's html5ever dependency proves too heavy for the WASM artifact, the fallback is comrak's built-in `escape`/tagfilter modes plus DOMPurify in the webview — noted as a risk (§14), not expected.

### 5.5 Sessions and the WASM API

```rust
// crates/markdown-engine/src/wasm_api.rs
#[wasm_bindgen]
pub struct Session { engine: Engine }   // retains previous block hashes for diffing

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session;
    /// options_json: RenderOptions { base_href, breaks, linkify, typographer,
    ///   html, math, mermaid, alerts, emoji, wikilinks, frontmatter }
    /// Returns RenderResult JSON (§5.3). Changing options forces reset: true.
    pub fn render(&mut self, markdown: &str, options_json: &str) -> String;
}
```

The host keeps one `Session` per previewed document (in practice one or two — single-preview model), dropped when the preview retargets. JSON in/out matches the repo's other WASM surfaces (`AnalyzerBridge` in [analyzer.ts](extensions/dotenv/src/analyzer.ts)); the host bridge hand-declares the module type and degrades gracefully when `wasm/` is missing, printing the standard "run `pnpm run build:wasm`" hint like [wasm.ts](extensions/log-viewer/src/wasm.ts).

### 5.6 TOC and front matter

- Headings are collected during the transform walk into a tree with `level`, rendered `text`, GitHub-compatible `slug` (comrak's header-ID generator, so anchors match GitHub), and `line`. The same tree feeds the sidebar TOC, the inline `[TOC]` block, and heading-anchor links.
- Front matter is extracted by comrak, parsed as YAML in Rust (`serde_yaml`), and returned as structured JSON. The webview renders the existing styled card ([frontmatter.css](extensions/markdown-preview-ultra/webview/styles/frontmatter.css)); `js-yaml` drops out of the webview bundle.

### 5.7 Crate layout and build

```
crates/markdown-engine/
  Cargo.toml          # cdylib + rlib, wasm-bindgen, comrak, ammonia, serde,
                      # serde_yaml; [package.metadata.wasm-pack] wasm-opt = false
  src/lib.rs          # Engine: parse → transform → sanitize → blocks → diff
  src/transform.rs    # mermaid/math/URL/[TOC] AST pass
  src/toc.rs          # heading tree + slugs
  src/diff.rs         # block-hash diff → patch script
  src/sanitize.rs     # ammonia profile
  src/wasm_api.rs     # #[wasm_bindgen] Session
```

Build follows the workspace convention verbatim:

```jsonc
// extensions/markdown-preview-ultra/package.json
"build:wasm": "cd ../../crates/markdown-engine && wasm-pack build --target nodejs --out-dir ../../extensions/markdown-preview-ultra/wasm --out-name markdown_engine",
"package":   "pnpm run build:wasm && pnpm run build && vsce package --no-dependencies --allow-missing-repository",
"vscode:prepublish": "pnpm run build:wasm && tsdown --minify"
```

Artifacts land in `extensions/markdown-preview-ultra/wasm/` (already covered by the root `.gitignore`'s `extensions/*/wasm/`), ship in the VSIX, and are `require`d at first preview open — not at activation — so activation cost stays near zero.

## 6. Extension Host

### 6.1 View modes

Three modes, one state machine, per the user-facing goal "edit / preview / side-by-side without friction":

| Mode | Layout | Entered by |
|---|---|---|
| **Edit** | text editor only | closing/leaving the preview; `setMode('edit')` |
| **Split** | editor + preview beside (preview group locked) | `showPreviewToSide` (`ctrl+k v`), `setMode('split')` |
| **Preview** | preview panel in the editor's own column | `showPreview` (`ctrl+shift+v`), `setMode('preview')` |

New surface on top of the existing commands (which all stay):

- **`markdownPreviewUltra.cycleMode`** — Edit → Split → Preview → Edit. Proposed keybinding `ctrl+k ctrl+m` / `cmd+k cmd+m`, joining the existing `ctrl+k ctrl+v` (focus toggle) and `ctrl+k ctrl+l` (lock) family. (Conflict check is an open question, §12.)
- **`markdownPreviewUltra.switchMode`** — QuickPick of the three modes.
- **Status-bar item** (visible while a markdown editor or the preview is active): `$(eye) Split` etc.; click opens the QuickPick. Backed by a `markdownPreviewUltra.mode` context key for menus.

Mode transitions reuse the existing panel: Split→Preview moves the panel into the source column (`panel.reveal(sourceColumn)`); Preview→Edit hides the panel and focuses the editor; Edit→Split recreates/reveals beside with the group-lock behavior already implemented ([previewManager.ts:202](extensions/markdown-preview-ultra/src/previewManager.ts#L202)). Follow-active-editor and preview-lock semantics are unchanged.

Two lifecycle upgrades:

- **`WebviewPanelSerializer`** so a preview (and its mode) survives window reloads.
- **`enableFindWidget: true`** on the panel — free in-preview search (`cmd+f`).

### 6.2 Two-way scroll sync

Editor → preview (kept, improved): on `onDidChangeTextEditorVisibleRanges`, post `scroll { line, ratio }` where `ratio` is the anchor's fractional position in the viewport (MPE's `topRatio` trick — syncing to ~⅓ from the top tracks the *reading* position, not the window top).

Preview → editor (new): the webview maintains a **line → offset map** built from `data-sourcepos` attributes: query anchors once per patch, drop out-of-order elements, linearly interpolate unmapped lines between anchors (MPE's approach, [§2.7 of its architecture]). On user-initiated preview scroll, binary-search the map at viewport-center, post `revealLine { line }`; host calls `revealRange(…, AtTop|InCenter)`.

Feedback-loop protection: each side stamps the origin of its last programmatic scroll and ignores reciprocal sync messages for ~100 ms (MPE uses a 500 ms guard; ours can be tighter because patches don't move the scroll position).

Click-to-jump: **double-click any block** in the preview → `jumpToLine { line }` → host reveals and focuses the editor at that line (switching Preview → Split if no editor is visible). TOC clicks scroll the preview and, in Split mode, also reveal the editor line.

### 6.3 Message protocol

Typed discriminated unions in `src/messages.ts`, imported by both sides (existing convention, [messages.ts](extensions/markdown-preview-ultra/src/messages.ts)), extended:

```typescript
type HostToWebview =
  | { type: 'update'; seq: number; reset: boolean; patches: Patch[];
      toc: TocEntry[]; frontmatter: Frontmatter | null;
      baseHref: string; settings: PreviewSettings }
  | { type: 'scroll'; line: number; ratio: number }
  | { type: 'theme'; kind: 'light' | 'dark' };

type WebviewToHost =
  | { type: 'ready' }
  | { type: 'revealLine'; line: number }          // preview scrolled (sync)
  | { type: 'jumpToLine'; line: number }          // double-click / TOC in split
  | { type: 'openLink'; href: string }
  | { type: 'toggleTask'; line: number; checked: boolean }  // opt-in, §7.5
  | { type: 'error'; message: string; context: string };
```

Host-side handling validates the shape (a small hand-rolled guard per variant — no `any` dispatch) and, for `toggleTask`, re-verifies the target line actually is a task-list item before editing. `seq` gives the same staleness protection MPE bolts on with per-URI request counters: the webview ignores any `update` whose `seq` is not greater than the last applied one, and the host drops renders that were superseded while awaiting the debounce.

### 6.4 Configuration

Curated surface (existing seven settings plus ten new, all under `markdownPreviewUltra.`):

| Setting | Default | Notes |
|---|---|---|
| `scrollSync` | `true` | Now bidirectional |
| `lockPreviewGroup` | `true` | Unchanged |
| `defaultMode` | `"split"` | Mode used by `cycleMode`/open commands when no preview exists |
| `breaks` / `linkify` / `typographer` | `false` / `true` / `false` | Mapped to comrak options |
| `html.enabled` | `true` | Raw HTML (always sanitized); `false` = escape it |
| `math.enabled` | `true` | KaTeX, `$`/`$$` |
| `mermaid.enabled` | `true` | |
| `mermaid.theme` | `"auto"` | `auto` follows preview theme; or `default`/`dark`/`forest`/`neutral` |
| `alerts.enabled` | `true` | GitHub `> [!NOTE]` callouts |
| `emoji.enabled` | `true` | `:shortcode:` → Unicode |
| `frontmatter.display` | `"card"` | `card` \| `hidden` |
| `theme` | `"auto"` | `auto` (VSCode CSS variables) \| `github-light` \| `github-dark` |
| `customCss` | `[]` | Workspace-relative CSS files appended to the preview (like built-in `markdown.styles`) |
| `toc.visible` | `false` | Sidebar TOC shown by default |
| `taskLists.toggleFromPreview` | `false` | The §7.5 opt-in |
| `wikiLinks.enabled` | `false` | Rendering-only (§12) |

Config changes re-render through the engine with new options (`reset: true` patch); theme-kind changes post `theme` so mermaid/highlight can restyle without a full reload.

## 7. Webview

### 7.1 Patch applier

The preview body is a flat list of top-level block elements. Applying a patch script is a cursor walk: `keep` advances, `replace`/`insert` parse the HTML strings into elements (via `<template>`), `delete` removes. Only inserted/replaced elements go through post-processing. The scroll map (§6.2) and the "current top line" bookmark are rebuilt after each patch; because untouched blocks keep their DOM nodes, images, open `<details>`, selection, and rendered SVGs all survive edits elsewhere in the document.

### 7.2 Post-processors (changed blocks only)

- **KaTeX**: `querySelectorAll('[data-math-style]')` within changed blocks; render with `throwOnError: false`; cache keyed by `style::tex` so unchanged formulas inside a replaced block still skip typesetting. KaTeX CSS + fonts keep the existing `copyKatexFonts` tsdown plugin ([tsdown.config.mts:16](extensions/markdown-preview-ultra/tsdown.config.mts#L16)).
- **Mermaid**: keep the current lazy-import + SVG cache design ([mermaid.ts](extensions/markdown-preview-ultra/webview/mermaid.ts)) — it is already the right shape; only the container selector changes. Add a per-diagram render timeout with an inline error card (MPE uses 30 s) so one pathological diagram can't wedge the preview.
- **highlight.js**: `common` bundle as today ([highlight.ts](extensions/markdown-preview-ultra/webview/highlight.ts)); applied per changed fence. Highlighting engine alternatives are §12.

### 7.3 UI chrome

All dependency-free, all small:

- **TOC sidebar** — collapsible panel rendered from the `toc` payload; toggled by an `esc`-adjacent keybinding inside the preview and a floating button; visibility persisted via `setState`. Active heading tracked while scrolling.
- **Heading anchors** — hover `¶` link per heading; click copies `#slug` and scrolls.
- **Copy-code button** — per fence, `navigator.clipboard.writeText` (works under webview CSP).
- **Image lightbox** — click any image for a dimmed full-size overlay, `esc`/click to close. (MPE ships this as `media/lightbox.js`; ours is a ~50-line module.)
- **Zoom** — `ctrl/cmd +/-/0` inside the preview adjusts a root `--zoom` scale, persisted via `setState`.
- **Frontmatter card** — existing design, fed structured data instead of raw YAML.

### 7.4 Theming

- **`auto` (default)**: the stylesheet is written against `--vscode-*` variables (as today), so it tracks any VSCode theme including high contrast.
- **`github-light` / `github-dark`**: self-contained palettes for users who want GitHub-faithful rendering regardless of editor theme. Each preview theme pins a paired highlight.js theme and a mermaid theme (MPE's `auto.css` mapping idea, hardcoded to our small matrix instead of a 25×17 table).
- Theme-kind changes (light↔dark) re-run mermaid with the new theme (cache keyed by theme, as today) — no full re-render needed since block HTML is theme-independent.

### 7.5 The one write path: task-list toggling (opt-in)

Clicking a rendered checkbox is the single most-missed interaction in a read-only preview, and it is a *structured, verifiable* edit: flip `[ ]`/`[x]` on one line. With `taskLists.toggleFromPreview: true`, the webview posts `toggleTask { line, checked }`; the host validates the line against `/^(\s*(?:[-*+]|\d+[.)])\s+)\[[ xX]\]/` and applies a one-character `WorkspaceEdit`. Default **off** to honor the "preview only" principle; behind the setting because it's too useful to omit entirely.

## 8. File Structure

```
extensions/markdown-preview-ultra/
  package.json              # commands, keybindings, settings per §6
  tsdown.config.mts         # host (cjs/node) + webview (esm/browser) entries, unchanged shape
  src/
    extension.ts            # activation, command registration
    previewManager.ts       # panel lifecycle, follow/lock/retarget (largely kept)
    modes.ts                # view-mode state machine + status bar item
    engine.ts               # WASM bridge: lazy require, Session per document
    scrollSync.ts           # editor-side sync + loop guard
    messages.ts             # typed protocol (§6.3)
    util.ts
  webview/
    index.ts                # bootstrap, message loop
    patch.ts                # block patch applier (§7.1)
    scrollMap.ts            # sourcepos → offset map, binary search
    postprocess/
      katex.ts  mermaid.ts  highlight.ts
    ui/
      toc.ts  copyCode.ts  lightbox.ts  zoom.ts  frontmatter.ts
    styles/
      preview.css  frontmatter.css  highlight.css  math.css  mermaid.css
      themes/github-light.css  themes/github-dark.css
  wasm/                     # generated by build:wasm (gitignored, ships in VSIX)
```

Dropped from the webview bundle: `markdown-it` + four plugins, `js-yaml` — replaced by the WASM module which never leaves the host. `katex`, `mermaid`, `highlight.js` remain (browser renderers). `.vscodeignore` copied per repo convention, keeping `wasm/` in the package.

## 9. Performance Targets

| Scenario | Target |
|---|---|
| Engine render, 100 KB document (cold session) | < 30 ms |
| Engine render + diff, keystroke in 10 K-line document | < 15 ms (comrak full-doc parse; diff is µs) |
| Patch apply + post-process, single-block edit | < 10 ms (one subtree swap + one KaTeX/hljs pass) |
| WASM artifact size | ≲ 3 MB on disk, loaded once per session, host-side only |
| Activation cost | ~0 (WASM loaded on first preview open, not activation) |

Full-document re-parse per keystroke is deliberate — comrak is fast enough that incremental *parsing* (RFC 001 §9's block-boundary re-parse machinery) is complexity we don't need; incrementality lives entirely in the diff/patch layer where it pays for itself in DOM stability, not CPU.

Debounce drops from 200 ms to ~150 ms (tunable) since renders are cheaper and patches don't flicker.

## 10. Security

1. **Sanitize in Rust** (§5.4): document-supplied HTML never reaches the webview unfiltered.
2. **CSP**: keep the current strict policy — `default-src 'none'`, nonce'd module scripts, images restricted to `webview.cspSource https: data:`. No CDN script loads (MPE loads ZenUML/MathJax from jsDelivr; we load nothing remote).
3. **Typed, validated messages** (§6.3): every webview→host message shape-checked; the only state-mutating message (`toggleTask`) is re-validated against document content and gated by a default-off setting.
4. **No execution surface**: no code chunks, no external binaries, no `config.js`/`parser.js`-style user scripts, no shell-outs.
5. **Link handling** stays host-side (`openLink`) with the existing scheme allowlist (`https?|mailto`) and workspace-relative resolution ([previewManager.ts:313](extensions/markdown-preview-ultra/src/previewManager.ts#L313)).

## 11. Testing

- **Engine (Rust, in-crate)**: snapshot tests for HTML output across a fixture corpus — including MPE's `test/markdown/{math,…}.md` fixtures where in-scope — plus unit tests for slug generation, sourcepos attribution, sanitizer allowlist (script/event-handler stripping), and diff correctness (property: applying patches to the previous block list reproduces the new one; edits at start/middle/end/multi-region).
- **Extension (vitest, in-tree)**: patch applier against synthetic scripts, scroll-map interpolation and binary search, message guards, task-line regex.
- **Manual acceptance checklist** in the PR: repo README files, a math-heavy doc, a mermaid-heavy doc, a 10 K-line doc, theme switching, all three modes, two-way sync.

Tests live with the code they test (crate tests in `crates/markdown-engine`, TS tests as `*.test.ts` siblings) per repo practice.

## 12. Open Questions

1. **Syntax highlighting engine.** highlight.js `common` (status quo; ~180 KB, decent fidelity) vs syntect-in-WASM (Rust purity; but fancy-regex fallback, ~1 MB+ grammar payload, slower cold start) vs shiki in the host (TextMate grammars, VSCode-grade fidelity, inline-styled HTML that would ride the existing patch protocol; adds a JS dependency host-side). Recommendation: ship Phase 1–3 with highlight.js, prototype shiki-in-host afterward — it fits the "render in host" architecture and would delete the webview highlighter entirely.
2. **`!!! note` admonition syntax** (MPE-style) in addition to GitHub `> [!NOTE]` alerts: a small block-level pre-pass in the engine. Worth it, or is the GitHub syntax enough?
3. **Task-checkbox toggle** (§7.5): keep as opt-in, promote to default-on, or drop to preserve a strictly read-only guarantee?
4. **`@import` / `![[file]]` transclusion**: requires the host to resolve and watch imported files and feed them to the engine (WASM can't do I/O). The engine API reserves room (`RenderOptions` can grow an `imports: {path: content}` map), but is the feature worth the file-graph invalidation complexity?
5. **Kroki opt-in** for non-mermaid diagrams (graphviz, plantuml, ditaa…): a single setting pointing at a server would cover a long tail cheaply, but sends document content to a third party by default-off necessity. Include ever?
6. **Keybinding for `cycleMode`**: `ctrl+k ctrl+m` proposed; verify against default keymap collisions before shipping.
7. **Additional theme pairs** (one-light/one-dark, solarized): trivially additive CSS; ship any beyond the GitHub pair initially?
8. **`.mdx` files**: continue treating as plain markdown (JSX blocks will render as inline code/HTML-escaped text, unchanged from today), or drop the `mdx` activation claim?

## 13. Phased Plan

### Phase 1 — Engine + parity
`crates/markdown-engine` (comrak, transform, sanitize, TOC, frontmatter, `wasm_api`; full-HTML `reset` renders only — no diffing yet). Host `engine.ts` bridge with graceful missing-WASM fallback message. Webview consumes full renders exactly as today (innerHTML swap). Existing commands/settings unchanged. **Milestone: feature parity with v0.3.1, markdown-it stack deleted, snapshot suite green.**

### Phase 2 — Incrementality + navigation
Block diff/patch protocol end-to-end; scroll map from `data-sourcepos`; two-way scroll sync with loop guard; double-click-to-source; `seq` staleness handling. **Milestone: zero-flicker typing on a 10 K-line document; preview scroll moves the editor.**

### Phase 3 — Modes + chrome
View-mode state machine, `cycleMode`/`switchMode`, status-bar item, panel serializer, find widget. TOC sidebar + `[TOC]` + heading anchors. GitHub alerts, emoji. Themes (`auto` + GitHub pair) with paired hljs/mermaid theming. Copy-code, lightbox, zoom, frontmatter card from structured data. **Milestone: the §3 "Adopt" column fully shipped.**

### Phase 4 — Options and stretch (each independently shippable)
Task-checkbox toggle (pending §12.3), wiki-link rendering, `!!!` admonitions, extra theme pairs, shiki-in-host prototype, `@import` design spike.

### Explicitly deferred
Presentation mode, export of any kind, notebook features — future RFCs if ever.

## 14. Risks

| Risk | Mitigation |
|---|---|
| comrak output differs from markdown-it in edge cases users notice | Snapshot corpus in Phase 1 includes real repo docs + MPE fixtures; differences triaged before parity milestone; comrak is the GFM reference implementation in Rust, so "renders like GitHub" is the tiebreak |
| ammonia (html5ever) inflates WASM size or misbehaves under wasm32 | Measured in Phase 1 week one; fallback path (comrak escape + webview DOMPurify) documented in §5.4 |
| Per-block rendering breaks document-global constructs (footnotes, reference links) | Resolution is parse-time; synthetic trailing footnote block; covered by dedicated diff tests |
| Patch protocol bugs corrupt preview DOM state | `reset: true` full-render escape hatch on any applier error + `error` message to host for logging |
| Two-way sync feedback loops | Origin-stamped scroll guard (§6.2); `scrollSync` off-switch retained |
| WASM missing in dev workflow (forgot `build:wasm`) | Same graceful degradation + console hint as log-viewer/dotenv; preview shows a friendly "engine not built" card instead of failing silently |
| Mode state machine fights VSCode's own layout persistence | Modes are *derived* from observable panel/editor state, not shadow state; serializer restores the panel and the mode is re-inferred |

## 15. Summary

Rewrite `markdown-preview-ultra` as a thin VSCode host + thin webview around a new `crates/markdown-engine` WASM module. The engine (comrak-based) owns parsing, GFM + alerts + emoji + math-span + mermaid-fence transformation, Rust-side HTML sanitization, GitHub-compatible TOC extraction, and block-level diffing that turns every keystroke into a minimal DOM patch. The webview keeps only what must run in a browser — KaTeX, mermaid, highlight.js — plus small self-written chrome: TOC sidebar, copy-code, lightbox, zoom. The host gains three cleanly-switchable view modes (Edit / Split / Preview) with a status-bar switcher, true two-way scroll sync, and click-to-source navigation.

Relative to MPE this drops export, code execution, presentations, notebooks, and external diagram services — and with them ~85 dependencies, a 4.2 MB React/Monaco webview bundle, and an attack surface that has needed repeated CVE patching — while adopting its genuinely valuable preview features on top of this repo's standard Rust/WASM extension architecture.
