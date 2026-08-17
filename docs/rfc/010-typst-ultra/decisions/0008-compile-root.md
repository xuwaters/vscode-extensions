# 0008 — Follow the focused file by default; pin a main file for projects

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 6

## Context

Typst compiles from a single entry file. Everything else — diagnostics, the preview, label completions,
find-references — is derived from compiling that entry. So "which file is the entry?" is not a detail; it
determines whether the extension is useful.

Two user populations want opposite answers:

- **Single-file / getting started.** One `.typ` open. The entry is obviously the file you are looking at.
  Requiring configuration before anything works is a bad first five minutes.
- **Large projects.** `main.typ` imports `chapters/*.typ`. Editing `chapters/03.typ` and compiling *it* is
  useless: it has no `#set page`, its `#import`s resolve differently, and its cross-references dangle. You
  need `main.typ` compiled while you edit chapter 3.

Tinymist eventually needed a project manifest (`tinymist.lock`) for this. That is out of scope
([proposal.md §2](../proposal.md#2-goals-and-non-goals)), but the underlying need is real.

Note this is distinct from **root path** (`typstUltra.rootPath`), which decides what an absolute typst path
like `/assets/logo.svg` resolves against and defaults to the workspace folder. Root path and main file are
independent settings.

## Decision

**Support both, with a resolution chain and a visible indicator.** The entry file is the first of:

| Precedence | Source | Scope | Set by |
| --- | --- | --- | --- |
| 1 | Session pin | `workspaceState` | `typstUltra.pinMain` command |
| 2 | `typstUltra.mainFile` setting | workspace / folder | `.vscode/settings.json`, checked in by a team |
| 3 | Focused `.typ` editor | — | default, no configuration |

Rationale for the ordering: the setting is the project's shared answer; the command is a user's explicit
in-session override, so it wins. `typstUltra.unpinMain` clears the pin and falls back to the setting.

### Making the mode visible and switchable

A status-bar item is the discoverability mechanism, because a silently-wrong compile root is the failure
mode that generates confused bug reports:

```
$(eye)  chapter-03.typ          following        ← mode 3
$(pin)  main.typ                pinned           ← mode 1 or 2
```

Clicking it opens a QuickPick: *Pin this file* · *Pin another file…* · *Unpin* · *Open compile root*.

### Bridging the two modes automatically

The gap between the two populations is crossed by one prompt. The server knows the compile graph, so when
the user focuses a `.typ` file that is **not** the entry but **is** reachable from a plausible root, the
extension offers once per workspace:

> `chapters/03.typ` is included by `main.typ`. Pin `main.typ` as the compile root?
> **[Pin] [Not now] [Never for this workspace]**

Root candidates are found by scanning workspace `.typ` files for ones that import/include the focused file
and are not themselves imported by anything. Cheap, and it only runs on the "focused file is not a root"
path.

### When the focused file is outside the pinned project

Editing an unrelated `.typ` while `main.typ` is pinned produces no diagnostics for it, which is
confusing without an explanation. The status bar shows `$(pin) main.typ — 03.typ not in project`, and the
QuickPick offers pinning the focused file instead. We deliberately do **not** run a second standalone
compile: that doubles memory and compile time to serve an uncommon case.

Multi-root workspaces resolve the entry per workspace folder.

## Consequences

**Buys.** Zero-configuration for the common case, and a correct model for real projects, with a documented
migration path between them.

**Costs.** More state than "compile what's focused": a status-bar item, a QuickPick, session state in
`workspaceState`, a settings key, and the root-candidate scan. This is the largest piece of pure UX
machinery in the extension, and it lands in Phase 2 rather than Phase 1 — Phase 1 ships mode 3 only.

**Risk.** The auto-suggest prompt is the kind of thing that becomes annoying if it misfires. It is
one-shot per workspace, has a permanent dismissal, and only triggers when the focused file is provably
imported by another file.

## Revisit if

- Projects appear that genuinely need more than one entry (e.g. a book plus its standalone chapters built
  separately), which is the point at which a manifest — the thing `tinymist.lock` exists for — stops being
  avoidable.
- The auto-suggest prompt proves annoying rather than helpful; the fallback is to drop it and rely on the
  status bar alone.
