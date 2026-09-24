// Perpetua stream math — off-chain mirror of contracts/stream/src/accrual.rs.
//
// Pure functions only: no RPC, no ledger access. Given a Stream and a wall-clock
// timestamp the caller can compute withdrawable balances locally. The reference
// implementation is Rust; every function here mirrors it one-to-one (same
// saturation, same rounding direction). Holds for `contracts/stream/Cargo.toml`
// @ commit `c5988fd`.
//
// Amounts and timestamps in this domain are u64/i128, which exceed the exact
// `Number` range, so every quantity is a `bigint`. Pass `0n`/`10n`-style
// literals or `BigInt(...)`; you may pass plain integers only when they are
// known safe (below `2**53`).

/// The stream's own clock: wall-clock time with all accumulated pauses removed.
/// While paused the clock is frozen at the pause instant. Saturates at zero.
export function streamTime(stream, now) {
  const frozenAt = stream.paused_at === null ? now : stream.paused_at;
  return clampSub(frozenAt, stream.paused_total);
}

/// Total scheduled duration in seconds.
export function duration(stream) {
  return clampSub(stream.end_time, stream.start_time);
}

/// Seconds of the schedule actually consumed, clamped to `[0, duration]`.
export function elapsed(stream, now) {
  const clock = streamTime(stream, now);
  const capped = clock > stream.end_time ? stream.end_time : clock;
  return clampSub(capped, stream.start_time);
}

/// Whether the cliff gate has opened, evaluated against the stream clock.
export function cliffReached(stream, now) {
  return streamTime(stream, now) >= stream.cliff_time;
}

/// Amount vested at `now`: total earned, withdrawn or not. Rounds **down** —
/// integer division truncating in the recipient's disfavour keeps the residue
/// in the contract, so the pool can never be short. Before the cliff this is 0.
export function vested(stream, now) {
  if (!cliffReached(stream, now)) return 0n;

  const totalDuration = duration(stream);

  // Zero duration means the schedule collapsed onto an instant (after a cancel
  // landing at start_time); deposited was already rewritten, return it in full.
  if (totalDuration === 0n) return stream.deposited;

  const consumed = elapsed(stream, now);
  if (consumed >= totalDuration) return stream.deposited;

  const numerator = stream.deposited * consumed;
  const raw = numerator / totalDuration; // BigInt division truncates toward zero

  return raw > stream.deposited ? stream.deposited : raw; // clamp, defence in depth
}

/// Amount withdrawable right now: vested minus withdrawn. Saturates at zero.
export function withdrawable(stream, now) {
  const earned = vested(stream, now);
  const available = earned - stream.withdrawn;
  return available < 0n ? 0n : available;
}

/// Amount the sender gets back on cancel at `now`: deposited minus vested.
export function refundable(stream, now) {
  return stream.deposited - vested(stream, now);
}

/// Outstanding pooled liability: deposited minus withdrawn.
export function liability(stream) {
  return stream.deposited - stream.withdrawn;
}

function clampSub(a, b) {
  return a >= b ? a - b : 0n;
}