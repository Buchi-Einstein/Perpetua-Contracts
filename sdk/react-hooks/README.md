# @perpetua/react-hooks

Reference React hooks for building payroll, grant and vesting interfaces on
[Perpetua](https://github.com/releaseken/Perpetua-Contracts) continuous payment
streams.

Two hooks do the heavy lifting:

- **`useStream`** — loads one stream by id from the ledger (polling optional).
- **`useAccruedBalance`** — a **continuous, second-by-second** balance that
  recomputes locally between ledger reads, so a UI shows the money moving the
  way it does on chain, without hammering RPC.

The accrual math is a BigInt port of the contract's own
`contracts/stream/src/accrual.rs`, so the locally-displayed number follows the
exact on-chain rounding rules (round down, stream clock stops while paused,
cliff gates but does not delay).

## Install

```bash
npm install @stellar/stellar-sdk react
npm install @perpetua/react-hooks
```

## Quick start

```tsx
import {
  PerpetuaProvider,
  useAccruedBalance,
  useStream,
} from '@perpetua/react-hooks';

function App() {
  return (
    <PerpetuaProvider
      rpcUrl="https://soroban-testnet.stellar.org"
      contractId="CBCGTSCJXBMPPPE4BPDIPYZXPE2J5TQEKD2KCS7VQF533NKKEYGUTHXW"
    >
      <VestingWidget streamId={7n} />
    </PerpetuaProvider>
  );
}

function VestingWidget({ streamId }: { streamId: bigint }) {
  // Second-by-second balance, refreshed from the chain every 60s.
  const {
    stream,
    vested,
    withdrawable,
    refundable,
    rate,
    paused,
    terminal,
    loading,
    error,
  } = useAccruedBalance(streamId, { refreshIntervalMs: 60_000 });

  if (loading) return <p>Loading stream #{streamId.toString()}…</p>;
  if (error || !stream) return <p>Failed to load: {error?.message}</p>;

  // The capability flags are immutable — issue #104. Read them once, trust them.
  const { cancellable, pausable, transferable } = stream;

  return (
    <dl>
      <dt>Streaming</dt>
      <dd>
        {rate} stroops/s {paused ? '(paused)' : ''} {terminal ? '(settled)' : ''}
      </dd>
      <dt>Vested</dt>
      <dd>{vested.toString()}</dd>
      <dt>Withdrawable now</dt>
      <dd>{withdrawable.toString()}</dd>
      <dt>Refundable if cancelled</dt>
      <dd>{refundable.toString()}</dd>
      <dt>Flags</dt>
      <dd>
        cancellable={String(cancellable)} pausable={String(pausable)}{' '}
        transferable={String(transferable)}
      </dd>
    </dl>
  );
}
```

## API

### `<PerpetuaProvider rpcUrl contractId>`

Provides a read client to every hook below. `rpcUrl` is a Soroban RPC endpoint
(or fake network); `contractId` is the deployed `fluxora_stream` address.

### `usePerpetua()`

Returns the configured `PerpetuaClient` (see `src/rpc.ts`). Throws if there is
no provider.

### `useStream(id, opts?)`

| Option | Default | Meaning |
|---|---|---|
| `refreshIntervalMs` | off | re-poll the ledger at this cadence |
| `enabled` | `true` | pause the subscription |

Returns `{ stream, loading, error, refresh }`. `stream` is null until the first
successful load and retains the last good value across later failures.

### `useAccruedBalance(id, opts?)`

| Option | Default | Meaning |
|---|---|---|
| `refreshIntervalMs` | `30_000` | re-pin stream + ledger clock from the chain |
| `tickMs` | `1000` | local recompute cadence |
| `enabled` | `true` | pause the whole subscription |

Returns `{ stream, vested, withdrawable, refundable, rate, status, paused,
terminal, now, loading, error, refresh }`. All amounts are **stroops**
(`bigint`); divide by the token's decimals (USDC on Stellar: **7**) for display.

## Client-side math vs on-chain truth

`useAccruedBalance` is a *simulation* between polls: it assumes the ledger
clock tracks wall time, which it does to well under a poll window on Stellar.
The authoritative number is whatever `withdrawable_of(streamId)` reports
through `useStream`'s refresh — the local value and that number converge at
each poll. For display this is far smoother than polling; for a *payout*, call
the contract.

## Withdrawing

The read hooks never sign. To actually withdraw, build an operation against the
same contract address with `@stellar/stellar-sdk`:

```ts
import { Contract, SorobanRpc, nativeToScVal } from '@stellar/stellar-sdk';

const server = new SorobanRpc.Server(rpcUrl);
const contract = new Contract(contractId, server);
// Simulate to get the exact withdrawable, then submit with the recipient's
// signer. Auth: the recipient (or a delegate with op::WITHDRAW).
const op = contract.call('withdraw', nativeToScVal(streamId), null); // None = max
```

## Tests

```bash
npm test            # vitest — accrual port conformance
npm run build       # tsc → dist/
```

## Reference status

This is the reference implementation for issue #106. It intentionally depends
only on `@stellar/stellar-sdk` + React, so the hooks are decodable and
auditable; the generated TypeScript SDK will later replace the manual `Stream`
decoder in `src/rpc.ts` with spec-driven parsing, keeping this API surface.