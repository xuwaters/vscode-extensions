# RFC 004: Git Branch / Revision Compare View

**Status**: Draft
**Date**: 2026-04-27
**Extension name**: `wx-vsce-git-compare`

---

## 1. Motivation

The built-in VSCode Git extension exposes a single "Open Changes" entry
point on the Source Control panel. Clicking it opens *every* changed file in
one stacked diff editor that hides everything except the changed lines plus
a few lines of context. That format is hostile to anything resembling a real
review:

- The reviewer cannot see the surrounding code that gives a hunk meaning —
  function signatures above, related branches below, the import block at the
  top of the file. Reading review comments from the team is even worse, as
  the comment author was looking at the file with full context, but the
  reviewer is looking at a strip of three lines.
- Files are concatenated end-to-end, so navigating between files is scroll
  guesswork. There is no per-file tree, no per-file affordances.
- There is no obvious way to compare the working copy against an arbitrary
  branch or revision — the built-in flow is geared toward "what's in the
  index vs. what's in HEAD". Comparing against `origin/main` or a tag
  requires either dropping to the terminal, or installing GitLens and
  learning a different mental model.

GitLens solves this with its **Compare** view: pick a base, pick a target,
get a per-file tree where each item opens a *real* diff editor (full file
side-by-side, not a hunk strip). This RFC proposes a small, focused
extension that delivers the *one* GitLens feature the user actually wants,
without dragging in the rest of GitLens (which is heavy, opinionated, and
has its own auth / telemetry surface).

**Why not a fork of vscode.git**. The built-in extension is tightly
maintained inside the VSCode repo itself; bolting a tree view onto it would
require shipping a fork. Instead, we sit *next to* it in the Source Control
viewlet and consume its public API
(`vscode.extensions.getExtension('vscode.git').exports.getAPI(1)`).

**Shape of `wx-vsce-git-compare`**. A single small TypeScript extension:

- Contributes one view (`gitCompare.view`) into the built-in `scm`
  view container, so it sits alongside Source Control and Source Control
  Graph at the top of the SCM sidebar.
- The view's first/top item is **"Compare with…"**. Clicking it opens a
  Quick Pick of branches / tags / recent commits to compare the working
  copy against.
- Once a ref is picked, the view shows a folder-shaped tree of changed
  files. Clicking a file opens a normal `vscode.diff` editor: full file on
  the left at the picked ref, working-copy file on the right.
- Each file row shows three inline icons on hover:
  1. **Open file at revision** — opens a read-only editor on the file's
     content at the picked ref (uses the `git:` URI scheme that vscode.git
     already serves).
  2. **Open file** — opens the working-copy file in a normal editor.
  3. **Open file on remote** — delegates to the built-in
     `git.openFileOnRemote` command, so whatever remote provider the user
     has configured (GitHub, GitLab, Bitbucket, Azure DevOps via
     third-party extensions) just works.
- Activates on view visibility (`onView:gitCompare.view`). No work happens
  until the user actually opens the SCM sidebar.
- Zero native deps, zero WASM, no Rust crate. Everything goes through the
  vscode.git API and built-in commands.

## 2. Design Goals

1. **One-click "compare working copy with a different ref"**. The default
   ref list includes the current branch's upstream, the default branch
   (`main` / `master` / `trunk`), recent local branches, all remote
   branches, all tags, and a free-form commit-ish entry at the bottom.
2. **Per-file tree, not stacked diffs**. Files are grouped by directory in a
   collapsible tree that mirrors the workspace layout. Each file is a leaf.
3. **Real diff editors on click**. Clicking a leaf opens
   `vscode.diff(leftUri, rightUri, title)`. The left side is the file at
   the picked ref (read-only `git:` URI). The right side is the working-copy
   file (writable `file:` URI). This is the same primitive the built-in Git
   extension uses for its index/HEAD diffs, so commenting, side-by-side
   navigation, and "Compare Selected" all work without us doing anything.
4. **Hover affordances per file** — three small inline icons:
   - Open at revision (read-only)
   - Open working-copy file
   - Open on remote (delegates to `git.openFileOnRemote`)
5. **Multi-repository awareness**. If the workspace has more than one
   repository, the Quick Pick prompts which repo to use first. The picked
   repo + ref pair is remembered per workspace.
6. **Persistence across reloads**. The last (repo, ref) pair is stashed in
   `workspaceState` so the view rehydrates with the previous comparison
   after reload. A "Change comparison ref…" command and a refresh icon on
   the view title bar let the user re-pick or re-run.
7. **No assumptions about a remote**. "Open on remote" hides itself when
   `git.openFileOnRemote` reports no remote; the other two icons still
   work.
8. **Graceful when no repo**. If no Git repository is detected in the
   workspace, the view shows a `viewsWelcome` message ("No Git repository
   in this workspace") with a button to refresh.

## 3. Architecture

```
┌──────────────────────────────────────────────────────────┐
│ VSCode Host                                                │
│                                                            │
│  vscode.git extension (built-in)                           │
│       │  exports getAPI(1) → GitAPI                        │
│       ▼                                                    │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ wx-vsce-git-compare                                  │  │
│  │                                                        │  │
│  │  GitBridge        → wraps GitAPI; selects repo;       │  │
│  │                     lists refs; runs `diffWith(ref)`. │  │
│  │                                                        │  │
│  │  CompareState     → (repo, ref) pair, persisted in    │  │
│  │                     workspaceState.                    │  │
│  │                                                        │  │
│  │  CompareTree-     → vscode.TreeDataProvider<Node>     │  │
│  │   DataProvider      Node = ChooseRefNode              │  │
│  │                          | HeaderNode                 │  │
│  │                          | FolderNode                 │  │
│  │                          | FileNode                   │  │
│  │                                                        │  │
│  │  Commands         → gitCompare.choose                  │  │
│  │                     gitCompare.refresh                 │  │
│  │                     gitCompare.openDiff                │  │
│  │                     gitCompare.openAtRevision          │  │
│  │                     gitCompare.openFile                │  │
│  │                     gitCompare.openOnRemote            │  │
│  └──────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

### 3.1 Tree shape

```
Compare With                                       (view title)
├─ ◧ Compare with…                                 (ChooseRefNode — always)
└─ ⌥ HEAD ↔ origin/main · 14 files                 (HeaderNode — when active)
   ├─ src/                                         (FolderNode)
   │   ├─ extension.ts        [M]    [⤴ ⤵ ↗]      (FileNode + inline icons)
   │   └─ tree.ts             [A]    [⤴ ⤵ ↗]
   └─ docs/
       └─ rfc/004-…           [M]    [⤴ ⤵ ↗]
```

- **ChooseRefNode** is always at the top; clicking it (`command` on the
  TreeItem) runs `gitCompare.choose`.
- **HeaderNode** appears only after the user has picked a ref. Its label
  shows the comparison summary; expanding it reveals the file tree.
- **FolderNode**s are synthesized from the changed-file paths so the tree
  mirrors the workspace layout. Folders with a single child get collapsed
  into the child (`src/inner/file.ts` rather than `src/ → inner/ → file.ts`)
  — this matches the explorer's `compactFolders` setting.
- **FileNode** carries the change kind (`A`/`M`/`D`/`R`) as a description
  badge, has a `command: gitCompare.openDiff` so single-click opens the
  diff, and exposes a `contextValue` of `gitCompare.file` so the inline
  icons hook in via `menus.view/item/context` with `group: "inline"`.

### 3.2 URI construction

The vscode.git extension serves a `git:` URI scheme for any file at any ref
via its content provider. The query string format is well-known and
stable:

```ts
function toGitUri(uri: Uri, ref: string): Uri {
  return uri.with({
    scheme: 'git',
    path: uri.path,
    query: JSON.stringify({
      path: uri.fsPath,
      ref,
    }),
  });
}
```

This is what the built-in Git extension itself produces for its diffs;
opening one through `vscode.diff` yields the exact same editor the user
gets when they click a file in the Source Control panel — including the
scroll-sync, gutter actions, and "compare with" affordances.

### 3.3 Listing refs

The Quick Pick shown by `gitCompare.choose` is built from
`Repository.state.refs` (covers local branches, remote branches, tags) plus
a synthetic "Enter commit-ish…" entry that drops to a free-form input box.
Refs are grouped:

1. **HEAD** of the upstream of the current branch (if any)
2. **Default branch** (`main` / `master` / `trunk`, picked by name match)
3. **Recent branches** — local branches sorted by recency
4. **Remote branches** — `origin/*` and friends
5. **Tags**
6. **— enter commit-ish…**

Each entry shows the short SHA and commit subject as Quick Pick `detail`,
so the user knows *what* they're about to compare against.

### 3.4 Computing the file list

`Repository.diffWith(ref)` (and its single-file variant
`diffWith(ref, path)`) is part of the public Git API and returns
`Change[]`. Each `Change` has `originalUri`, `renameUri`, `uri`, and a
`status: Status` enum. We map that to our `FileNode`s directly. For renames
we display `old → new` and the diff editor uses `originalUri` on the left
at `ref` and `uri` on the right at the working copy.

### 3.5 Refresh & invalidation

- The view exposes a refresh icon (`gitCompare.refresh`) on the title bar.
- We listen to `Repository.state.onDidChange` and debounce-refresh the
  tree, so commits, checkouts, and stashes update the changed-file list
  without manual intervention.
- `vscode.workspace.onDidSaveTextDocument` triggers a debounced refresh as
  well, so the badge counts stay honest while editing.
- Switching repository or ref is an explicit user action via
  `gitCompare.choose`; we do not auto-switch on `HEAD` change.

## 4. UX Notes

- The Source Control viewlet already hosts **Source Control**, **Source
  Control Graph**, and **Source Control Repositories**. Our view sits at
  the top by declaring `"order": -100` in its view contribution, matching
  the user's request to "add a new item to the GRAPH items to the top".
  Order is hint-only across extension contributions; users can drag to
  reorder via the context menu.
- The view container icon is `$(git-compare)`, the same glyph the built-in
  diff editor uses, so it reads as a comparison surface.
- Each file row's inline icons use VSCode's codicon set:
  - `$(git-commit)` — open at revision
  - `$(go-to-file)` — open working-copy file
  - `$(globe)` — open on remote
  Tooltips are explicit ("Open `path` at `ref`", etc.) so the affordances
  remain discoverable.

## 5. Out of Scope

- Three-dot ranges (`A...B`) and arbitrary base/target pairs. v1 always
  compares the **working copy** against the picked ref. A later revision
  could expand `gitCompare.choose` to ask for two refs.
- Inline blame, file history, commit graphs, codelens. Those belong in
  GitLens; this extension is intentionally a single feature.
- Staged-vs-HEAD or HEAD-vs-index toggles. The built-in Source Control
  panel already does these well.
- Editing comments / reviews. The diff editor we open is the standard one,
  so anything VSCode itself supports (Comments API consumers, GitHub PRs
  extension overlays, etc.) keeps working.

## 6. Implementation Plan

1. **Scaffolding** — `extensions/git-compare/` with `package.json`,
   `tsconfig.json`, `tsdown.config.mts`, `LICENSE.md`, mirroring
   `extensions/base64-tools/`. Activates on `onView:gitCompare.view`.
2. **GitBridge** — thin wrapper over `GitExtension.getAPI(1)`. Resolves the
   active `Repository`, exposes `listRefs()`, `diffWith(ref)`, `getCommit(ref)`,
   and `toGitUri(uri, ref)`.
3. **CompareState** — holds the current `(repoRoot, ref)` selection,
   persists to `context.workspaceState`, fires a change event.
4. **CompareTreeDataProvider** — builds the node tree per §3.1, handles
   refresh, listens to repo state.
5. **Commands** — `choose`, `refresh`, `openDiff`, `openAtRevision`,
   `openFile`, `openOnRemote`. The last is a thin shim over
   `git.openFileOnRemote` so it picks up whatever remote provider the user
   has configured.
6. **Contributions in `package.json`** — view container slot, view, view
   welcome message, command palette entries, `view/title` and
   `view/item/context` menus for the inline icons.
7. **Manual smoke test** — open the host repo, pick `origin/main`, verify
   the file tree matches `git diff --name-status origin/main`, click each
   icon affordance, switch refs, switch repos.
