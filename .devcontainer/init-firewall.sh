#!/usr/bin/env bash
#
# Default-deny egress for the dev container.
#
# The threat this answers is the one `arrayref` 0.3.10 demonstrated in November
# 2025: a crate's build.rs runs during `cargo build`, downloads a second stage
# from a hardcoded address, and executes it. Nothing about that needs the
# program to be run, so "I only compiled it" is not a defence. What does defend
# against it is that the dropper dials a bare IP on a high port, and this script
# makes every destination unreachable except a short list of package registries.
#
# Re-run it any time a registry's CDN rotates its addresses:
#     sudo /usr/local/sbin/init-firewall.sh
set -euo pipefail
IFS=$'\n\t'

ALLOWED_DOMAINS=(
  # Rust toolchain and crate registry
  static.rust-lang.org
  crates.io
  index.crates.io
  static.crates.io
  # Node and pnpm
  registry.npmjs.org
  # GitHub: git-sourced crate dependencies, plus the wasm-bindgen and wasm-opt
  # binaries that wasm-pack fetches from Releases the first time it runs.
  github.com
  api.github.com
  codeload.github.com
  objects.githubusercontent.com
  raw.githubusercontent.com
  # Only `vsce publish` needs this; `vsce package` is offline.
  marketplace.visualstudio.com
)

echo "==> flushing existing rules"
iptables -F
iptables -X
iptables -t nat -F
iptables -t nat -X
iptables -t mangle -F
iptables -t mangle -X

# ipset keeps this to one rule instead of one rule per address, but Docker
# Desktop's kernel does not always carry the module — fall back to plain rules.
use_ipset=1
if ipset create allowed-domains hash:net -exist 2>/dev/null; then
  ipset flush allowed-domains
else
  echo "==> ipset unavailable, falling back to individual iptables rules"
  use_ipset=0
fi

echo "==> allowing loopback, DNS, and established flows"
iptables -A INPUT  -i lo -j ACCEPT
iptables -A OUTPUT -o lo -j ACCEPT
iptables -A OUTPUT -p udp --dport 53 -j ACCEPT
iptables -A OUTPUT -p tcp --dport 53 -j ACCEPT
iptables -A INPUT  -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
iptables -A OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT

# The VS Code server talks to the editor on the host across this subnet. Without
# this the window disconnects the moment the policy flips to DROP.
host_subnet="$(ip -o -f inet addr show eth0 | awk '{print $4}')"
echo "==> allowing the host link ($host_subnet)"
iptables -A INPUT  -s "$host_subnet" -j ACCEPT
iptables -A OUTPUT -d "$host_subnet" -j ACCEPT

echo "==> resolving the allowlist"
for domain in "${ALLOWED_DOMAINS[@]}"; do
  addrs="$(dig +short A "$domain" | grep -E '^[0-9]+(\.[0-9]+){3}$' || true)"
  if [[ -z "$addrs" ]]; then
    echo "    warning: no A record for $domain — skipping"
    continue
  fi
  echo "    $domain -> $(echo "$addrs" | tr '\n' ' ')"
  while read -r addr; do
    if (( use_ipset )); then
      ipset add allowed-domains "$addr" -exist
    else
      iptables -A OUTPUT -d "$addr" -j ACCEPT
    fi
  done <<< "$addrs"
done

if (( use_ipset )); then
  iptables -A OUTPUT -m set --match-set allowed-domains dst -j ACCEPT
fi

echo "==> setting default-deny policies"
iptables -P INPUT DROP
iptables -P OUTPUT DROP
iptables -P FORWARD DROP

# Prove both halves rather than trusting the ruleset by inspection. The address
# below is the arrayref campaign's payload host; it stands in for "anywhere the
# allowlist does not mention".
echo "==> verifying"
if curl -s --max-time 5 --connect-timeout 5 https://23.254.165.112:9089 >/dev/null 2>&1; then
  echo "    FAIL: reached a host that should be blocked" >&2
  exit 1
fi
echo "    blocked: 23.254.165.112:9089"

if ! curl -s --max-time 10 https://index.crates.io/config.json >/dev/null 2>&1; then
  echo "    FAIL: crates.io is unreachable, builds will not work" >&2
  exit 1
fi
echo "    reachable: index.crates.io"

echo "==> egress allowlist active"
