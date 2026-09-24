/**
 * Thin read-only client over the deployed Perpetua stream contract.
 *
 * This is the reference integration layer the hooks sit on top of. It talks to
 * a Soroban RPC endpoint via `@stellar/stellar-sdk` and decodes the contract's
 * ABI-shaped `Stream` struct (see `contracts/stream/abi/fluxora_stream.json`)
 * into the typed [`Stream`] used by [`useStream`] and [`useAccruedBalance`].
 *
 * Nothing here mutates state — it is the read side only. For authenticated
 * writes (`withdraw`, `cancel`, `pause`, ...) consumers should build operations
 * against the same `Contract` handle; see the package README for an example.
 */

import {
  Contract,
  SorobanRpc,
  StrKey,
  nativeToScVal,
  scValToNative,
  xdr,
} from '@stellar/stellar-sdk';

import { Stream, StreamStatus } from './types.js';

/** Read-only operations the hooks need. Swap for a mock in tests. */
export interface PerpetuaClient {
  readonly rpcUrl: string;
  readonly contractId: string;
  /** Decode the full stream record for `id`. */
  getStream(id: bigint): Promise<Stream>;
  /** Total earned by the recipient since `start_time`, withdrawn or not. */
  vestedOf(id: bigint): Promise<bigint>;
  /** Amount the recipient could withdraw right now. */
  withdrawableOf(id: bigint): Promise<bigint>;
  /** Amount the sender would get back on an immediate cancel. */
  refundableOf(id: bigint): Promise<bigint>;
  /** Number of streams ever created. */
  streamCount(): Promise<bigint>;
  /** Current ledger close time in uint64 Unix seconds. */
  ledgerTimestamp(): Promise<bigint>;
}

export interface PerpetuaClientOptions {
  /** e.g. `https://soroban-testnet.stellar.org` */
  rpcUrl: string;
  /** The deployed `fluxora_stream` contract address. */
  contractId: string;
}

function addressFromScAddress(sc: xdr.ScAddress): string {
  if (sc.switch() === xdr.ScAddressType.scAddressTypeAccount()) {
    const account = sc.accountId();
    return StrKey.encodeEd25519PublicKey(account.ed25519());
  }
  return StrKey.encodeContract(sc.contractId());
}

function scalar(val: xdr.ScVal): bigint | number | boolean | string | null {
  return scValToNative(val) as bigint | number | boolean | string | null;
}

function toBigInt(value: bigint | number | boolean | string | null): bigint {
  if (typeof value === 'bigint') return value;
  if (value === null || value === undefined) return 0n;
  return BigInt(String(value));
}

function asStream(val: xdr.ScVal, id: bigint): Stream {
  const map = val.map();
  const field = (name: string): xdr.ScVal | undefined => {
    for (const entry of map.entries()) {
      if (scalar(entry.key()) === name) return entry.val();
    }
    return undefined;
  };
  const num = (name: string): bigint => toBigInt(scalar(field(name)!));
  const bool_ = (name: string): boolean => Boolean(scalar(field(name)!));
  const addr = (name: string): string => addressFromScAddress(field(name)!.address());
  const opt = (name: string): bigint | null => {
    const raw = field(name);
    if (!raw) return null;
    const native = scalar(raw);
    return native === null ? null : toBigInt(native);
  };
  const status = (): StreamStatus => {
    const n = Number(scalar(field('status')!));
    return [0, 1, 2, 3].includes(n) ? (n as StreamStatus) : StreamStatus.Active;
  };

  return {
    id,
    sender: addr('sender'),
    recipient: addr('recipient'),
    token: addr('token'),
    deposited: num('deposited'),
    withdrawn: num('withdrawn'),
    start_time: num('start_time'),
    end_time: num('end_time'),
    cliff_time: num('cliff_time'),
    cancellable: bool_('cancellable'),
    pausable: bool_('pausable'),
    transferable: bool_('transferable'),
    paused_at: opt('paused_at'),
    paused_total: num('paused_total'),
    status: status(),
  };
}

/** Production client backed by a Soroban RPC endpoint. */
export class SorobanPerpetuaClient implements PerpetuaClient {
  readonly rpcUrl: string;
  readonly contractId: string;
  private readonly server: SorobanRpc.Server;
  private readonly contract: Contract;

  constructor(opts: PerpetuaClientOptions) {
    this.rpcUrl = opts.rpcUrl;
    this.contractId = opts.contractId;
    this.server = new SorobanRpc.Server(opts.rpcUrl);
    this.contract = new Contract(opts.contractId, this.server);
  }

  async getStream(id: bigint): Promise<Stream> {
    const result = (await this.contract.call('get_stream', nativeToScVal(id))) as unknown as xdr.ScVal;
    return asStream(result, id);
  }

  async vestedOf(id: bigint): Promise<bigint> {
    return toBigInt(await this.contract.call('vested_of', nativeToScVal(id)));
  }

  async withdrawableOf(id: bigint): Promise<bigint> {
    return toBigInt(await this.contract.call('withdrawable_of', nativeToScVal(id)));
  }

  async refundableOf(id: bigint): Promise<bigint> {
    return toBigInt(await this.contract.call('refundable_of', nativeToScVal(id)));
  }

  async streamCount(): Promise<bigint> {
    return toBigInt(await this.contract.call('stream_count'));
  }

  async ledgerTimestamp(): Promise<bigint> {
    const latest = await this.server.getLatestLedger();
    return BigInt(latest.timestamp);
  }
}

/** Construct a [`SorobanPerpetuaClient`]. */
export function createPerpetuaClient(opts: PerpetuaClientOptions): PerpetuaClient {
  return new SorobanPerpetuaClient(opts);
}