#!/usr/bin/env bash
# Verify WASM build reproducibility by checking SHA256 checksums.
# Usage: bash script/verify-wasm-checksum.sh [--no-build]

set -euo pipefail

# ANSI color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STREAM_DIR="$REPO_ROOT/contracts/stream"
WASM_DIR="$STREAM_DIR/target/wasm32v1-none/release"
SHA256_FILE="$WASM_DIR/fluxora_stream.wasm.sha256"
OPT_SHA256_FILE="$WASM_DIR/fluxora_stream.optimized.wasm.sha256"

NO_BUILD=false
if [[ "${1:-}" == "--no-build" ]]; then
    NO_BUILD=true
fi

if [[ "$NO_BUILD" == "false" ]]; then
    echo -e "${GREEN}Building WASM...${NC}"
    cd "$STREAM_DIR"
    cargo build --release --target wasm32v1-none
fi

echo -e "${GREEN}Verifying WASM SHA256 checksums...${NC}"

if [[ ! -f "$SHA256_FILE" ]]; then
    echo -e "${RED}ERROR: WASM checksum file not found at $SHA256_FILE${NC}"
    echo "Run 'sha256sum contracts/stream/target/wasm32v1-none/release/fluxora_stream.wasm > ...sha256' first."
    exit 1
fi

# Verify the WASM file matches its checksum
cd "$WASM_DIR"
if sha256sum -c fluxora_stream.wasm.sha256; then
    echo -e "${GREEN}OK: fluxora_stream.wasm checksum verified.${NC}"
else
    echo -e "${RED}FAIL: fluxora_stream.wasm checksum mismatch.${NC}"
    exit 1
fi

# Optionally verify optimized WASM
if [[ -f "$OPT_SHA256_FILE" ]]; then
    if sha256sum -c fluxora_stream.optimized.wasm.sha256; then
        echo -e "${GREEN}OK: fluxora_stream.optimized.wasm checksum verified.${NC}"
    else
        echo -e "${RED}FAIL: fluxora_stream.optimized.wasm checksum mismatch.${NC}"
        exit 1
    fi
else
    echo -e "${YELLOW}INFO: No optimized WASM checksum file found, skipping.${NC}"
fi

echo -e "${GREEN}OK: All WASM checksums verified.${NC}"
