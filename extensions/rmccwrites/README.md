# Remove .cc-writes

Claude Code stages edits in `.cc-writes` directories, usually under `.claude`,
and leaves the empty shells behind once the writes have landed. This extension
walks your workspace and removes those directories — but only the ones that are
completely empty, so nothing you still have on disk can be lost.

## What it removes, exactly

The rules are narrow on purpose:

- **Only directories whose name matches `rmccwrites.names`** — `.cc-writes` by
  default. Names are compared as base names; nothing else is a candidate.

- **Only when the directory is empty.** Every removal is a non-recursive
  directory delete. A `.cc-writes` holding so much as one file or one
  subdirectory is left alone, and the walk re-checks emptiness immediately
  before deleting, not just during the scan.

- **Plus the containers left hollow behind them.** `rmccwrites.prune` lists
  names that follow their contents out, `.claude` by default: if removing
  `.claude/.cc-writes` leaves `.claude` with nothing in it, `.claude` goes too.
  A `.claude` that still holds settings, agents or anything else stays. Set
  `rmccwrites.prune` to `[]` if you would rather keep the empty container.

It never leaves the folders you point it at. The walk starts at your workspace
folders — or at the folder you right-clicked — and only descends. `.git`
directories are never entered, and symlinks are never followed, so a symlinked
directory cannot be used to step outside the tree. Removals are permanent: they
do not go to the trash.

## Usage

1. Run **Remove .cc-writes: Preview Empty .cc-writes Directories (Dry Run)**
   from the Command Palette. Nothing is touched; the extension scans every
   workspace folder behind a cancellable progress notification and counts what
   it finds.

2. A quick pick opens listing every directory that can be removed, by
   workspace-relative path. From there you can pick **Remove Them** to go ahead,
   **Show Log** to read the full dry-run output, or any listed directory to
   reveal it in the Explorer. Picking a directory or pressing Escape closes the
   list without removing anything.

3. Choosing **Remove Them** starts a second pass that does the removal, and a
   notification reports how many directories went. If anything could not be read
   or deleted, the notification is a warning with the error count and the log has
   the details.

**Remove .cc-writes: Remove Empty .cc-writes Directories** is the same flow with
the intent stated up front: it still scans first and still shows the same
confirmation list before touching anything, unless you turn
`rmccwrites.confirmBeforeRemoving` off, in which case it scans and removes in
one go.

To clean one subtree instead of the whole workspace, right-click a folder in the
Explorer and choose **Remove Empty .cc-writes Directories Here**. Multi-select
works; every selected folder is scanned. The scanned folder is itself a
candidate when its name matches.

Everything a run did — the roots, the settings each root was scanned with, and
one line per directory found or removed — goes to the **Remove .cc-writes**
output channel, reachable with **Remove .cc-writes: Show Log** or the **Show
Log** button on any notification.

## Working with .gitignore

`.claude` is usually gitignored, which is awkward for a cleaner that skips
ignored directories — the targets live inside the thing being skipped. So the
rules are:

- With `rmccwrites.respectGitignore` on (the default), the walk reads
  `.gitignore` files at and below each scanned folder, deeper files overriding
  shallower ones and the last matching rule in a file winning, the way git does
  it. Patterns support `*`, `**`, `?`, `[charset]`, `!` negation, a trailing `/`
  for directory-only rules and a leading `/` for anchoring. An ignored directory
  prunes the whole subtree below it.

- Target names and the names in `rmccwrites.descend` (`.claude` by default) are
  always entered anyway, ignored or not.

- Turning `rmccwrites.respectGitignore` off scans everything, `.git` and
  symlinks excepted.

Only `.gitignore` files inside the scanned tree are read; a `.gitignore` above
the workspace root, your global excludes and `.git/info/exclude` are not
consulted.

## Settings

`names`, `descend`, `prune` and `respectGitignore` are resource-scoped, so a
multi-root workspace can configure each folder separately.

| Setting | Default | Description |
| --- | --- | --- |
| `rmccwrites.names` | `[".cc-writes"]` | Directory names removed when they are completely empty. Base names only — entries containing `/` are ignored. Nested matches are handled bottom-up, so an empty match inside another match removes both. |
| `rmccwrites.descend` | `[".claude"]` | Directory names entered even when a `.gitignore` file excludes them. `.claude` is typically ignored, yet holds the `.cc-writes` targets. |
| `rmccwrites.prune` | `[".claude"]` | Directory names removed once the scan leaves them empty, so removing `.claude/.cc-writes` takes `.claude` with it. Set to `[]` to keep such containers. |
| `rmccwrites.respectGitignore` | `true` | Skip directories excluded by a `.gitignore` file at or below the scanned folder. Target names and `rmccwrites.descend` names are always entered; any other ignored directory prunes everything below it. Turn this off to scan those too. |
| `rmccwrites.confirmBeforeRemoving` | `true` | List what was found and ask for confirmation before anything is removed. |
| `rmccwrites.revealLog` | `false` | Open the log panel automatically at the end of every run. |

In the three name lists, blank entries, `.`, `..`, `.git` and anything
containing `/` are dropped, and duplicates are collapsed. A value that is not an
array falls back to the default; an explicitly empty array is taken at its word,
so `"rmccwrites.names": []` means nothing will ever match.

## Commands

- **Remove .cc-writes: Remove Empty .cc-writes Directories**
  (`rmccwrites.clean`) — scan every workspace folder and remove what is found,
  after confirmation.

- **Remove .cc-writes: Preview Empty .cc-writes Directories (Dry Run)**
  (`rmccwrites.preview`) — scan without removing anything, then offer the
  removal from the result list.

- **Remove Empty .cc-writes Directories Here** (`rmccwrites.cleanFolder`) — the
  Explorer folder context-menu entry, scoped to the selected folders. Not
  offered in the Command Palette.

- **Remove .cc-writes: Show Log** (`rmccwrites.showLog`) — open the output
  channel.
