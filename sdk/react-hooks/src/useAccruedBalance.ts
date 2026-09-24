import { useEffect, useMemo, useRef, useState } from 'react';
import { perSecondRate, refundable, vested, withdrawable } from './accrual.js';
import type { Stream, StreamStatus } from './types.js';
import { usePerpetua } from './usePerpetua.js';
import { useStream } from './useStream.js';

export interface UseAccruedBalanceOptions {
  /**
   * Re-pin the stream and its ledger timestamp from the chain. 30s is a sane
   * default; the local math keeps the number moving between polls.
   */
  refreshIntervalMs?: number;
  /** Local recompute cadence in milliseconds. Defaults to 1000 (per second). */
  tickMs?: number;
  /** Set `false` to suspend the whole subscription. */
  enabled?: boolean;
}

export interface UseAccruedBalanceResult {
  /** The underlying stream snapshot the balance comes from. */
  stream: Stream | null;
  /**
   * Total earned since `start_time` at the current instant, withdrawn or not.
   * Computed locally, second by second, without hitting the ledger.
   */
  vested: bigint;
  /** Amount the recipient can withdraw right now. Stops growing while paused. */
  withdrawable: bigint;
  /** Amount the sender would get back on an immediate cancel. */
  refundable: bigint;
  /** Stroops per second flowing; zero for a settled/cancelled stream. */
  rate: bigint;
  status: StreamStatus | null;
  /** Whether accrual is currently frozen by a pause. */
  paused: boolean;
  /** True when the stream has reached a terminal (Cancelled/Depleted) state. */
  terminal: boolean;
  /** Ledger-time estimate the values were computed at. */
  now: bigint;
  loading: boolean;
  error: Error | null;
  /** Re-pin stream + ledger timestamp immediately. */
  refresh: () => Promise<void>;
}

/**
 * Real-time accrued balance for a Perpetua stream.
 *
 * The contract settles every drip at the instant it is claimed, so between RPC
 * polls the balance can only be *computed*, not read. This hook ports the
 * contract's accrual model (`contracts/stream/src/accrual.rs`) to BigInt and
 * re-evaluates it on a local timer, giving payroll and vesting UIs a smooth
 * second-by-second number with only occasional chain reads.
 *
 * Pause semantics are honoured exactly: while `paused_at` is set the stream
 * clock freezes, so `withdrawable` stops growing until the stream is resumed
 * and refetched.
 */
export function useAccruedBalance(
  streamId: bigint,
  opts: UseAccruedBalanceOptions = {},
): UseAccruedBalanceResult {
  const { refreshIntervalMs = 30_000, tickMs = 1000, enabled = true } = opts;
  const client = usePerpetua();

  const [ledgerNow, setLedgerNow] = useState<bigint>(0n);
  const baseRef = useRef<bigint>(0n);
  const baseWallMsRef = useRef<number>(Date.now());

  const { stream, loading, error, refresh } = useStream(streamId, {
    refreshIntervalMs,
    enabled,
  });

  // Every time the stream refreshes, re-pin the ledger clock so wall-clock
  // drift cannot accumulate beyond one refresh window.
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void client
      .ledgerTimestamp()
      .then((ts) => {
        if (cancelled) return;
        baseRef.current = ts;
        baseWallMsRef.current = Date.now();
        setLedgerNow(ts);
      })
      .catch(() => {
        /* keep the previous base; the next poll will retry */
      });
    return () => {
      cancelled = true;
    };
  }, [client, stream, enabled]);

  useEffect(() => {
    if (!enabled) return;
    const timer = setInterval(() => {
      const wall = Math.floor((Date.now() - baseWallMsRef.current) / 1000);
      setLedgerNow(baseRef.current + BigInt(Math.max(0, wall)));
    }, Math.max(50, tickMs));
    return () => clearInterval(timer);
  }, [tickMs, enabled]);

  const values = useMemo(() => {
    if (!stream) return null;
    return {
      vested: vested(stream, ledgerNow),
      withdrawable: withdrawable(stream, ledgerNow),
      refundable: refundable(stream, ledgerNow),
      rate: perSecondRate(stream),
    };
  }, [stream, ledgerNow]);

  return {
    stream,
    vested: values?.vested ?? 0n,
    withdrawable: values?.withdrawable ?? 0n,
    refundable: values?.refundable ?? 0n,
    rate: values?.rate ?? 0n,
    status: stream?.status ?? null,
    paused: stream?.status === 1,
    terminal: stream ? stream.status === 2 || stream.status === 3 : false,
    now: ledgerNow,
    loading,
    error,
    refresh,
  };
}