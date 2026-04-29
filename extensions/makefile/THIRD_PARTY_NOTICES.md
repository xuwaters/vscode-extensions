# Third-Party Notices

This VSIX bundles or links third-party software. Their license terms
and required attributions are reproduced below.

---

## fish-shell

The `cli-completions-data-fish` crate embeds a snapshot of completion
data extracted from [fish-shell](https://github.com/fish-shell/fish-shell).
fish-shell is licensed under the **GNU General Public License, version
2 or later (GPL-2.0-or-later)**.

- Copyright © 2005–2009 Axel Liljencrantz
- Copyright © 2009– fish-shell contributors

The full text of the license, plus fish-shell's own COPYING notice, is
shipped alongside the data crate at:

- `crates/cli-completions-data-fish/LICENSE` — GPL-2.0 text
- `crates/cli-completions-data-fish/LICENSE-fish` — fish-shell COPYING

Source for fish-shell is publicly available at the upstream URL above.
The exact snapshot commit and date are recorded in
`crates/cli-completions-data-fish/data/fish-snapshot.toml` (when the
snapshot is vendored locally).

Because the compiled VSIX includes data derived from fish-shell, the
combined work is itself distributed under GPL-2.0-or-later. See
`LICENSE.md` and RFC 005 §11 for design rationale.

---

## Rust crates

The shipped wasm bundle statically links Rust crates from this
repository (`makefile-analyzer`, `cli-completions`,
`cli-completions-data-fish`) and from third-party sources resolved
via Cargo. License information for transitive dependencies is
available in the lockfile at the repository root and via `cargo about`
or equivalent tooling at build time.
