#!/usr/bin/env bash
# Verify WASM build reproducibility by checking SHA256 checksums.
# Usage: bash script/verify-wasm-checksum.sh [--no-build]

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STREAM_DIR="$REPO_ROOT/contracts/stream"
WASM_DIR="$STREAM_DIR/target/wasm32-unknown-unknown/release"
SHA256_FILE="$WASM_DIR/fluxora_stream.wasm.sha256"
OPT_SHA256_FILE="$WASM_DIR/fluxora_stream.optimized.wasm.sha256"

NO_BUILD=false
if [[ "${1:-}" == "--no-build" ]]; then
    NO_BUILD=true
fi

if [[ "$NO_BUILD" == "false" ]]; then
    echo "Building WASM..."
    cd "$STREAM_DIR"
    cargo build --release --target wasm32-unknown-unknown
fi

echo "Verifying WASM SHA256 checksums..."

if [[ ! -f "$SHA256_FILE" ]]; then
    echo "ERROR: WASM checksum file not found at $SHA256_FILE"
    echo "Run 'sha256sum contracts/stream/target/wasm32-unknown-unknown/release/fluxora_stream.wasm > ...sha256' first."
    exit 1
fi

# Verify the WASM file matches its checksum
cd "$WASM_DIR"
if sha256sum -c fluxora_stream.wasm.sha256; then
    echo "OK: fluxora_stream.wasm checksum verified."
else
    echo "FAIL: fluxora_stream.wasm checksum mismatch."
    exit 1
fi

# Optionally verify optimized WASM
if [[ -f "$OPT_SHA256_FILE" ]]; then
    if sha256sum -c fluxora_stream.optimized.wasm.sha256; then
        echo "OK: fluxora_stream.optimized.wasm checksum verified."
    else
        echo "FAIL: fluxora_stream.optimized.wasm checksum mismatch."
        exit 1
    fi
else
    echo "INFO: No optimized WASM checksum file found, skipping."
fi

echo "OK: All WASM checksums verified."
