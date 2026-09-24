/** Lifecycle state of a stream, mirroring `types.rs::StreamStatus`. */
export enum StreamStatus {
  Active = 0,
  Paused = 1,
  Cancelled = 2,
  Depleted = 3,
}

export const STREAM_STATUS = StreamStatus;

/** A single payment stream, decoded from `get_stream`. */
export interface Stream {
  /** Global AT, monotonically increasing, never reused. */
  id: bigint;
  sender: string;
  recipient: string;
  /** SEP-41 token contract address. One token per stream; never changes. */
  token: string;
  /** Total ever deposited (reduced to what vested on cancel), in stroops. */
  deposited: bigint;
  /** Total ever withdrawn, in stroops. */
  withdrawn: bigint;
  /** Unix seconds. May be in the past (backdated vesting) or future. */
  start_time: bigint;
  end_time: bigint;
  cliff_time: bigint;
  /** Fixed at creation; immutable. */
  cancellable: boolean;
  /** Fixed at creation; immutable. */
  pausable: boolean;
  /** Fixed at creation; immutable. */
  transferable: boolean;
  /** Unix seconds the clock froze at, while paused. */
  paused_at: bigint | null;
  /** Cumulative seconds spent paused (excluding an in-progress pause). */
  paused_total: bigint;
  status: StreamStatus;
}

/** Timestamp source for the client-side ticker. */
export interface Clock {
  /** Ledger timestamp at the base instant (uint64 Unix seconds). */
  baseTimestamp: bigint;
  /** JS `Date.now()` at the same instant wall-clock captured. */
  baseEpochMs: number;
  /** Lock a new base (typically right after a stream refetch). */
  rebase(timestamp: bigint): void;
  /** Current estimate of the ledger timestamp. */
  now(): bigint;
}

/** A contract clock that never re-reads the ledger between calls. */
export function pinnedClock(initialTimestamp: bigint): Clock {
  const baseTimestamp = initialTimestamp < 0n ? 0n : initialTimestamp;
  return {
    baseTimestamp,
    baseEpochMs: Date.now(),
    rebase(timestamp: bigint) {
      this.baseTimestamp = timestamp < 0n ? 0n : timestamp;
      this.baseEpochMs = Date.now();
    },
    now() {
      const wallSeconds = Math.floor((Date.now() - this.baseEpochMs) / 1000);
      return this.baseTimestamp + BigInt(wallSeconds);
    },
  };
}