//! Issue #95 — cancel/withdraw mempool ordering must not change payouts.
//!
//! If a sender submits `cancel` while the recipient's `withdraw` is pending,
//! either may execute first. Whichever order the ledger chooses, the two
//! parties must end up with the same tokens: the recipient gets everything
//! vested at the instant, the sender gets the remainder, and the pool empties.
//!
//! Each scenario is run twice — withdraw-first and cancel-first — at the same
//! simulated instant, and the final balances must be identical.

use super::common::*;

fn race_outcome(order: &[&str]) -> (i128, i128, i128) {
    let h = Harness::new();
    let id = h.create_simple(1_200 * ONE, 360 * DAY);
    h.advance(90 * DAY);

    for op in order {
        match *op {
            "withdraw" => {
                h.client.withdraw(&id, &None);
            }
            "cancel" => {
                h.client.cancel(&id);
            }
            _ => unreachable!(),
        }
    }

    h.assert_pool_exact();
    (h.balance(&h.recipient), h.balance(&h.sender), h.pool())
}

#[test]
fn cancel_and_withdraw_are_order_independent() {
    let withdraw_first = race_outcome(&["withdraw", "cancel"]);
    let cancel_first = race_outcome(&["cancel", "withdraw"]);

    assert_eq!(
        withdraw_first, cancel_first,
        "payouts must not depend on cancel/withdraw execution order"
    );
    let (recipient, sender, pool) = withdraw_first;

    // 90 of 360 days of a 1200 stream vests, the remainder is refunded.
    assert_eq!(recipient, 300 * ONE);
    assert_eq!(sender, 1_000_000 * ONE + 900 * ONE);
    assert_eq!(pool, 0, "pool must be empty after both settle");
}

#[test]
fn partial_withdraw_then_cancel_cannot_double_pay() {
    let h = Harness::new();
    let id = h.create_simple(1_200 * ONE, 360 * DAY);
    h.advance(90 * DAY); // 300 vested

    let recipient_before = h.balance(&h.recipient);
    let sender_before = h.balance(&h.sender);

    h.client.withdraw(&id, &Some(100 * ONE));

    let stream_after_withdraw = h.get(id);
    assert_eq!(stream_after_withdraw.withdrawn, 100 * ONE);

    h.client.cancel(&id);
    h.assert_pool_exact();

    // Recipient keeps the 100 drawn plus the 200 vesting tail; sender gets
    // the remaining 900. Nothing is created or destroyed.
    assert_eq!(h.balance(&h.recipient), recipient_before + 300 * ONE);
    assert_eq!(h.balance(&h.sender), sender_before + 900 * ONE);
    assert_eq!(h.pool(), 0);
}