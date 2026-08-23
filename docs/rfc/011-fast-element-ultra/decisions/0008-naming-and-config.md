# 0008 — `wx-vsce-fast-element-ultra`, `fastElementUltra.*`, and no migration from `fast-plugin.*`

**Status**: Accepted · **Date**: 2026-08-22

## Context

fast-analyzer publishes as `weixu.fast-plugin`, with settings under `fast-plugin.*` and a TypeScript
plugin named `ts-fast-plugin`. This repo's convention is different, and consistent across all 22
extensions:

| | Convention | Examples |
| --- | --- | --- |
| Package name | `wx-vsce-<slug>` | `wx-vsce-typst-ultra`, `wx-vsce-csv-ultra` |
| Display name | Title Case | "Typst Ultra", "CSV Ultra" |
| Directory | `extensions/<slug>` | `extensions/typst-ultra` |
| Settings | `camelCase` namespace | `typstUltra.*`, `csvUltra.*`, `markdownPreviewUltra.*` |

## Decision

| | Value |
| --- | --- |
| Package | `wx-vsce-fast-element-ultra` |
| Display name | FAST Element Ultra |
| Directory | `extensions/fast-element-ultra` |
| Settings namespace | `fastElementUltra.*` |
| Command | `fastElementUltra.analyze` |
| TS plugin package | `wx-fast-element-tsplugin` |
| Diagnostic source | `fast-element-ultra` |
| Rust crates | `crates/fast/fast-*` |

**No migration path from `fast-plugin.*`.** Settings are not read from the old namespace, rule ids
are not aliased, and the two extensions do not know about each other.

## Consequences

**A user moving from fast-analyzer re-does their settings.** There are fourteen of them, most
defaulted, and the rule ids have changed anyway
([0007](0007-fast-rule-semantics.md)) — three are gone and three are renamed, so a mechanical
migration would silently drop or misapply settings. Telling someone to reconfigure is more honest
than half-migrating them. The README carries a mapping table.

**Both can be installed at once**, and will both decorate the language service. tsserver composes
plugins by chaining decorations, so the result is the union of their diagnostics — which for someone
who genuinely has both lit and FAST in one workspace is what they want, and for anyone else is
duplicate squiggles they should resolve by uninstalling one. We do not attempt detection: an
extension that disables itself based on what else is installed is a support problem waiting to
happen.

**The TS plugin name is a public contract.** It appears in `contributes.typescriptServerPlugins`, in
users' `tsconfig.json` if they configure it there, and in tsserver's logs. It gets its own name
rather than sharing the extension's because they are separate npm-shaped packages living in the same
VSIX.

**`fast-element-ultra` as the diagnostic source** — the string in the Problems panel next to each
message. Long, and unambiguous, which matters when two analyzers may both be running.

## Revisit if

- Nothing here is load-bearing. If the repo's naming convention changes, this changes with it. The
  one thing that would be expensive to change later is the TS plugin package name, because it may
  appear in users' `tsconfig.json`.
