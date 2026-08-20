#!/usr/bin/env bash
#
# Runs once, after the container is created and after init-firewall.sh has
# already locked egress down. Everything below is untrusted code execution —
# that ordering is the point.
set -euo pipefail

echo "==> pnpm install"
# pnpm 10+ refuses to run a dependency's install scripts unless the package is
# listed under `allowBuilds` in pnpm-workspace.yaml. That list is currently
# @vscode/vsce-sign, esbuild and keytar; anything else is inert on install.
pnpm install --frozen-lockfile

echo "==> warming the cargo registry volume"
# `fetch` downloads and verifies checksums without running a single build
# script, so it is safe to do eagerly. The first real `cargo build` is where
# build.rs code runs.
cargo fetch --locked

echo
echo "==> toolchain"
printf '    rustc      %s\n' "$(rustc --version)"
printf '    cargo      %s\n' "$(cargo --version)"
printf '    wasm-pack  %s\n' "$(wasm-pack --version)"
printf '    node       %s\n' "$(node --version)"
printf '    pnpm       %s\n' "$(pnpm --version)"
printf '    targets    %s\n' "$(rustup target list --installed | tr '\n' ' ')"

cat <<'EOF'

Egress is default-deny. If a build fails on a network error, the host it wanted
is probably not on the allowlist — add it to ALLOWED_DOMAINS in
.devcontainer/init-firewall.sh and re-run:

    sudo /usr/local/sbin/init-firewall.sh

EOF
