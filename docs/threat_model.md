# Perpetua Streaming Protocol — Threat Model

> **Status:** Draft
> **Audience:** External auditors, security researchers, integrators

This document analyzes the trust boundaries, actor capabilities, and attack
vectors for the Perpetua streaming protocol on Soroban.  It is not an
exhaustive formal proof; it is a structured map of where value moves, who can
influence it, and what the protocol does to limit damage when assumptions
break.

---

## 1. Trust Boundaries

### 1.1 Token Contract (SEP-41)

The stream contract pulls deposits from the sender and pays the recipient by
calling `token.transfer()` on an external SEP-41 contract.  That contract is
**untrusted**:

* It may be a standard Stellar Asset Contract or a custom issuer contract.
* Its `transfer()` may contain arbitrary logic, including reentrant callbacks.
* It may freeze clawbacks, revoke authorization, or trap for any reason.

**Boundary rule:** the stream contract must never assume the token contract is
well-behaved.  All state changes that must persist must be committed to
storage *before* the token transfer is invoked.

### 1.2 Sender Authorization

`create_stream` and `top_up` require the sender's auth.  That auth covers the
nested token transfer; no prior token approval is needed because the stream
contract is the immediate recipient.

**Boundary rule:** a compromised sender key can drain the sender's token
balance through the stream contract.  The protocol cannot prevent this — it is
the sender's own key.

### 1.3 Recipient Authorization

`withdraw` requires the recipient's auth.  A compromised recipient key can
withdraw accrued funds to an attacker-controlled address.

### 1.4 Factory and Governance (out of scope here)

Factory admin, keepers, and multi-sig governance are defined in sibling
contracts.  This document covers the stream primitive only.

---

## 2. Actor Capabilities

| Actor | Can do | Cannot do |
|---|---|---|
| **Sender** | Create stream, top up, cancel (if `cancellable`), pause/resume (if `pausable`) | Withdraw, transfer recipient, call delegate ops |
| **Recipient** | Withdraw, transfer recipient (if `transferable`), call delegate ops | Cancel, top up, pause/resume |
| **Delegate** | Call granted ops on behalf of grantor | Any op not explicitly granted |
| **Keeper** | Extend stream TTL (permissionless) | Move funds, change stream state |
| **Token Contract** | Execute `transfer()` logic | Write stream storage directly (cross-contract only) |

---

## 3. Attack Vectors and Mitigations

### 3.1 Reentrant Token Contract

**Threat:** A malicious token contract calls back into the stream contract
during `transfer()`, attempting to withdraw or cancel before the original
invocation finishes.

**Mitigation:**
* Soroban forbids reentrancy at the host level.  Nested calls into the same
  transaction's call stack are rejected.
* The stream contract additionally follows **checks-effects-interactions**:
  `stream.withdrawn` and `stream.status` are written to storage before
  `token.transfer()` is called.  Even if reentrancy were possible, the
  attacker would observe only the post-update state.

### 3.2 Token Transfer Failure

**Threat:** The token contract returns an error (insufficient balance,
deauthorized, custom rejection) or traps (no deployed code, panic).

**Mitigation:**
* All token failures are mapped to two stable stream-level errors:
  `TokenTransferFailed` (25) and `TokenMissing` (26).
* Soroban rolls back **all** storage writes on error, so a failed
  `create_stream`, `withdraw`, `cancel`, or `top_up` leaves the stream in
  exactly the same state as before the call.  No phantom entries, no partial
  accounting.

### 3.3 Storage TTL Exhaustion

**Threat:** A stream's ledger entry expires and is archived.  The recipient
cannot withdraw until the entry is restored.

**Mitigation:**
* The contract extends the entry TTL at creation and on every `withdraw`.
* A permissionless `extend_stream_ttl` keeper path exists so any caller can
  pay rent for any stream.  Recipient access never depends on the sender's
  cooperation.
* Multi-year streams require periodic top-ups via keeper calls; this is
  documented in `docs/KNOWN-LIMITATIONS.md`.

### 3.4 Math Truncation

**Threat:** Integer division in accrual or top-up rounding produces dust or
retroactive rate changes.

**Mitigation:**
* All accrual uses checked arithmetic; `deposit * duration` is verified to fit
  in `i128` at creation and again after every top-up.
* Top-up duration extension rounds **down**, never up.  This guarantees
  `vested` never decreases across a top-up.
* Dust-rate streams (`deposit < duration`) are rejected at creation.

### 3.5 Authorization Bypass

**Threat:** A caller invokes an operation without the required auth, or a
delegate uses an expired grant.

**Mitigation:**
* Every mutating entry point calls `require_auth()` on the correct party.
* Delegate grants are checked for ops coverage and `expires_at` on every call.
* `transfer_recipient` requires sender auth (issue #1637); the recipient
  cannot redirect a stream without a sender-issued delegate grant.

### 3.6 State Corruption via Malicious Event Consumers

**Threat:** An off-chain indexer misinterprets events and builds an incorrect
view of stream state.

**Mitigation:**
* Events carry all fields needed to reconstruct the stream state independently.
* The contract is immutable (no admin key, no upgrade path), so the event
  schema is frozen.

---

## 4. Assumptions

### 4.1 Soroban Host Semantics

This threat model assumes the Soroban host:
* Rolls back all storage writes on any contract error or trap.
* Enforces the reentrancy guard (nested calls into the same transaction are
  rejected).
* Provides monotonic, authoritative ledger timestamps.

### 4.2 Multi-Sig Governance

Factory-level policy (capacity caps, minimum duration, rate bounds, allowlist,
creation pause) is governed by a timelocked multi-sig contract.  This model
assumes:
* The multi-sig threshold is set to a value that makes key compromise
  infeasible for the protocol's risk profile.
* The timelock gives integrators time to react to policy changes.

### 4.3 Token Decimal Normalization

SEP-41 tokens may have different `decimals()` values.  The stream contract
operates on raw token units; the SDK and UI are responsible for normalizing
to human-readable amounts.  A token with 0 decimals would break UI display
but not contract safety.

---

## 5. Out-of-Scope Threats

* **Frontend phishing / wallet drain:** The protocol cannot prevent a user
  from signing a malicious transaction crafted by a rogue dApp.
* **RPC / indexer manipulation:** Integrators must validate responses from
  their RPC providers.
* **Side-channel timing:** The contract has no secret-dependent branches; all
  operations run in constant time with respect to secret data.

---

## 6. References

* `contracts/stream/src/lib.rs` — stream contract implementation
* `contracts/stream/src/error.rs` — error discriminant table
* `contracts/stream/src/test/token_errors.rs` — token failure regression tests
* `docs/KNOWN-LIMITATIONS.md` — TTL archival recovery caveats
* `docs/ABI.md` — ABI versioning policy
