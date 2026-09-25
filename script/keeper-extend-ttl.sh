#!/usr/bin/env bash
#
# Perpetua keeper — automated rent maintenance.
#
# Keeps every active stream entry inside its TTL window by calling the
# contract's permissionless `batch_extend_ttl` in batched sweeps. Anyone can
# run it: the operation needs no auth and the contract skips unknown or
# archived ids without failing, so a keeper can sweep a superset of live ids
# safely.
#
# Ids are discovered two ways:
#   1. From a file (`IDS_FILE`), e.g. a dump of the indexer's stream table —
#      recommended for production, since it avoids a per-run scan and lets the
#      indexer's liveness state drive the sweep.
#   2. Fallback: read `stream_count()` and sweep the whole id range. Cheap for
#      small instance counts; a large instance should use the indexer file.
#
# Usage:
#   CONTRACT_ID=<id> SOURCE=<keeper-account> script/keeper-extend-ttl.sh
#   CONTRACT_ID=<id> IDS_FILE=./streams.txt script/keeper-extend-ttl.sh --once
#
# Environment:
#   CONTRACT_ID   Perpetua stream contract instance (required)
#   SOURCE        funded keypair that pays fees and sponsors rent (required)
#   IDS_FILE      path to a file of stream ids, one per line (optional)
#   NETWORK       stellar network alias, default testnet
#   RPC_URL       RPC endpoint, default testnet public
#   BATCH_SIZE    ids per batch_extend_ttl call, clamped to contract MAX_BATCH_SIZE
#
# Run periodically (cron/systemd timer). The contract targets a 30-day buffer,
# so a daily or even weekly sweep has ample headroom.

set -euo pipefail

CONTRACT_ID="${CONTRACT_ID:?CONTRACT_ID is required}"
SOURCE="${SOURCE:?SOURCE (keeper account) is required}"
NETWORK="${NETWORK:-testnet}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"
BATCH_SIZE="${BATCH_SIZE:-16}"
IDS_FILE="${IDS_FILE:-}"

DEFAULT_BATCH_SIZE=16
if [[ "$BATCH_SIZE" -gt "$DEFAULT_BATCH_SIZE" ]]; then
  BATCH_SIZE="$DEFAULT_BATCH_SIZE"
fi

say() { printf '\n\033[1m── %s\033[0m\n' "$*"; }

contract_invoke() {
  stellar contract invoke --id "$CONTRACT_ID" --source "$SOURCE" \
    --network "$NETWORK" --rpc-url "$RPC_URL" "$@"
}

# ---------------------------------------------------------------------------
# Discover the sweep target: indexer file, or fall back to the full id range.
# ---------------------------------------------------------------------------
if [[ -n "$IDS_FILE" ]]; then
  [[ -f "$IDS_FILE" ]] || { echo "IDS_FILE not found: $IDS_FILE" >&2; exit 1; }
  mapfile -t IDS < <(grep -E '^[0-9]+$' "$IDS_FILE" | sort -n -u)
  echo "Keeper sweep via IDS_FILE  : $IDS_FILE ($((${#IDS[@]})) ids)"
else
  COUNT=$(contract_invoke -- stream_count | tr -d '"')
  echo "Keeper sweep via stream_count: $COUNT ids"
  IDS=($(seq 0 $((COUNT - 1))))
fi

if (( ${#IDS[@]} == 0 )); then
  echo "Nothing to sweep."
  exit 0
fi

# ---------------------------------------------------------------------------
# Sweep in batches of BATCH_SIZE.
# ---------------------------------------------------------------------------
total=0
batches=0
for ((i = 0; i < ${#IDS[@]}; i += BATCH_SIZE)); do
  batch=("${IDS[@]:i:BATCH_SIZE}")
  batches=$((batches + 1))
  args=()
  for id in "${batch[@]}"; do
    args+=(--u64 "$id")
  done
  n=$(contract_invoke -- batch_extend_ttl "${args[@]}" | tr -d '"')
  total=$((total + n))
  printf '  batch %-3d ids %2d  extended %d\n' "$batches" "${#batch[@]}" "$n"
done

cat <<REPORT

╭───────────────────────────────────────────────────────────────╮
│ Perpetua keeper sweep complete                                │
│   streams swept    ${total}                      │
│   batches used     ${batches}                      │
╰───────────────────────────────────────────────────────────────╯
Each stream entry now carries ~30 days of rent headroom. Re-run on a schedule.
REPORT