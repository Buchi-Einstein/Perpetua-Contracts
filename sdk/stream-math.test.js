const assert = require("node:assert");
const {
  streamTime,
  duration,
  elapsed,
  cliffReached,
  vested,
  withdrawable,
  refundable,
  liability,
} = require("./stream-math.js");

const DAY = 86400n;

function stream({
  deposited,
  start_time,
  end_time,
  cliff_time = start_time,
  withdrawn = 0n,
  paused_at = null,
  paused_total = 0n,
}) {
  return {
    deposited,
    withdrawn,
    start_time,
    end_time,
    cliff_time,
    paused_at,
    paused_total,
  };
}

// Linear accrual: 1000 over 10 days.
const s = stream({ deposited: 1000n, start_time: 1_700_000_000n, end_time: 1_700_000_000n + 10n * DAY });

assert.equal(duration(s), 10n * DAY);
assert.equal(elapsed(s, s.start_time + 5n * DAY), 5n * DAY);

// Cliff gates payout, does not delay accrual: at the cliff instant the full
// backdated amount vests.
assert.equal(cliffReached(s, s.start_time), true); // cliff == start: open from the start
assert.equal(vested(s, s.start_time), 0n);
const cliffed = stream({
  deposited: 1000n,
  start_time: s.start_time,
  end_time: s.end_time,
  cliff_time: s.start_time + 4n * DAY,
});
assert.equal(vested(cliffed, s.start_time + 4n * DAY), 400n);

// 1000/10days = 100/day; half-way = 500.
assert.equal(vested(s, s.start_time + 5n * DAY), 500n);
assert.equal(withdrawable(s, s.start_time + 5n * DAY), 500n);
assert.equal(refundable(s, s.start_time + 5n * DAY), 500n);

// Floor division + residue stays refundable (conservation, no dust).
const odd = stream({ deposited: 1000n, start_time: s.start_time, end_time: s.start_time + 7n });
assert.equal(vested(odd, s.start_time + 3n), 428n); // 3000/7 = 428.57
assert.equal(vested(odd, s.start_time + 3n) + refundable(odd, s.start_time + 3n), 1000n);

// Maturity returns the full deposit even past end.
assert.equal(vested(s, s.end_time), 1000n);
assert.equal(vested(s, s.end_time + 100n), 1000n);
assert.equal(withdrawable(s, s.end_time + 100n), 1000n);
assert.equal(refundable(s, s.end_time + 100n), 0n);

// withdrawable saturates at zero; over-drawn liability reads through.
assert.equal(withdrawable(s, s.start_time), 0n);
assert.equal(refundable(s, s.start_time), 1000n);
assert.equal(liability({ ...s, withdrawn: 999n }), 1n);
assert.equal(withdrawable({ ...s, withdrawn: 1001n }, s.start_time + 5n * DAY), 0n);

// Paused stream: clock freezes at the pause instant, no accrual while paused.
const p = stream({
  deposited: 1000n,
  start_time: s.start_time,
  end_time: s.end_time,
  paused_at: s.start_time + 5n * DAY,
});
assert.equal(streamTime(p, s.start_time + 9n * DAY), s.start_time + 5n * DAY);
assert.equal(vested(p, s.start_time + 9n * DAY), 500n);

// overwhelmed paused_total saturates the clock at zero.
assert.equal(streamTime(stream({ ...s, paused_total: 500n }), 100n), 0n);

console.log("stream-math.js: all differential vectors pass");