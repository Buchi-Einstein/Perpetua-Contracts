/**
 * Client-side reimplementation of the contract's accrual model.
 *
 * This is a faithful port of `contracts/stream/src/accrual.rs`, expressed in
 * BigInt so the hook can render a *continuous* balance between RPC polls
 * without any ledger interaction. The conformance target is the on-chain math;
 * every function here has a one-to-one counterpart in the Rust module and the
 * same rounding semantics:
 *
 * - **stream clock stops while paused.** `stream_time` freezes at
 *   `paused_at` and subtracts `paused_total`, exactly like the contract.
 * - **rounding is down.** Integer division truncates in the recipient's
 *   disfavour; the residue remains in the pool and returns to the sender at
 *   settlement.
 * - **cliff gates, does not delay.** At `cliff_time` the recipient becomes
 *   entitled to everything accrued since `start_time`.
 * - **conservation is exact.** `vested + refundable == deposited` with no dust
 *   term, because vesting is derived cumulatively rather than summed per
 *   interval.
 *
 * All amounts are stroops (`bigint`). All timestamps are uint64 Unix seconds.
 */

import type { Stream } from "./types.js";

/** The stream's own clock: wall-clock minus accumulated + in-progress pauses. */
export function streamTime(stream: Stream, now: bigint): bigint {
  const frozenAt = stream.paused_at ?? now;
  const t = frozenAt - stream.paused_total;
  return t < 0n ? 0n : t;
}

/** Total scheduled duration in seconds; zero only after a cancel at start. */
export function duration(stream: Stream): bigint {
  return stream.end_time - stream.start_time;
}

/**
 * Seconds of schedule consumed at `now`, clamped to `[0, duration]`.
 */
export function elapsed(stream: Stream, now: bigint): bigint {
  const clock = streamTime(stream, now);
  const capped = clock > stream.end_time ? stream.end_time : clock;
  const raw = capped - stream.start_time;
  return raw < 0n ? 0n : raw;
}

/** Whether the cliff gate has opened, evaluated on the stream clock. */
export function cliffReached(stream: Stream, now: bigint): boolean {
  return streamTime(stream, now) >= stream.cliff_time;
}

/**
 * Amount vested at `now`: what the recipient has earned in total, ever.
 *
 * Before the cliff this is zero. A zero `duration` (a cancel collapsing the
 * schedule onto `start_time`) returns `deposited` in full, matching the
 * contract's special case.
 */
export function vested(stream: Stream, now: bigint): bigint {
  if (!cliffReached(stream, now)) {
    return 0n;
  }
  const totalDuration = duration(stream);
  if (totalDuration === 0n) {
    return stream.deposited;
  }
  const consumed = elapsed(stream, now);
  if (consumed >= totalDuration) {
    return stream.deposited;
  }
  const raw = (stream.deposited * consumed) / totalDuration;
  return raw > stream.deposited ? stream.deposited : raw;
}

/** Amount the recipient can withdraw right now: vested minus withdrawn. */
export function withdrawable(stream: Stream, now: bigint): bigint {
  const earned = vested(stream, now);
  const available = earned - stream.withdrawn;
  return available < 0n ? 0n : available;
}

/** Amount the sender gets back if they cancel at `now`. */
export function refundable(stream: Stream, now: bigint): bigint {
  const earned = vested(stream, now);
  return stream.deposited - earned;
}

/** The stream's outstanding liability against the contract's pooled balance. */
export function liability(stream: Stream): bigint {
  return stream.deposited - stream.withdrawn;
}

/** Rate in stroops per second, for display: `deposited / duration`. */
export function perSecondRate(stream: Stream): bigint {
  const d = duration(stream);
  return d === 0n ? 0n : stream.deposited / d;
}