//! Stage 2 — token sub-invocation error categories.
//!
//! Regression suite for [`Error::TokenTransferFailed`] (25) and
//! [`Error::TokenMissing`] (26).
//!
//! ## Design rationale
//!
//! A raw token sub-invocation failure surfaces at the RPC as
//! `Error(Contract, #N)` where `N` is the *token contract's* own discriminant.
//! A client decoding that against Perpetua's error table would misinterpret it
//! silently — e.g. token error #7 reads as `Unauthorized`.
//!
//! Instead, every token failure is mapped onto one of two stable stream-level
//! categories:
//!
//! * `TokenTransferFailed` (25) — token returned a typed contract error
//!   (insufficient balance, pool underfunded, etc.).
//! * `TokenMissing` (26) — host raised an `Abort` (non-contract trap).
//!   On a real network this fires when the token address has no deployed code.
//!   In the test host all sub-invocation failures are contract-typed, so this
//!   variant is verified through its discriminant value only.
//!
//! ## Soroban rollback semantics
//!
//! When a Soroban contract call returns an `Err`, the host rolls back **all**
//! storage writes made during that invocation — including writes made before
//! the failing transfer (the checks-effects-interactions ordering provides
//! reentrancy safety, not persistence-on-failure semantics).
//!
//! Consequences tested here:
//! * A failed `create_stream` leaves no stream entry and does not advance the
//!   id counter.
//! * A failed `withdraw` or `cancel` rolls back accounting and status writes
//!   entirely; the stream is exactly as it was before the call.
//!
//! ## Test matrix
//!
//! | site | scenario | expected error | state after |
//! |---|---|---|---|
//! | `create_stream` | token contract panics → `TokenTransferFailed`* | `TokenTransferFailed` | no entry, id counter unchanged |
//! | `create_stream` | sender balance zero | `TokenTransferFailed` | no entry, id counter unchanged |
//! | `top_up` | sender balance drained | `TokenTransferFailed` | stream unchanged |
//! | `cancel` | pool drained | `TokenTransferFailed` | stream status unchanged (Active) |
//! | `withdraw` | pool drained | `TokenTransferFailed` | withdrawn unchanged, recipient 0 |
//! | `batch_withdraw` | pool drained | `TokenTransferFailed` | recipient 0 |
//! | discriminants | ABI table values | 25 and 26 confirmed | — |
//!
//! *The test host does not distinguish a panicking sub-contract from a
//!  contract-error sub-contract — both surface as `TokenTransferFailed`.
//!  `TokenMissing` is only reachable via WASM execution on a real network.
//!  The variant's discriminant (26) is verified by `token_error_discriminants_match_the_abi_table`.

use soroban_sdk::testutils::{Address as _, IssuerFlags};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{contract, contractimpl, Address, Env, MuxedAddress, String};

use super::common::*;
use crate::{Error, StreamStatus};

// ─── panic token ─────────────────────────────────────────────────────────────

/// A minimal token contract whose `transfer` always panics.
///
/// In WASM execution on a real network a `panic!` produces
/// `ScErrorType::WasmVm`, which the host converts to `InvokeError::Abort` and
/// the stream contract surfaces as [`Error::TokenMissing`].
///
/// In the test host contracts execute as native Rust; a `panic!` is caught and
/// converted to a `ScErrorType::Contract` error, which maps to
/// `InvokeError::Contract(_)` and therefore [`Error::TokenTransferFailed`].
/// This is a known test-host limitation — the two paths converge to the same
/// observable client behaviour (a stable, typed stream-level error) even though
/// they use different discriminants in the two environments.
#[contract]
pub struct PanicToken;

#[contractimpl]
impl PanicToken {
    pub fn transfer(_env: Env, _from: Address, _to: MuxedAddress, _amount: i128) {
        panic!("PanicToken: transfer always fails");
    }

    pub fn balance(_env: Env, _id: Address) -> i128 {
        0
    }
    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        0
    }
    pub fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _live_until_ledger: u32,
    ) {
    }
    pub fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) {
    }
    pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    pub fn burn_from(_env: Env, _spender: Address, _from: Address, _amount: i128) {}
    pub fn decimals(_env: Env) -> u32 {
        7
    }
    pub fn name(env: Env) -> String {
        String::from_str(&env, "PanicToken")
    }
    pub fn symbol(env: Env) -> String {
        String::from_str(&env, "PANIC")
    }
}

fn register_panic_token(h: &Harness) -> Address {
    h.env.register(PanicToken, ())
}

// ─── clawback-enabled SAC ────────────────────────────────────────────────────

/// Create a fresh Stellar Asset Contract with `ClawbackEnabledFlag` set.
///
/// A dedicated asset per test avoids mutating the harness's main token.
fn make_clawback_token<'a>(
    h: &'a Harness<'a>,
) -> (Address, TokenClient<'a>, StellarAssetClient<'a>) {
    let admin = Address::generate(&h.env);
    let asset = h.env.register_stellar_asset_contract_v2(admin.clone());
    asset.issuer().set_flag(IssuerFlags::ClawbackEnabledFlag);
    let token = asset.address();
    (
        token.clone(),
        TokenClient::new(&h.env, &token),
        StellarAssetClient::new(&h.env, &token),
    )
}


// ─── mock sep-41 tokens with custom decimals ─────────────────────────────────────

/// A mock SEP-41 token contract with 6 decimals.
#[contract]
pub struct Token6Decimals;

#[contractimpl]
impl Token6Decimals {
    pub fn transfer(_env: Env, _from: Address, _to: MuxedAddress, _amount: i128) {
        // Transfers succeed for testing purposes
    }

    pub fn balance(_env: Env, _id: Address) -> i128 {
        1_000_000_000_000 // 1 billion stroops (sufficient for tests)
    }

    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        0
    }

    pub fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _live_until_ledger: u32,
    ) {
    }

    pub fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) {
    }

    pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    pub fn burn_from(_env: Env, _spender: Address, _from: Address, _amount: i128) {}
    pub fn decimals(_env: Env) -> u32 {
        6
    }
    pub fn name(env: Env) -> String {
        String::from_str(&env, "Token6Decimals")
    }
    pub fn symbol(env: Env) -> String {
        String::from_str(&env, "TK6")
    }
}

/// A mock SEP-41 token contract with 9 decimals.
#[contract]
pub struct Token9Decimals;

#[contractimpl]
impl Token9Decimals {
    pub fn transfer(_env: Env, _from: Address, _to: MuxedAddress, _amount: i128) {
        // Transfers succeed for testing purposes
    }

    pub fn balance(_env: Env, _id: Address) -> i128 {
        1_000_000_000_000_000_000 // 1 quintillion stroops
    }

    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        0
    }

    pub fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _live_until_ledger: u32,
    ) {
    }

    pub fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) {
    }

    pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    pub fn burn_from(_env: Env, _spender: Address, _from: Address, _amount: i128) {}
    pub fn decimals(_env: Env) -> u32 {
        9
    }
    pub fn name(env: Env) -> String {
        String::from_str(&env, "Token9Decimals")
    }
    pub fn symbol(env: Env) -> String {
        String::from_str(&env, "TK9")
    }
}

/// A mock SEP-41 token contract with 18 decimals.
#[contract]
pub struct Token18Decimals;

#[contractimpl]
impl Token18Decimals {
    pub fn transfer(_env: Env, _from: Address, _to: MuxedAddress, _amount: i128) {
        // Transfers succeed for testing purposes
    }

    pub fn balance(_env: Env, _id: Address) -> i128 {
        500_000_000_000_000_000 // 5e17 stroops (safe for i128)
    }

    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        0
    }

    pub fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _live_until_ledger: u32,
    ) {
    }

    pub fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) {
    }

    pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    pub fn burn_from(_env: Env, _spender: Address, _from: Address, _amount: i128) {}
    pub fn decimals(_env: Env) -> u32 {
        18
    }
    pub fn name(env: Env) -> String {
        String::from_str(&env, "Token18Decimals")
    }
    pub fn symbol(env: Env) -> String {
        String::from_str(&env, "TK18")
    }
}

/// Register a 6-decimal token with the harness.
fn register_token_6_decimals(h: &Harness) -> Address {
    h.env.register(Token6Decimals, ())
}

/// Register a 9-decimal token with the harness.
fn register_token_9_decimals(h: &Harness) -> Address {
    h.env.register(Token9Decimals, ())
}

/// Register an 18-decimal token with the harness.
fn register_token_18_decimals(h: &Harness) -> Address {
    h.env.register(Token18Decimals, ())
}

// ─── create_stream ──────────────────────────────────────────────────────────

/// A panicking token causes the sub-invocation to fail with a contract-level
/// error in the test host, surfacing as [`Error::TokenTransferFailed`].
/// (On a real WASM network, the same panic produces `InvokeError::Abort` →
/// [`Error::TokenMissing`] — see module-level notes.)
#[test]
fn create_stream_with_panicking_token_returns_a_token_error() {
    let h = Harness::new();
    let panic_token = register_panic_token(&h);

    let start = h.now();
    let err = h
        .client
        .try_create_stream(
            &h.sender,
            &h.recipient,
            &panic_token,
            &(1_000 * ONE),
            &start,
            &(start + 100 * DAY),
            &start,
            &true,
            &true,
            &true,
        )
        .unwrap_err()
        .unwrap();

    // In the test host this surfaces as TokenTransferFailed; on a WASM network
    // it would be TokenMissing.  Either way it is a stable stream-level error.
    assert!(
        matches!(err, Error::TokenTransferFailed | Error::TokenMissing),
        "expected a token error, got {err:?}"
    );
}

/// A failed `create_stream` must leave no observable stream entry and must not
/// advance the id counter.  Soroban rolls back all storage writes on error.
#[test]
fn create_stream_token_failure_leaves_no_phantom_entry() {
    let h = Harness::new();
    let panic_token = register_panic_token(&h);

    assert_eq!(h.client.stream_count(), 0);

    let start = h.now();
    let _ = h.client.try_create_stream(
        &h.sender,
        &h.recipient,
        &panic_token,
        &(1_000 * ONE),
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );

    // Soroban rolls back the entire invocation on error, so the id counter and
    // any stream entry written before the transfer are both undone.
    assert_eq!(h.client.stream_count(), 0, "id counter must not advance");
    assert!(!h.client.stream_exists(&0), "no phantom entry at id 0");

    // The next create succeeds and gets id 0 (not 1, because the failure
    // rolled back the counter).
    let next_id = h.create_simple(100 * ONE, 10 * DAY);
    assert_eq!(next_id, 0, "successful create must get id 0");
    assert!(h.client.stream_exists(&0));
}

/// When the sender has insufficient balance the SAC returns a typed contract
/// error; the stream contract surfaces it as [`Error::TokenTransferFailed`].
/// No entry is written because the host rolls back on error.
#[test]
fn create_stream_with_sender_insufficient_balance_returns_token_transfer_failed() {
    let h = Harness::new();
    let (token, tc, admin) = make_clawback_token(&h);

    // Mint then immediately drain so the deposit pull will fail.
    admin.mint(&h.sender, &(100 * ONE));
    admin.clawback(&h.sender, &(100 * ONE));
    assert_eq!(tc.balance(&h.sender), 0);

    let start = h.now();
    let err = h
        .client
        .try_create_stream(
            &h.sender,
            &h.recipient,
            &token,
            &(1_000 * ONE),
            &start,
            &(start + 100 * DAY),
            &start,
            &true,
            &true,
            &true,
        )
        .unwrap_err()
        .unwrap();

    assert_eq!(err, Error::TokenTransferFailed);
    assert_eq!(h.client.stream_count(), 0, "id counter must not advance");
    assert!(!h.client.stream_exists(&0));
}

// ─── top_up ─────────────────────────────────────────────────────────────────

/// `top_up` must return [`Error::TokenTransferFailed`] when the sender has no
/// balance.  Because the whole invocation rolls back, the stream's `deposited`
/// and `end_time` must be exactly as they were before the call.
#[test]
fn top_up_returns_token_transfer_failed_when_sender_has_no_balance() {
    let h = Harness::new();
    let (token, tc, admin) = make_clawback_token(&h);

    admin.mint(&h.sender, &(2_000 * ONE));
    let start = h.now();
    let id = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &(1_000 * ONE),
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );
    h.advance(10 * DAY);

    let before = h.client.get_stream(&id);

    // Drain sender's remaining balance.
    let remaining = tc.balance(&h.sender);
    if remaining > 0 {
        admin.clawback(&h.sender, &remaining);
    }
    assert_eq!(tc.balance(&h.sender), 0);

    let err = h.client.try_top_up(&id, &(200 * ONE)).unwrap_err().unwrap();

    assert_eq!(err, Error::TokenTransferFailed);

    // Rollback: stream must be exactly as before.
    let after = h.client.get_stream(&id);
    assert_eq!(after.deposited, before.deposited);
    assert_eq!(after.end_time, before.end_time);

    // Confirm a subsequent top_up works once the balance is restored.
    admin.mint(&h.sender, &(500 * ONE));
    h.client.top_up(&id, &(200 * ONE));
    assert_eq!(h.client.get_stream(&id).deposited, 1_200 * ONE);
}

// ─── cancel ─────────────────────────────────────────────────────────────────

/// `cancel` must return [`Error::TokenTransferFailed`] when the pool is empty.
/// Because Soroban rolls back all writes on error, the stream status stays
/// `Active` — the cancel did not take effect.
#[test]
fn cancel_returns_token_transfer_failed_when_pool_is_underfunded() {
    let h = Harness::new();
    let (token, tc, admin) = make_clawback_token(&h);
    let contract_id = h.contract_id.clone();

    admin.mint(&h.sender, &(1_000 * ONE));
    let start = h.now();
    let id = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &(1_000 * ONE),
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );
    h.advance(10 * DAY);

    assert!(h.client.refundable_of(&id) > 0);

    // Drain the pool so the refund transfer will fail.
    let pool = tc.balance(&contract_id);
    admin.clawback(&contract_id, &pool);

    let err = h.client.try_cancel(&id).unwrap_err().unwrap();
    assert_eq!(err, Error::TokenTransferFailed);

    // Rollback: stream status must still be Active, nothing moved.
    assert_eq!(
        h.client.get_stream(&id).status,
        StreamStatus::Active,
        "cancel rolled back entirely; stream must still be Active"
    );
    assert_eq!(tc.balance(&h.sender), 0, "no refund moved");
}

// ─── withdraw / apply_withdrawal ────────────────────────────────────────────

/// `withdraw` must return [`Error::TokenTransferFailed`] when the pool is
/// empty.  Rollback means `withdrawn` is unchanged and the recipient has zero.
#[test]
fn withdraw_returns_token_transfer_failed_when_pool_is_underfunded() {
    let h = Harness::new();
    let (token, tc, admin) = make_clawback_token(&h);
    let contract_id = h.contract_id.clone();

    admin.mint(&h.sender, &(1_000 * ONE));
    let start = h.now();
    let id = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &(1_000 * ONE),
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );
    h.advance(50 * DAY);

    assert!(h.client.withdrawable_of(&id) > 0);

    let pool = tc.balance(&contract_id);
    admin.clawback(&contract_id, &pool);

    let err = h.client.try_withdraw(&id, &None).unwrap_err().unwrap();
    assert_eq!(err, Error::TokenTransferFailed);

    // Rollback: nothing changed.
    assert_eq!(tc.balance(&h.recipient), 0);
    assert_eq!(h.client.get_stream(&id).withdrawn, 0);
}

/// `batch_withdraw` must propagate [`Error::TokenTransferFailed`] when the
/// pool is drained.
#[test]
fn batch_withdraw_returns_token_transfer_failed_when_pool_is_underfunded() {
    let h = Harness::new();
    let (token, tc, admin) = make_clawback_token(&h);
    let contract_id = h.contract_id.clone();

    admin.mint(&h.sender, &(2_000 * ONE));
    let start = h.now();
    let a = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &(1_000 * ONE),
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );
    let b = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &(1_000 * ONE),
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );
    h.advance(50 * DAY);

    let pool = tc.balance(&contract_id);
    admin.clawback(&contract_id, &pool);

    let err = h
        .client
        .try_batch_withdraw(&h.recipient, &h.ids(&[a, b]))
        .unwrap_err()
        .unwrap();

    assert_eq!(err, Error::TokenTransferFailed);
    assert_eq!(tc.balance(&h.recipient), 0);
}

// ─── boundary / retry ────────────────────────────────────────────────────────

/// After a `TokenTransferFailed` on `withdraw`, Soroban rolls back all writes
/// including the `withdrawn` counter increment.  Replenishing the pool and
/// retrying succeeds and pays out the full amount — the failed call left no
/// trace in the accounting.
#[test]
fn withdraw_is_retryable_once_pool_is_replenished() {
    let h = Harness::new();
    let (token, tc, admin) = make_clawback_token(&h);
    let contract_id = h.contract_id.clone();

    admin.mint(&h.sender, &(1_000 * ONE));
    let start = h.now();
    let id = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &(1_000 * ONE),
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );
    h.advance(50 * DAY);

    let available = h.client.withdrawable_of(&id);

    // Drain, attempt, confirm failure.
    let pool = tc.balance(&contract_id);
    admin.clawback(&contract_id, &pool);
    let err = h.client.try_withdraw(&id, &None).unwrap_err().unwrap();
    assert_eq!(err, Error::TokenTransferFailed);
    assert_eq!(tc.balance(&h.recipient), 0);

    // Replenish and retry.  Because the failed call rolled back entirely,
    // the retry sees the full withdrawable balance and succeeds.
    admin.mint(&contract_id, &available);
    let paid = h.client.withdraw(&id, &None);
    assert_eq!(paid, available);
    assert_eq!(tc.balance(&h.recipient), available);
}

// ─── discriminants ───────────────────────────────────────────────────────────

/// Confirm the frozen ABI discriminants for both token error variants.
#[test]
fn token_error_discriminants_match_the_abi_table() {
    assert_eq!(Error::TokenTransferFailed as u32, 25);
    assert_eq!(Error::TokenMissing as u32, 26);
}


// ─── custom decimal tokens ────────────────────────────────────────────────────

/// A stream created with a 6-decimal token must accrue and withdraw correctly.
/// Precision conservation (vested + refundable == deposited) must hold.
#[test]
fn stream_with_6_decimal_token_preserves_precision() {
    let h = Harness::new();
    let token = register_token_6_decimals(&h);
    let start = h.now();
    let deposit = 1_000_000_000; // 1,000 tokens with 6 decimals (1e6 stroops each)

    let id = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &deposit,
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );

    h.advance(50 * DAY);
    let withdrawn = h.client.withdraw(&id, &None);

    // Withdrawn amount should be roughly half the deposit (50 days out of 100)
    assert!(withdrawn > 0, "withdrawal should succeed");
    assert!(withdrawn <= deposit, "withdrawn cannot exceed deposit");

    // Verify precision conservation
    let s = h.client.get_stream(&id);
    let vested = accrual::vested(&s, h.now()).expect("no overflow");
    let refundable = accrual::refundable(&s, h.now()).expect("no overflow");
    assert_eq!(vested + refundable, s.deposited, "precision must be conserved");
}

/// A stream created with a 9-decimal token must accrue and withdraw correctly.
#[test]
fn stream_with_9_decimal_token_preserves_precision() {
    let h = Harness::new();
    let token = register_token_9_decimals(&h);
    let start = h.now();
    let deposit = 1_000_000_000_000; // 1,000 tokens with 9 decimals (1e9 stroops each)

    let id = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &deposit,
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );

    h.advance(50 * DAY);
    let withdrawn = h.client.withdraw(&id, &None);

    assert!(withdrawn > 0, "withdrawal should succeed");
    assert!(withdrawn <= deposit, "withdrawn cannot exceed deposit");

    let s = h.client.get_stream(&id);
    let vested = accrual::vested(&s, h.now()).expect("no overflow");
    let refundable = accrual::refundable(&s, h.now()).expect("no overflow");
    assert_eq!(vested + refundable, s.deposited, "precision must be conserved");
}

/// A stream created with an 18-decimal token must accrue and withdraw correctly.
#[test]
fn stream_with_18_decimal_token_preserves_precision() {
    let h = Harness::new();
    let token = register_token_18_decimals(&h);
    let start = h.now();
    // Use a conservative deposit that stays well within i128 range
    let deposit = 100_000_000_000_000_000; // 0.1 tokens with 18 decimals (1e17 stroops)

    let id = h.client.create_stream(
        &h.sender,
        &h.recipient,
        &token,
        &deposit,
        &start,
        &(start + 100 * DAY),
        &start,
        &true,
        &true,
        &true,
    );

    h.advance(50 * DAY);
    let withdrawn = h.client.withdraw(&id, &None);

    assert!(withdrawn > 0, "withdrawal should succeed");
    assert!(withdrawn <= deposit, "withdrawn cannot exceed deposit");

    let s = h.client.get_stream(&id);
    let vested = accrual::vested(&s, h.now()).expect("no overflow");
    let refundable = accrual::refundable(&s, h.now()).expect("no overflow");
    assert_eq!(vested + refundable, s.deposited, "precision must be conserved");
}

/// Across all three decimal configurations, the same logical stream parameters
/// must produce proportionally identical vested amounts.
#[test]
fn custom_decimal_tokens_maintain_scale_invariance() {
    let start = T0;
    let duration = 100 * DAY;
    let token_amount_6 = 1_000_000_000;   // 1000 tokens @ 6 decimals
    let token_amount_9 = 1_000_000_000_000; // 1000 tokens @ 9 decimals
    let token_amount_18 = 1_000_000_000_000_000_000; // 1000 tokens @ 18 decimals

    // 6-decimal token
    let h6 = Harness::new();
    let token6 = register_token_6_decimals(&h6);
    let id6 = h6.client.create_stream(
        &h6.sender, &h6.recipient, &token6,
        &token_amount_6, &start, &(start + duration), &start,
        &true, &true, &true,
    );
    h6.advance(50 * DAY);
    let vested6 = accrual::vested(&h6.client.get_stream(&id6), h6.now()).unwrap();

    // 9-decimal token
    let h9 = Harness::new();
    let token9 = register_token_9_decimals(&h9);
    let id9 = h9.client.create_stream(
        &h9.sender, &h9.recipient, &token9,
        &token_amount_9, &start, &(start + duration), &start,
        &true, &true, &true,
    );
    h9.advance(50 * DAY);
    let vested9 = accrual::vested(&h9.client.get_stream(&id9), h9.now()).unwrap();

    // 18-decimal token
    let h18 = Harness::new();
    let token18 = register_token_18_decimals(&h18);
    let id18 = h18.client.create_stream(
        &h18.sender, &h18.recipient, &token18,
        &token_amount_18, &start, &(start + duration), &start,
        &true, &true, &true,
    );
    h18.advance(50 * DAY);
    let vested18 = accrual::vested(&h18.client.get_stream(&id18), h18.now()).unwrap();

    // All three should be exactly half of their respective deposits
    assert_eq!(vested6, token_amount_6 / 2);
    assert_eq!(vested9, token_amount_9 / 2);
    assert_eq!(vested18, token_amount_18 / 2);
}
