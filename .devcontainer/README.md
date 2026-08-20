# Dev container

A sandbox for building this repo. It exists because `cargo build` and
`pnpm install` both execute third-party code on this machine — `build.rs`
scripts and npm lifecycle scripts — and the `arrayref` 0.3.10 compromise showed
what that costs when a registry account is taken over.

## Using it

Open the repo in VS Code and run **Dev Containers: Reopen in Container**. First
build takes a few minutes, mostly compiling `wasm-pack`. After that:

```
pnpm build          # every extension
pnpm test           # vitest, everywhere
cargo test --workspace
cargo clippy --all-targets -- -D warnings -p typst-session -p typst-lsp-core \
    -p typst-preview-core -p typst-lsp-wasm
```

`F5` extension-host debugging works from inside the container. No display stack
is installed because every test here is vitest, not `@vscode/test-electron`.

## What is contained

- **Egress is default-deny.** `init-firewall.sh` allowlists crates.io, npm,
  GitHub, static.rust-lang.org and the VS Code marketplace, and drops everything
  else. A dropper that dials `23.254.165.112:9089` gets nowhere. The script
  verifies both halves on every start and fails loudly if either is wrong.
- **No blanket `sudo`.** The base image's passwordless-sudo rule is removed and
  replaced with one that permits `init-firewall.sh` and nothing else, so code
  running as `vscode` cannot take down the allowlist containing it.
- **No credentials.** `SSH_AUTH_SOCK` is cleared. Push and pull from the host.
- **Caches are separate from the host's.** The container's cargo registry and
  target dir are named volumes, so nothing it downloads lands in `~/.cargo` on
  the Mac.

## Two settings the container cannot set for itself

Dev Containers copies your `.gitconfig` and forwards a git credential helper
into the container by default, and `devcontainer.json` has no key to refuse it.
Turn both off in your **user** `settings.json`:

```jsonc
"dev.containers.copyGitConfig": false,
"dev.containers.gitCredentialHelperConfigLocation": "none"
```

## Adjusting

New network dependency: add the host to `ALLOWED_DOMAINS` in
`init-firewall.sh`, then `sudo /usr/local/sbin/init-firewall.sh`. Registry CDNs
rotate addresses, so that command is also the fix for a build that suddenly
cannot reach a host it reached yesterday — though a container restart does it
too.

Toolchain versions are `ARG`s at the top of the `Dockerfile`. `NODE_VERSION` and
`PNPM_VERSION` are kept in sync by hand with `engines.node` and
`packageManager` in the root `package.json`.

`rustfmt` is deliberately not installed: the Rust crates here are hand-formatted
and carry no `rustfmt.toml`, so `cargo fmt` would rewrite every file.
