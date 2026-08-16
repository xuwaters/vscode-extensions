# Git Compare

Compare your working copy with any branch, tag, or commit. The result is a
per-file tree in the Source Control sidebar — pick a ref once, then click
through the changed files one at a time in a normal diff editor. No stacked
"all changes in one scrollable editor" view.

## Features

- **A "Compare With" view in the Source Control sidebar.** Pick a ref and the
  view fills with the files that differ, grouped and laid out as a folder tree.
  The selection is remembered per workspace, so it is still there after a
  window reload until you clear it.

- **A ref picker that covers everything git calls a commit-ish.** The quick
  pick groups refs under *Upstream* (the remote branch matching your current
  branch name, listed first — the usual "what would I push?" comparison),
  *Default branches* (`main`, `master`, `trunk`, `develop`), *Local branches*,
  *Remote branches*, and *Tags*, each row showing the short SHA. The last entry
  is **Enter commit-ish…**, and anything you type in the filter box is offered
  as **Use "…"** — so a raw SHA, `HEAD~3`, or a branch that has not been
  fetched into the ref list all work. The picker opens immediately and streams
  refs in as git lists them, rather than blocking on a cold ref cache.

- **Three groups, with merge-base semantics.** Under the root row:

  - **Changed Files** — every file differing between the working copy (edits
    included, not just commits) and the chosen ref.
  - **Behind** — files touched by commits the compared ref has and `HEAD` does
    not, i.e. the merge-base-to-ref slice. Incoming changes.
  - **Ahead** — files touched by commits `HEAD` has and the compared ref does
    not. Outgoing changes.

  Each group carries a file count, and *Behind* / *Ahead* are empty when the
  two histories are unrelated (no common ancestor).

- **A real file tree, not a flat path list.** Folders sort before files, both
  alphabetically, and single-child folder chains collapse into one row
  (`src/util/text`) unless you turn `gitCompare.compactFolders` off. Each file
  row shows a status letter — `A`, `D`, `M`, `R`, `C`, `T` — with added and
  deleted files spelled out (`A · added`, `D · deleted`) and renames showing
  the old path (`R · old/name.ts →`).

- **Click a file, get a diff editor.** Clicking a row runs **Open Diff**, which
  opens VS Code's normal side-by-side diff. For *Changed Files* the left side
  is the file at the compared ref and the right side is the file on disk; for
  *Behind* and *Ahead* both sides come from history — merge-base against the
  compared ref, or merge-base against `HEAD`. Titles say which is which, e.g.
  `main.ts (v1.2.0) ↔ main.ts` or `main.ts (merge-base) ↔ main.ts (HEAD)`.
  Files that exist on only one side get an empty pane on the other, so a new
  file reads as all-added instead of erroring; pure additions skip the diff and
  open the file itself.

- **Open a file as it exists at the other ref.** **Open File at Revision**
  opens a read-only editor holding the file's content at the compared ref (or
  at `HEAD`, for rows under *Ahead*). The document keeps the original filename
  so syntax highlighting and language features work, and the tab gets a badge —
  `C` for the compared side, `W` for the working side — with the ref name in
  the hover. Renames are followed to whichever path the file has on that side.
  If the file does not exist there, you get a message instead of a git error.

- **Multi-root and multi-repo aware.** With more than one repository open you
  are asked which one to compare in; the tree tracks that repository's root.

- **Keeps itself current.** The tree reloads when the repository's git state
  changes (debounced by 250 ms), when you save a document (turn
  `gitCompare.refreshOnSave` off in very large repositories), and when
  `gitCompare.compactFolders` changes. **Refresh** forces a reload by hand.

Everything goes through the built-in Git extension's API rather than a separate
git process, so the underlying operations are the same ones the Source Control
view uses: `git diff <ref>` for the changed set, `git merge-base HEAD <ref>`
plus `git diff <merge-base> <ref>` and `git diff <merge-base> HEAD` for the
behind/ahead sets, and `git show <ref>:<path>` for file content at a revision.
The built-in Git extension (`vscode.git`) is a hard dependency and must be
enabled.

## Usage

1. Open the Source Control sidebar and expand **Compare With**. If no
   comparison is active the view shows a single **Compare with…** row.

2. Click it — or run **Git Compare: Compare With…** from the command palette,
   or press the compare icon in the view's title bar. If the workspace has
   several repositories, choose one first.

3. Pick a branch, tag, or remote branch from the list, or type a SHA or other
   commit-ish and choose **Use "…"**. The view focuses itself and loads.

4. Expand **Changed Files**, **Behind**, or **Ahead** and click any file to
   open its diff. Hovering a row shows the path, its status, and which side of
   the comparison it belongs to.

5. Use the two inline buttons on a file row when you want something other than
   a diff: **Open File at Revision** for the file's content at the other ref,
   **Open File** for the version on disk.

6. When you are done, press **Clear Comparison** in the title bar (it only
   appears while a comparison is active) to return the view to its
   **Compare with…** state.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `gitCompare.compactFolders` | `true` | Collapse single-child folders in the changed-file tree, like the Explorer's compactFolders setting. |
| `gitCompare.refreshOnSave` | `true` | Refresh the changed-file list when the user saves a document. Disable for very large repositories. |

## Commands

| Command | ID | Where it appears |
| --- | --- | --- |
| **Git Compare: Compare With…** | `gitCompare.choose` | Command palette; **Compare With** view title bar; inline on the comparison's root row |
| **Git Compare: Refresh** | `gitCompare.refresh` | Command palette; view title bar |
| **Git Compare: Clear Comparison** | `gitCompare.clear` | Command palette; view title bar (only while a comparison is active); inline on the root row |
| **Open Diff** | `gitCompare.openDiff` | Clicking a file row. Hidden from the command palette |
| **Open File at Revision** | `gitCompare.openAtRevision` | Inline button on a file row. Hidden from the command palette |
| **Open File** | `gitCompare.openFile` | Inline button on a file row. Hidden from the command palette |

The three file commands need a file row as their argument, so they are hidden
from the command palette; invoking them any other way reports that no file is
selected. The extension contributes no keybindings.
