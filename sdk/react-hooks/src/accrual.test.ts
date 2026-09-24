import { describe, expect, it } from 'vitest';
import {
  cliffReached,
  duration,
  elapsed,
  liability,
  perSecondRate,
  refundable,
  streamTime,
  vested,
  withdrawable,
} from './accrual.js';
import { Stream, StreamStatus } from './types.js';

const DAY = 86_400n;
const ONE = 10_000_000n; // 7-decimals stroops
const T0 = 1_700_000_000n;

function stream(overrides: Partial<Stream> = {}): Stream {
  return {
    id: 0n,
    sender: 'A',
    recipient: 'B',
    token: 'T',
    deposited: 100n * ONE,
    withdrawn: 0n,
    start_time: T0,
    end_time: T0 + 100n * DAY,
    cliff_time: T0,
    cancellable: true,
    pausable: true,
    transferable: true,
    paused_at: null,
    paused_total: 0n,
    status: StreamStatus.Active,
    ...overrides,
  };
}

describe('streamTime (the stream clock)', () => {
  it('equals wall clock when never paused', () => {
    const s = stream();
    expect(streamTime(s, T0 + 10n * DAY)).toBe(10n * DAY);
  });

  it('freezes at paused_at while paused', () => {
    const s = stream({ paused_at: T0 + 10n * DAY, status: StreamStatus.Paused });
    expect(streamTime(s, T0 + 50n * DAY)).toBe(10n * DAY);
  });

  it('absorbs a completed pause into paused_total', () => {
    const s = stream({ paused_total: 5n * DAY });
    expect(streamTime(s, T0 + 30n * DAY)).toBe(25n * DAY);
  });

  it('never underflows below zero', () => {
    const s = stream({ paused_total: 10n * DAY });
    expect(streamTime(s, T0)).toBe(0n);
  });
});

describe('vested', () => {
  it('is zero before the cliff even after time has passed', () => {
    const s = stream({ cliff_time: T0 + 30n * DAY });
    expect(vested(s, T0 + 20n * DAY)).toBe(0n);
  });

  it('at the cliff instant exposes everything accrued since start', () => {
    const s = stream({ cliff_time: T0 + 30n * DAY });
    expect(vested(s, T0 + 30n * DAY)).toBe(30n * ONE);
  });

  it('is fully vested at end_time', () => {
    const s = stream();
    expect(vested(s, T0 + 100n * DAY)).toBe(100n * ONE);
  });

  it('is clamped at deposited past the end', () => {
    const s = stream();
    expect(vested(s, T0 + 5n * 100n * DAY)).toBe(100n * ONE);
  });

  it('returns deposited in full for a zero-duration cancel-collapsed stream', () => {
    const s = stream({ end_time: T0, deposited: 40n * ONE });
    expect(duration(s)).toBe(0n);
    expect(vested(s, T0)).toBe(40n * ONE);
  });

  it('rounds down in the recipient’s disfavour', () => {
    // 100 tokens over 3 seconds: at 1s -> 33 (not 33.33)
    const s = stream({
      deposited: 100n * ONE,
      end_time: T0 + 3n,
      cliff_time: T0,
    });
    expect(vested(s, T0 + 1n)).toBe(33n * ONE);
  });

  it('does not grow while paused', () => {
    const s = stream({ paused_at: T0 + 10n * DAY, status: StreamStatus.Paused });
    expect(vested(s, T0 + 10n * DAY)).toBe(10n * ONE);
    expect(vested(s, T0 + 40n * DAY)).toBe(10n * ONE);
  });
});

describe('withdrawable / refundable', () => {
  it('is vested minus withdrawn, never negative', () => {
    const s = stream({ withdrawn: 5n * ONE });
    expect(withdrawable(s, T0 + 10n * DAY)).toBe(5n * ONE);
  });

  it('saturates at zero on an over-withdrawn corruption', () => {
    const s = stream({ withdrawn: 200n * ONE });
    expect(withdrawable(s, T0 + 10n * DAY)).toBe(0n);
  });

  it('conservation: vested + refundable == deposited', () => {
    const s = stream();
    for (const t of [T0, T0 + 33n * DAY, T0 + 99n * DAY, T0 + 200n * DAY]) {
      expect(vested(s, t) + refundable(s, t)).toBe(s.deposited);
    }
  });

  it('refundable is everything pre-cliff', () => {
    const s = stream({ cliff_time: T0 + 30n * DAY });
    expect(refundable(s, T0 + 1n * DAY)).toBe(100n * ONE);
  });
});

describe('misc', () => {
  it('elapsed clamps to the schedule', () => {
    const s = stream();
    expect(elapsed(s, T0 + 5n * 100n * DAY)).toBe(100n * DAY);
  });

  it('cliffReached uses the stream clock, not wall clock', () => {
    const paused = stream({ paused_at: T0 + 10n * DAY, status: StreamStatus.Paused });
    expect(cliffReached(paused, T0 + 400n * DAY)).toBe(false);
  });

  it('liability is deposited minus withdrawn', () => {
    const s = stream({ withdrawn: 12n * ONE });
    expect(liability(s)).toBe(88n * ONE);
  });

  it('rate is deposited over duration', () => {
    const s = stream();
    expect(perSecondRate(s)).toBe(ONE / DAY); // 100 tokens over 100 days
  });
});