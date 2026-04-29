# License

This VSIX is distributed under the **GNU General Public License,
version 2 or later (GPL-2.0-or-later)**.

The extension's own source code (TypeScript under `src/`, the analyzer
crate, configuration) was written for this project. The compiled
binary that ships in this VSIX, however, links the
`cli-completions-data-fish` crate, which embeds a snapshot of
[fish-shell](https://github.com/fish-shell/fish-shell) completion data.
fish-shell is GPL-2-or-later, and including its data makes the combined
binary distribution a derivative work — hence the GPL-2-or-later terms
on the VSIX as a whole.

For the full text of the license, see
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md), which links to the
GPL-2 text bundled with the data crate.

See RFC 005 §11 in this repository for the design rationale.
