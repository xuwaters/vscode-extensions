# syntax=docker/dockerfile:1

# Rust stable, because that is what CI builds with
# (.github/workflows/ci.yml pins dtolnay/rust-toolchain@stable). The host
# toolchain here is nightly; if a build works in the container it will work in
# CI, which is the more useful guarantee of the two.
FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Kept in sync by hand with package.json's `packageManager` field and the
# `engines.node` floor. Bump these together.
ARG NODE_VERSION=24.17.0
ARG PNPM_VERSION=11.20.0
ARG WASM_PACK_VERSION=0.15.0

# ipset/iptables/iproute2 back init-firewall.sh. dnsutils resolves the
# allowlist. pkg-config and libssl-dev are wasm-pack's build dependencies.
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
 && apt-get install -y --no-install-recommends \
      ipset iptables iproute2 dnsutils pkg-config libssl-dev jq \
 && rm -rf /var/lib/apt/lists/*

# Node from the official tarball rather than a devcontainer Feature: a Feature
# is a script fetched from a registry and run as root at build time, which is
# the same class of thing this container exists to contain.
RUN set -eux; \
    case "$(dpkg --print-architecture)" in \
      amd64) node_arch=x64 ;; \
      arm64) node_arch=arm64 ;; \
      *) echo "unsupported architecture: $(dpkg --print-architecture)" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" \
      | tar -xJ -C /usr/local --strip-components=1 --no-same-owner; \
    node --version

# corepack rather than `npm i -g pnpm`, because it honours the sha512 integrity
# hash already recorded in package.json's `packageManager` field.
RUN corepack enable && corepack prepare "pnpm@${PNPM_VERSION}" --activate

# Built from source instead of a release tarball: upstream does not publish an
# aarch64-linux binary, and this image has to work on Apple Silicon. Costs a few
# minutes once, at image build.
RUN cargo install wasm-pack --locked --version "${WASM_PACK_VERSION}"

# clippy is what CI lints with. rustfmt is deliberately absent: the Rust crates
# in this repo are hand-formatted and there is no rustfmt.toml, so a stray
# `cargo fmt` would rewrite every file.
RUN rustup target add wasm32-unknown-unknown \
 && rustup component add clippy

# Docker seeds an empty named volume from the image's contents *and ownership*
# at the mount point, so creating these here is what lets the cache volumes in
# devcontainer.json come up writable without a chown at startup — which in turn
# is what lets the sudoers rule below stay as narrow as it is.
RUN mkdir -p /usr/local/cargo/registry /home/vscode/.cache/cargo-target \
 && chown -R vscode:vscode /usr/local/cargo/registry /home/vscode/.cache

# The firewall script is root-owned and not writable by the user it protects.
COPY --chown=root:root --chmod=0755 init-firewall.sh /usr/local/sbin/init-firewall.sh

# The base image ships passwordless sudo for everything; replace it with a rule
# that grants exactly one command. A build script that runs as `vscode` then has
# no way to take down the egress allowlist that is containing it.
#
# To get a normal `sudo` back (for `apt-get install`, say), delete this RUN line
# and rebuild — but understand that it hands any build script root, too.
RUN rm -f /etc/sudoers.d/vscode /etc/sudoers.d/1000 \
 && echo 'vscode ALL=(root) NOPASSWD: /usr/local/sbin/init-firewall.sh' \
      > /etc/sudoers.d/init-firewall \
 && chmod 0440 /etc/sudoers.d/init-firewall \
 && visudo -c
