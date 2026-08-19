# RFC 010 — Amendment 001: Preview UX parity with Markdown Preview Ultra

**Status**: Implemented
**Date**: 2026-08-17
**Amends**: [proposal.md §7 Extension Surface](proposal.md#7-extension-surface) ·
[design/preview.md §1, §3, §6, §7](design/preview.md)
**Affects**: `extensions/typst-ultra` only — no Rust change, no protocol change on the wire except one
notification the server already understood

---

## Why

RFC 010 said the preview would use "the same model as
[markdown-preview-ultra](../009-markdown-preview-ultra/proposal.md#61-view-modes), which is already proven
in this repo" ([design/preview.md §1](design/preview.md#1-the-model)). What shipped was the *panel* part of
that model. The rest of what makes MPU pleasant — three view modes on one key, the tab-replacement trick
that keeps the tab bar from growing, a preview that actually follows you when you click another file, a
group lock so the next file you open does not land on top of the preview — was never brought across.

The gap was not visible from the task list, because every task it fell between was done. It is visible the
first time you use both extensions in one window: `cmd+shift+v` means two different things, the title bar
has one button in one and five in the other, and clicking a second `.typ` leaves the preview showing the
first one's pages.

This amendment closes that gap, and adds the export affordances the preview needed anyway.

## The review

Every piece of markdown-preview-ultra's UX, and what happened to it here.

| MPU behaviour | Verdict | Why |
| --- | --- | --- |
| Three view modes (Edit / Split / Preview) with one state machine | **Adopt** | The reason `cmd+shift+v` can mean "put it beside me" from either side |
| Mode derived from observable layout, not stored | **Adopt** | A stored mode goes stale the moment the user drags a tab. `modeState.ts` is the same 20 lines, tested |
| Title-bar icons `$(edit)` / `$(preview)` / `$(layout-sidebar-right)`, current mode hidden | **Adopt** | Including the choice *not* to use `$(split-horizontal)`, which is VSCode's own Split Editor Right |
| Status-bar mode switcher | **Adopt** | Placed at priority 97, below the three items typst already shows |
| Full-tab preview replaces the source tab (Reopen With), rather than opening in front of it | **Adopt** | The tab bar is the same width either side of a mode switch |
| Panel follows the active editor | **Adopt, and finish it** | It was half-built here: the panel retargeted but the server kept compiling the previous file. See §2 |
| Preview pin, with a title marker | **Adopt** | `toggleLock` existed with **no caller** — no command, no keybinding, no menu. It was unreachable |
| Group lock for the side preview | **Adopt** | Without it, the next file opened from the explorer replaces the preview |
| Toggle focus between editor and preview | **Adopt** | `cmd+k cmd+v`, same key |
| Reader's place survives a mode switch | **Adopt, adapted** | MPU parks a *line*; a typst preview parks a *page* ([`pageMemory.ts`](../../../extensions/typst-ultra/src/preview/pageMemory.ts)) |
| In-page toolbar button back to the source | **Adopt** | A full-tab preview otherwise has no way back except the title bar |
| `workbench.editorAssociations` shipped as a `configurationDefaults` | **Reject** | Stands: [design/preview.md §1](design/preview.md#1-the-model) and RFC 009 both call this out. Preview mode asks for the editor **by name** instead, which needs no global setting |
| Search-result hand-off (`search.action.getSearchResults`) | **Reject** | It exists because MPU's association claims every `.md`. Nothing reaches our preview editor unasked, so there is nothing to hand back |
| Link history (back / forward in the preview) | **Reject for now** | A typst document's links resolve to a page or a URL, not to another document's preview. There is no trail to walk |
| TOC sidebar, theme/font overrides, task-list write path | **Reject** | Markdown-specific. A typst document brings its own design, and the preview stays read-only |

## What changes

### 1. Three view modes, on MPU's keys and icons

New: [`src/preview/modeState.ts`](../../../extensions/typst-ultra/src/preview/modeState.ts) (pure, tested)
and [`src/preview/modes.ts`](../../../extensions/typst-ultra/src/preview/modes.ts) (the manager), plus
[`src/preview/editors.ts`](../../../extensions/typst-ultra/src/preview/editors.ts) for the tab-replacement
helpers.

| Mode | Layout | Entered by |
| --- | --- | --- |
| **Edit** | text editor only | `typstUltra.setModeEdit`, closing the preview |
| **Split** | editor + preview beside, preview's group locked | **`cmd+shift+v`**, `cmd+k v`, `typstUltra.setModeSplit` |
| **Preview** | the preview *in the source's own tab* | `typstUltra.setModePreview` |

`cmd+shift+v` used to be `showPreview` — the panel in the active column, covering the source. It is now
`setModeSplit`, which is what it means in markdown-preview-ultra and what a reader pressing it expects.

Two keybindings differ from MPU deliberately:

- **`cmd+k cmd+l` (pin) is bound only inside the preview panel**, not in the editor. In a text editor that
  chord is VSCode's fold-level toggle, and typst files are code — shadowing it for markdown is one thing,
  shadowing it in a language with `#let` blocks is another.
- **`cmd+k cmd+j` (sync preview to cursor) is kept**; it has no MPU counterpart because markdown has no
  page to jump to.

### 2. Following the active editor, finished

The panel already retargeted on `onDidChangeActiveTextEditor`. Two things underneath it did not:

1. **The server kept compiling the previous document.** It follows whatever was last opened, changed, or
   saved — which is right while typing and wrong the moment you click between two files that are *both
   already open*, because no edit arrives to say so. The host now sends **`typst/compile { uri }`** on a
   retarget. The Rust side has always understood this notification
   ([`dispatch.rs`](../../../crates/typst/typst-lsp-core/src/dispatch.rs)); it was simply not in the
   server bundle's allowlist. A pinned compile root outranks it and the host does not even send it —
   recompiling a book to switch chapters is a waste of the engine's only thread.
2. **The webview kept the old document's pages.** `metrics` has always carried a `uri`; the webview now
   compares it and resets the page list and the scroll position when it changes. Otherwise a two-page
   letter opens at page 40 of the book it replaced.

The full-tab preview claims the compile the same way when its tab becomes active — it is not a text
editor, so nothing else tells the server it is now the subject.

### 3. Export, with a destination

| Before | After |
| --- | --- |
| Export writes `export.outputPath` silently | A save dialog opens **on the document's own folder**, under the document's own name |
| No way to choose a folder short of editing a setting | The dialog is the choice; `export.askForLocation: false` restores the old behaviour |
| The export request is sent, *then* the file is written | The destination is asked for **first** — a reader who changes their mind has not paid for an export |
| Reachable from the palette only | Palette, editor-title `$(export)` button, the preview toolbar's `⭳`, and the explorer context menu |
| Exports whatever the server last compiled | Names its subject first, so it cannot write document A's pages under document B's name |

The multi-file PNG behaviour is unchanged (`name-1.png`, `name-2.png`), which is why the dialog's answer is
reduced to a base path: [`baseFor`](../../../extensions/typst-ultra/src/commands/exportPath.ts) strips the format's
own extension and nothing else, so a reader who types `paper.v2` keeps their `.v2`.

### 4. The two preview surfaces are now distinguishable

The panel and the custom editor both used the view type `typstUltra.preview`, so
`activeWebviewPanelId` and `activeCustomEditorId` could not tell them apart — and a `when` clause that
cannot name a surface cannot put a button on it.

**The custom editor is now `typstUltra.editor`; the panel keeps `typstUltra.preview`.** Same split as
markdown-preview-ultra. The only compatibility cost is a user who had manually written
`"workbench.editorAssociations": { "*.typ": "typstUltra.preview" }` — never something this extension wrote,
per [design/preview.md §1](design/preview.md#1-the-model) — who must change it to `typstUltra.editor`.

While it was open, the custom editor also picked up the message handling it never had: it answered `ready`
and `viewport` and dropped everything else, so a full-tab preview had no click-to-source, no link
handling, and no state persistence. Both surfaces now share
[`src/preview/rpc.ts`](../../../extensions/typst-ultra/src/preview/rpc.ts) rather than each keeping its own
copy of the request shapes.

### 5. The toolbar moves to the top

It was pinned to the bottom of the page, which is where a status bar goes, not where a document viewer's
controls go. At the top it continues the editor's own title bar: the buttons that act on the *tab* and the
buttons that act on the *page* now sit in one band instead of at opposite ends of the panel. It is also
where the eye already is after clicking Split or Export.

The toolbar is now in the flow rather than absolutely positioned, so the page list takes the remaining
height with nothing to offset by hand; the compile-status banner sits below it, which stops an error
swallowing the zoom controls. Both surfaces share `html.ts`, so both move together — a toolbar that
changed ends depending on which surface you were in would be its own small bug.

### 6. Command category

Every command moves from **Typst** to **Typst Ultra**, matching the extension's display name and the
`Markdown Preview Ultra` precedent. `typstUltra.selectMain` — registered in code, backing the compile-root
status bar item, and missing from `package.json` — is now declared, so it is reachable from the palette.

## Surface delta

**New commands** (all category `Typst Ultra`): `setModeEdit`, `setModeSplit`, `setModePreview`,
`cycleMode`, `switchMode`, `toggleEditPreview`, `toggleFocus`, `togglePreviewLock`, `selectMain`.

**New context keys**: `typstUltra.mode` (`edit` | `split` | `preview`), `typstUltra.previewLocked`.

**New settings**:

| Setting | Default | |
| --- | --- | --- |
| `typstUltra.preview.defaultMode` | `"split"` | Placement when the preview is opened without one |
| `typstUltra.preview.lockPreviewGroup` | `true` | Keep newly-opened files out of the preview's group |
| `typstUltra.export.askForLocation` | `true` | Ask where to save, starting at `export.outputPath` |

**New webview → host messages**: `export`, `openSource`. Both are payload-free, and both go through the
same hand-written guard every other variant does — the rule from
[design/preview.md §3](design/preview.md#3-update-protocol) has not been bent for the easy cases.

## Testing

Extension tests: **91 → 116**, all passing.

| What | Where |
| --- | --- |
| The mode rule, including a preview tab winning over a panel elsewhere | `src/preview/modeState.test.ts` |
| The reader's parked page, including a page index that is not one | `src/preview/pageMemory.test.ts` |
| The export base path: extension stripped, `paper.v2` kept | `src/commands/exportPath.test.ts` |
| The two new messages, and that neither smuggles a URI | `src/preview/messages.test.ts` |
| A document switch drops the old pages, cache, and scroll position | `webview/pageList.test.ts` |
| **`typst/compile` changes the compiled document**, over a real LSP connection to the built server | `server/lsp.test.ts` |

The last one is the load-bearing test of §2: it opens a one-page and a two-page document, lets the second
become the subject, then sends nothing but the notification and asserts the metrics now describe the first.

## What this does not do

- **No second preview panel.** One panel, one subject — unchanged from
  [design/preview.md §1](design/preview.md#1-the-model), and everything above assumes it.
- **No write path from the preview.** MPU has exactly one (task-list toggling, opt-in); the typst preview
  has none, and this amendment adds none.
- **No editor-association default.** Preview mode reaches the full-tab preview by asking for it by name.

## Revisit if

- **A retarget compile becomes noticeable.** Clicking a chapter of an unpinned book now compiles that
  chapter on the click rather than on the first keystroke. Cold compile is measured at
  [524 ms at 104 pages](research/corpus.md), in a child process, so it does not block the UI — but if
  reports come in, the answer is to make the pin suggestion ([0008](decisions/0008-compile-root.md)) more
  insistent, not to stop following the editor.
- **VSCode ships a real preview-mode API.** The Reopen With trick in `editors.ts` is a command, not API;
  a supported equivalent should replace it.
- **A typst document gains cross-file preview navigation.** Then MPU's link history stops being
  inapplicable and starts being missing.
