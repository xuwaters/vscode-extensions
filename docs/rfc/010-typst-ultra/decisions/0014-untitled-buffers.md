# 0014 — Compile untitled buffers from a reserved project path

**Status**: Accepted
**Date**: 2026-08-20

## Context

A reader opens a new untitled file, sets its language to Typst, and opens the preview. Nothing renders —
or worse, the pages of some *other* document appear, because the preview serves the server's last good
compile and this buffer never reached the server at all.

Three separate things had to be true for that to happen, and each of them is a defensible rule on its own:

- The language client's document selector was `scheme: 'file'`, so `vscode-languageclient` sent no
  `didOpen` for a buffer with no file. The server did not know the document existed.
- `UriMap::to_file_id` resolves a URI by stripping the compile root or the package cache prefix. An
  `untitled:` URI matches neither, so the buffer was "not in project" ([0008](0008-compile-root.md)) and
  `didOpen` would have been dropped anyway.
- Nothing on the host side gates on the scheme — every check is `languageId === 'typst'` — so the preview
  opened, targeted the buffer, and asked for pages that describe a different document.

The underlying obstacle is typst's data model. Every `FileId` is a path under a root, and upstream offers
two roots: the project and a package. An untitled buffer has real text and no path.

Fixing it by *refusing* — "save the file first" — is a legitimate option, and one line of `when` clause.
It was rejected because a scratch buffer is how people try a language out, and "it does not work until you
commit to a filename" is the wrong first five minutes for the same reason
[0008](0008-compile-root.md) rejected requiring configuration.

## Decision

**Give untitled buffers a project path in a reserved directory, and serve them entirely from the
open-document overlay.**

`untitled:Untitled-1` maps to the project-rooted virtual path `/.typst-ultra/untitled/Untitled-1`.
`typst_session::Vfs` already holds every open document's text in an overlay that is authoritative over the
file provider, so nothing reads that directory and it never has to exist. The mapping lives in
`UriMap`, which is the single place both directions are decided:

| URI | File id |
| --- | --- |
| `untitled:Untitled-1` | `Project` + `/.typst-ultra/untitled/Untitled-1` |
| `untitled:/drafts/x.typ` | `Project` + `/.typst-ultra/untitled/drafts/x.typ` |
| `untitled:../../main.typ` | none — refused, see below |

Adding a third `VirtualRoot` variant upstream would be cleaner and is ruled out by
[0001](0001-unmodified-upstream-typst.md).

Because `to_uri` is the one place a file id becomes something the editor can open, diagnostics, jumps from
a preview click, go-to-definition, references and workspace symbols all address the buffer correctly with
no further change.

Three supporting rules follow from the buffer having no directory:

- **`..` in the name is refused, not normalized.** It would walk the path out of the reserved directory
  and land on a real project file, which the buffer would then shadow.
- **The compile root falls back to the temporary directory** when no workspace folder answers — the common
  case is a scratch buffer in a window with no folder open, and everything downstream assumes the root is
  a real absolute path it can build `file:` URIs from.
- **The "this compiles to no pages, pin a main file?" prompt does not fire for untitled buffers.** Nothing
  on disk can import a file that has no path, so a blank preview means "you have not typed anything yet".

## Consequences

**Buys.** A scratch buffer is a first-class document: it compiles, previews, exports, reports diagnostics
and jumps in both directions, and turns into a normal file on save with no special case — VSCode closes
the untitled document and opens the `file:` one, which the client already handles.

**Costs.** The mapping runs both ways, so a real checked-in `.typst-ultra/untitled/` directory would be
reported to the editor as untitled buffers. The name is chosen so that does not happen in practice, and
the behaviour is pinned by a test rather than left to be discovered.

Untitled buffers also appear in path completions offered inside other documents, under their reserved
path. Filtering them would mean teaching `typst-session` a convention that belongs to the URI layer, which
costs more than the wart.

**Hard.** Relative paths from an untitled buffer — `#import "helper.typ"`, `#image("cover.png")` — resolve
inside the reserved directory and fail. This is not a limitation of the mapping but of the document: a
buffer with no directory has nothing for a relative path to be relative *to*. Package imports and
root-absolute paths (`/assets/logo.svg`) both work, which covers what a scratch document reaches for.

## Revisit if

- Upstream typst gains a root for detached or in-memory sources, at which point the reserved directory and
  its shadowing cost stop being necessary.
- Readers hit the relative-import failure often enough that resolving untitled buffers against the
  compile root itself — which would make relative paths work at the price of colliding with real files —
  becomes the better trade.
