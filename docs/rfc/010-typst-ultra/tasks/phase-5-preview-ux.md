# Phase 5 — Preview UX parity

**Goal**: the preview behaves like this repo's other preview. Same three modes, same keys, same icons,
plus the export affordances the preview needed anyway.
**Exit criterion**: `cmd+shift+v` in a `.typ` puts the document beside its source; clicking a second `.typ`
moves the preview to it; Export is one click from either the title bar or the page.
**Status**: ☑ Complete — 10 / 10

Scope comes from [Amendment 001](../proposal-amendment-001-preview-ux.md), which reviewed
markdown-preview-ultra's UX piece by piece and records what was adopted, adapted, and refused. Nothing here
touches Rust.

## Tasks

### View modes

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P5-01 | The mode state machine: `modeState.ts` (pure — mode derived from observable layout, never stored) plus `modes.ts`, the status-bar switcher, and the `typstUltra.mode` context key | [Amendment 001 §1](../proposal-amendment-001-preview-ux.md#1-three-view-modes-on-mpus-keys-and-icons) | ☑ |
| P5-02 | `editors.ts`: take a tab over with Reopen With rather than opening in front of it, so Edit ⇄ Preview never grows the tab bar. Preview mode opens the full-tab preview **by name**, needing no `editorAssociations` write | [preview.md §1](../design/preview.md#1-the-model) | ☑ |
| P5-03 | Keybinding and icon parity: `cmd+shift+v` → Split, `cmd+k cmd+v` focus toggle, `cmd+k cmd+m` cycle, `cmd+k cmd+p` toggle; `$(edit)` / `$(preview)` / `$(layout-sidebar-right)` in the title bar with the current mode hidden | [Amendment 001 §1](../proposal-amendment-001-preview-ux.md#1-three-view-modes-on-mpus-keys-and-icons) | ☑ |

**P5-03 notes.** Two deliberate divergences, both recorded in the amendment: `cmd+k cmd+l` (pin) is bound
only inside the preview panel, because in a text editor that chord folds code and typst files *are* code;
and `cmd+k cmd+j` (sync to cursor) is kept, having no markdown counterpart.

### Following the reader

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P5-04 | Retarget end to end: `typst/compile { uri }` from the host so the server's subject follows the focused file, the notification added to the server bundle's allowlist with `lastActiveUri` kept in step, and a webview reset of pages and scroll when `metrics` carries a new URI | [Amendment 001 §2](../proposal-amendment-001-preview-ux.md#2-following-the-active-editor-finished) | ☑ |
| P5-05 | Preview pin (`togglePreviewLock`, 📌 in the title, `typstUltra.previewLocked`), the side preview's group lock (`preview.lockPreviewGroup`), and focus toggle between editor and preview | [preview.md §1](../design/preview.md#1-the-model) | ☑ |
| P5-06 | `pageMemory.ts`: the page each document was read to, shared by both surfaces so a mode switch is continuous — restored **once per document**, not on every compile | [Amendment 001 §1](../proposal-amendment-001-preview-ux.md#the-review) | ☑ |

**P5-04 notes.** The Rust side has always handled `typst/compile`; it was missing from `server/main.ts`'s
notification allowlist, which is why the preview could retarget while the compiler stayed on the previous
file. `server/lsp.test.ts` now proves the fix against the built server over a real LSP connection.

**P5-05 notes.** `toggleLock` existed with no caller at all — no command, no keybinding, no menu entry. The
feature was written, shipped, and unreachable.

**P5-06 notes.** Restoring the parked page on *every* metrics refresh would snap the view to a page
boundary on every keystroke; the placement is one-shot per URI for that reason.

### Export

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P5-07 | Ask where the export goes: a save dialog opening on the document's own folder, `export.askForLocation` to skip it, the destination asked for **before** the work, and `baseFor` to reduce the answer to a base path that multi-file PNG can still suffix | [preview.md §7](../design/preview.md#7-export) | ☑ |
| P5-08 | Export where the reader is: `$(export)` in the editor title bar, `⭳` in the preview's toolbar, and the explorer context menu — plus the `export` and `openSource` webview messages, each with the same hand-written guard every other variant has | [preview.md §3](../design/preview.md#3-update-protocol) | ☑ |

**P5-07 notes.** Export writes the server's *last good document*, which is whichever file it was last told
to compile — so the command names its subject first. Without that, "export" in a two-document workspace
could write one document's pages under the other's name.

### Chrome

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P5-10 | Move the toolbar from the bottom of the page to the top, in the flow rather than absolutely positioned, with the compile-status banner below it | [Amendment 001 §5](../proposal-amendment-001-preview-ux.md#5-the-toolbar-moves-to-the-top) | ☑ |

### Naming

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P5-09 | Command category → **Typst Ultra**; the custom editor's view type → `typstUltra.editor`, leaving `typstUltra.preview` to the panel so `when` clauses can name a surface; `typstUltra.selectMain` declared in `package.json` | [Amendment 001 §4, §6](../proposal-amendment-001-preview-ux.md#4-the-two-preview-surfaces-are-now-distinguishable) | ☑ |

**P5-09 notes.** `selectMain` was registered in code as the compile-root status bar item's command but
never contributed, so it existed only for anyone who clicked the item. The view-type rename is the one
breaking change in this phase: a user who had hand-written
`"workbench.editorAssociations": { "*.typ": "typstUltra.preview" }` must change it to `typstUltra.editor`.

## Exit checklist

- [x] `cmd+shift+v` means the same thing here as in markdown-preview-ultra
- [x] The current mode's own button is hidden, so the title bar shows the two moves available
- [x] Opening a second `.typ` moves the preview, the compiler, and the webview's page list to it —
      `server/lsp.test.ts` proves the compiler half against the built server
- [x] A pinned preview stays on its file, and says so in its title
- [x] Export is reachable from the title bar, the page, the explorer, and the palette
- [x] The export dialog opens on the document's folder, and cancelling costs nothing
- [x] Extension tests: 91 → **116**, all passing
- [ ] Verified in a running VSCode window — **not done here.** Every claim above is asserted by a test or
      by the contributed `when` clauses; the layout behaviour (group lock, tab replacement, focus
      hand-off) is the part that needs a real window, and it is the part most worth checking first
