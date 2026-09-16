#!/usr/bin/env bash
#
# Release command — produces ONLY the product contract artifact.
#
# The repo contains four Soroban contracts, each its own standalone Cargo
# project with its own lockfile (no shared workspace):
#
#   contracts/stream          -> fluxora_stream.wasm           (the product)
#   contracts/archival-probe  -> fluxora_archival_probe.wasm   (throwaway, NOT product)
#   contracts/factory         -> fluxora_factory.wasm          (policy factory)
#   contracts/governance      -> fluxora_governance.wasm       (multi-sig governance)
#
# The archival probe is described in its own manifest as "NOT part of the product"
# and exists only to prove the live-network archival/restore round trip that the
# unit suite structurally cannot (see KNOWN-LIMITATIONS.md §1). It must never be
# deployed to mainnet or shipped as a release artifact.
#
# Because the contracts are isolated projects, there is no shared output
# directory for a probe wasm to accidentally leak into. The build runs inside
# `contracts/stream` and this command verifies that its release output contains
# only the product artifact and nothing else that would be deployable.
#
# This release command builds ONLY the products that ship (currently the stream
# contract) and then asserts that no other contract artifact is present in the
# output. It is the single entry point a release/publish pipeline (or a human)
# uses to obtain deployable artifacts.
#
# Usage:
#   script/release.sh
#
# Output:
#   <repo>/contracts/stream/target/wasm32v1-none/release/fluxora_stream.wasm
#
# The probe, factory, and governance contracts remain independently buildable
# and testable (`cargo build`, `cargo test` inside their own directories; see
# .github/workflows/ci.yml) and are archived out of release pipelines by not
# being built here at all.

set -euo pipefail

TARGET="wasm32v1-none"
PROFILE="release"
PRODUCT_WASM="fluxora_stream.wasm"
STREAM_DIR="contracts/stream"

cd "$(dirname "$0")/.."

say() { printf '\n\033[1m── %s\033[0m\n' "$*"; }

say "1. build the product artifact only"
(
  cd "$STREAM_DIR"
  cargo build --target "$TARGET" --profile "$PROFILE"
)

OUT="$STREAM_DIR/target/$TARGET/$PROFILE"
PRODUCT="$OUT/$PRODUCT_WASM"

say "2. verify only the product artifact is present"
if [[ ! -f "$PRODUCT" ]]; then
  echo "   ✗ product artifact missing: $PRODUCT" >&2
  exit 1
fi
# With a standalone stream project nothing else lands in its output dir, but the
# guard stays so a future "every contract its own wasm" regression is caught.
for other in "$OUT"/*.wasm; do
  [[ -e "$other" ]] || break
  name="$(basename "$other")"
  if [[ "$name" != "$PRODUCT_WASM" ]]; then
    echo "   ✗ unexpected artifact in release output: $name" >&2
    echo "     Releases must contain only the product contract." >&2
    exit 1
  fi
done

say "3. done"
printf '   \033[32m✓\033[0m %s\n' "$PRODUCT"
printf '   \033[32m✓\033[0m release artifacts contain only the product contract\n'