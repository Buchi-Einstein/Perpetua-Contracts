# Dust Theft Impossibility (§102)

A formal argument that micro-top-ups cannot erode the contract's balance or
steal residual stroops.

## Model

Three numbers partition every stream's deposit at every instant (`vested` and
`refundable` in `contracts/stream/src/accrual.rs`, with `withdrawn` tracking
what has already left the contract; invariant **I4** from the accrual docs):

```
vested(t)    = the recipient's total entitlement at t
refundable(t)= deposited - vested(t)     (what cancel returns to the sender)
withdrawn    = what the recipient has already pulled, withdrawn <= vested(t)
liability    = deposited - withdrawn      (what the pool still owes this stream)
```

## Lemma 1 — conservation is exact, residue included

`vested` computes `deposited * consumed` then **floor-divides** by the
duration. For any `deposited, consumed, duration` in the valid domain:

```
vested = floor(deposited * consumed / duration)
       = deposited * consumed / duration - r   where the residue
         r = (deposited * consumed) mod duration, 0 <= r < duration
```

By definition `refundable = deposited - vested`, so

```
vested + refundable = deposited               (exactly, no dust term)
```

There is **no third bucket**. Every stroop is either the recipient's entitlement
or the sender's refund. This identity is asserted on every cancel
(`lib.rs:607-615`) and property-tested across random schedules
(`test::props`).

## Lemma 2 — residue always stays in the pool or returns to the sender

The residue floor `r/duration` of a *stroke* is never credited to anyone as
revenue. It is merely *not yet* vested, so it sits in `refundable`. Two fates
are possible and both are safe:

* the stream matures → `consumed = duration` makes `vested = deposited` exactly,
  residue is zero, the recipient takes the full deposit;
* someone cancels earlier → the entire `refundable` return to the sender as one
  token transfer (`lib.rs:631-639`).

In neither case can a *third* party — attacker or not — collect the residue.
The contract's only outbound transfer paths are `withdraw` (to the recipient),
`cancel` (to the sender), and token finds never touch any address in between.

## Lemma 3 — top-ups cannot create a withdrawable residue

`top_up` extends the duration by `delta = floor(amount * duration / deposited)`
(`lib.rs:373-391`). Rounding is **down**, so the top-up buys *at most* as much
schedule as it funds — the residual fraction of a second, `r = (amount *
duration) mod deposited`, in the *recipient's* favour (the extension is slightly
short, the rate therefore slightly high). Two consequences:

1. `vested` never decreases across a top-up at a fixed `t` (invariant **I3**),
   so no re-vesting can move sender funds toward the recipient without the
   recipient's work — and *zero* toward anyone else.
2. The micro-residue becomes part of `deposited` like any other stroop,
   and is subsumed by Lemma 1/2: it stays in the pool until it either vests to
   the recipient at maturity or returns to the sender on cancel.

No entry point can route it anywhere else.

## Lemma 4 — the attacker's own cost defeats the attack

The only "free" balance a micro-top-up could theoretically `earn` is the
residue of *other* streams' floors. But:

* any third party's residue is `refundable`, which settles to the **sender**,
  not to the tippler;
* the tippler's own top-up requires `delta >= 1` second (`Error::TopUpTooSmall`),
  i.e. `amount >= deposited/duration` — the attack pays the pool at least the
  per-second rate for every attempt;
* the contract pool invariant (`liability <= contract balance`,
  `test::assert_pool_exact`) is checked after every operation, so any attempt
  that *eroded* the pool would trip the suite's poisoned literal, not silently
  accrue to an attacker.

Rate-to-steal is strictly negative: every stroop an attacker could extract
would have to be a stroop that settled to a party who was legitimately owed it,
and settling is a deterministic function the attacker does not control.

## Conclusion

Micro-top-ups **cannot** erode the pool or steal residual stroops:

* every row partitions into `vested + refundable` exactly;
* residue is never creditable to anyone except the recipient (at maturity) or
  the sender (on cancel);
* top-up rounding error runs in the recipient's favour and stays inside the
  same partition;
* the pool invariant is asserted after every operation, and the residue
  fraction — the floor error of a tokenized row — is bounded for any stream the
  contract accepts.

The threat reduces to the constant question any token vault faces — whether the
ledger's *total* liability exceeds the *actual* token balance — which the suite
checks as `assert_pool_exact` on every path.