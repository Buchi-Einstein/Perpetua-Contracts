#![no_std]
//! # Perpetua Factory
//!
//! Parameter-gated stream factory for Perpetua.
//!
//! All of Perpetua's protocol-wide policy is enforced here, before any stream is
//! launched: a capacity cap, a minimum stream duration, optional per-second
//! rate bounds, an optional recipient allowlist, and a global creation pause.
//! Each policy axis has a matching admin-only setter, so governance can tune the
//! factory's policy in place without touching already-created streams.
//!
//! ## Why a factory
//!
//! The [`FluxoraStream`] contract itself is deliberately un-configurable — no
//! admin key, no upgrade path, no fee switch. That immutability is what makes a
//! stream safe for an untrusted recipient to accept. The factory is the layer
//! that *does* carry policy: a treasury that wants its on-chain creators to
//! respect caps and minimums pins this contract in front of stream creation.
//!
//! ## Admin rotation (issue #39)
//!
//! The factory gates every policy setter behind a single admin key. Because a
//! single-step rotation can destructively transfer control (or, worse, rotate
//! it onto an un-signable key), the factory supports an optional two-step
//! hand-off: `propose_admin` nominates a successor, and `accept_admin` — called
//! by that successor — completes the transfer and immediately revokes the
//! previous admin. The zero address and the factory's own address are rejected
//! outright as admin candidates in both the single-step and two-step paths.

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Env, String,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Instance entries are extended to the network-maxmium-adjacent window on every
/// mutating call (same values as the stream and governance contracts). A factory
/// that is merely read never bumps rent, by design — reads have no side effects.
const INSTANCE_LIFETIME_THRESHOLD: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 120_960;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Storage keys.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// The single persistent policy record, written by `init` and rewritten by
    /// every setter.
    Config,
    /// Presence of an address under this key means it is allowlisted.
    Allowlist(Address),
    /// The proposed next admin for the two-step rotation hand-off. Written by
    /// `propose_admin`, consumed (and cleared) by `accept_admin`.
    PendingAdmin,
}

/// The full on-chain configuration record.
///
/// Written atomically by `init` and re-written atomically by every setter, so a
/// reader of [`FluxoraFactory::get_factory_config`] always observes a coherent
/// policy, never a half-applied one.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactoryConfig {
    pub admin: Address,
    pub stream_contract: Address,
    pub max_deposit: i128,
    pub min_duration: u64,
    /// When true, the batch creation path enforces `max_deposit` per stream
    /// (not merely per batch).
    pub batch_cap_enforced: bool,
    /// When true, `create_stream` is rejected while the factory is paused.
    pub creation_paused: bool,
    /// Optional lower bound on the per-second stream rate, applied at creation.
    pub min_rate_per_second: Option<i128>,
    /// Optional upper bound on the per-second stream rate, applied at creation.
    pub max_rate_per_second: Option<i128>,
}

/// The policy view consumed by the stream-creation paths.
///
/// This is the same record as [`FactoryConfig`] minus the admin field. Keeping
/// the two types separate means a policy consumer can never accidentally read
/// the admin address, and lets `load_policy` serve as the single chokepoint
/// between raw storage and every semantic guard.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactoryPolicy {
    pub stream_contract: Address,
    pub max_deposit: i128,
    pub min_duration: u64,
    pub batch_cap_enforced: bool,
    pub creation_paused: bool,
    pub min_rate_per_second: Option<i128>,
    pub max_rate_per_second: Option<i128>,
}

/// Error codes for the factory contract.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FactoryError {
    /// Contract has not been initialised.
    NotInitialized = 1,
    /// Contract is already initialised.
    AlreadyInitialized = 2,
    /// Caller is not the admin.
    Unauthorized = 3,
    /// The recipient is not allowlisted.
    AllowlistDenied = 4,
    /// The proposed admin cannot be set: it is the zero address or the factory
    /// contract itself, either of which would permanently lock the factory.
    InvalidNewAdmin = 5,
    /// `accept_admin` was called but no admin transfer has been proposed.
    NoPendingAdmin = 6,
}

/// Emitted when an admin rotation is proposed (two-step hand-off, issue #39).
#[contractevent]
pub struct AdminTransferProposed {
    #[topic]
    pub current: Address,
    #[topic]
    pub pending: Address,
}

/// Emitted when the proposed admin accepts the hand-off. From this moment the
/// previous admin is immediately revoked.
#[contractevent]
pub struct AdminTransferred {
    #[topic]
    pub previous: Address,
    #[topic]
    pub new: Address,
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

/// Bumps the instance TTL on every mutating call so an actively-administered
/// factory never archives.
fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Returns the stored config or `NotInitialized`.
fn load_config(env: &Env) -> Result<FactoryConfig, FactoryError> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(FactoryError::NotInitialized)
}

/// Centralised policy-load chokepoint.
///
/// All creation-path guards read policy through here rather than reaching into
/// storage themselves, so defaults and optional-field handling live in exactly
/// one place. Returns `NotInitialized` before any `init`.
pub fn load_policy(env: &Env) -> Result<FactoryPolicy, FactoryError> {
    let config = load_config(env)?;
    Ok(FactoryPolicy {
        stream_contract: config.stream_contract,
        max_deposit: config.max_deposit,
        min_duration: config.min_duration,
        batch_cap_enforced: config.batch_cap_enforced,
        creation_paused: config.creation_paused,
        min_rate_per_second: config.min_rate_per_second,
        max_rate_per_second: config.max_rate_per_second,
    })
}

// ---------------------------------------------------------------------------
// Admin rotation guards
// ---------------------------------------------------------------------------

/// The all-zeros Stellar account (`G…WHF`). Anything signed by this key does
/// not exist, so installing it as admin would permanently burn the factory's
/// control surface. The factory refuses it everywhere an admin can be set.
fn is_zero_address(env: &Env, addr: &Address) -> bool {
    let zero = Address::from_string(&String::from_str(
        env,
        "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
    ));
    *addr == zero
}

/// An address that must never become admin: the zero account (which cannot
/// sign) or the factory's own contract address (which would make the factory
/// its own admin and lock every setter behind a self-authorization).
fn is_forbidden_admin(env: &Env, addr: &Address) -> bool {
    is_zero_address(env, addr) || *addr == env.current_contract_address()
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct FluxoraFactory;

#[contractimpl]
impl FluxoraFactory {
    /// Initialise the factory with an admin and the initial policy axes.
    ///
    /// # Parameters
    /// - `admin`: Address that can update every policy axis.
    /// - `stream_contract`: Address of the `fluxora-stream` contract created
    ///   streams will point at.
    /// - `max_deposit`: Capacity cap per stream (and per batch when
    ///   `batch_cap_enforced` is true).
    /// - `min_duration`: Minimum stream duration in seconds.
    ///
    /// # Errors
    /// - `AlreadyInitialized`: A policy record already exists.
    pub fn init(
        env: Env,
        admin: Address,
        stream_contract: Address,
        max_deposit: i128,
        min_duration: u64,
    ) -> Result<(), FactoryError> {
        if env.storage().instance().has(&DataKey::Config) {
            return Err(FactoryError::AlreadyInitialized);
        }
        admin.require_auth();

        let config = FactoryConfig {
            admin,
            stream_contract,
            max_deposit,
            min_duration,
            // Documented init defaults for the optional axes.
            batch_cap_enforced: true,
            creation_paused: false,
            min_rate_per_second: None,
            max_rate_per_second: None,
        };
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    /// Full configuration record, including the admin address.
    ///
    /// # Errors
    /// - `NotInitialized`: No policy record exists.
    pub fn get_factory_config(env: Env) -> FactoryConfig {
        match load_config(&env) {
            Ok(config) => config,
            Err(e) => panic_with_error!(&env, e),
        }
    }

    /// True if `recipient` is allowlisted.
    ///
    /// # Errors
    /// - `NotInitialized`: No policy record exists.
    pub fn is_allowlisted(env: Env, recipient: Address) -> bool {
        load_config(&env).map_or_else(
            |e| panic_with_error!(&env, e),
            |_| {
                env.storage()
                    .persistent()
                    .has(&DataKey::Allowlist(recipient))
            },
        )
    }

    /// True when the factory is paused and stream creation is rejected.
    ///
    /// # Errors
    /// - `NotInitialized`: No policy record exists.
    pub fn is_factory_paused(env: Env) -> bool {
        load_config(&env).map_or_else(
            |e| panic_with_error!(&env, e),
            |config| config.creation_paused,
        )
    }

    // -----------------------------------------------------------------------
    // Admin setters
    // -----------------------------------------------------------------------

    fn guard(env: &Env) -> Result<FactoryConfig, FactoryError> {
        let config = load_config(env)?;
        config.admin.clone().require_auth();
        Ok(config)
    }

    /// Rotate the admin address.
    ///
    /// # Burn protection
    ///
    /// Rejects the zero address (`G…WHF`) and the factory's own contract
    /// address: both would silently lock every policy setter behind an
    /// unreachable signature. For a safer hand-off use the two-step
    /// [`propose_admin`](Self::propose_admin) / [`accept_admin`](Self::accept_admin)
    /// pair instead.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        if is_forbidden_admin(&env, &new_admin) {
            return Err(FactoryError::InvalidNewAdmin);
        }
        config.admin = new_admin;
        env.storage().instance().set(&DataKey::Config, &config);
        // A single-step rotation while a two-step hand-off is pending would
        // leave a stale PendingAdmin behind; clear it so accept_admin cannot
        // later resurrect a superseded proposal.
        env.storage().instance().remove(&DataKey::PendingAdmin);
        bump_instance(&env);
        Ok(())
    }

    /// Nominate the next admin for a two-step transfer (issue #39).
    ///
    /// The pending admin takes over only after they call
    /// [`accept_admin`](Self::accept_admin); until then the current admin keeps
    /// full authority. This gives a rotating key a chance to prove it can sign
    /// before it is handed the factory.
    ///
    /// # Authorization
    /// - Requires the current admin's signature.
    ///
    /// # Errors
    /// - `InvalidNewAdmin`: the proposed address is the zero address or the
    ///   factory itself.
    /// - `Unauthorized`: the caller is not the current admin.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), FactoryError> {
        let config = Self::guard(&env)?;
        if is_forbidden_admin(&env, &new_admin) {
            return Err(FactoryError::InvalidNewAdmin);
        }
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        bump_instance(&env);

        // CEI: the pending nomination is persisted before the event is emitted.
        AdminTransferProposed {
            current: config.admin,
            pending: new_admin,
        }
        .publish(&env);
        Ok(())
    }

    /// Accept a pending admin nomination and complete the transfer.
    ///
    /// The caller must be the exact address stored by [`propose_admin`](Self::propose_admin).
    /// On success the previous admin is **immediately revoked** — every setter
    /// re-checks the current admin, which is now the new address — and the
    /// pending nomination is consumed.
    ///
    /// # Errors
    /// - `NoPendingAdmin`: no transfer has been proposed.
    pub fn accept_admin(env: Env) -> Result<(), FactoryError> {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(FactoryError::NoPendingAdmin)?;
        // The caller must *be* the pending admin; any other address fails here.
        pending.require_auth();

        let mut config = load_config(&env)?;
        let previous = config.admin;
        config.admin = pending;
        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        bump_instance(&env);

        // CEI: the transfer is persisted before the event is emitted.
        AdminTransferred {
            previous,
            new: config.admin,
        }
        .publish(&env);
        Ok(())
    }

    /// The currently nominated successor, if a two-step transfer is pending.
    ///
    /// # Errors
    /// - `NotInitialized`: No policy record exists.
    pub fn pending_admin(env: Env) -> Option<Address> {
        load_config(&env).map_or_else(
            |e| panic_with_error!(&env, e),
            |_| env.storage().instance().get(&DataKey::PendingAdmin),
        )
    }

    /// Point the factory at a different `fluxora-stream` contract.
    pub fn set_stream_contract(env: Env, stream_contract: Address) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.stream_contract = stream_contract;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    /// Update the per-stream capacity cap.
    pub fn set_cap(env: Env, max_deposit: i128) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.max_deposit = max_deposit;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    /// Update the minimum stream duration.
    pub fn set_min_duration(env: Env, min_duration: u64) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.min_duration = min_duration;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    /// Add or remove a recipient from the allowlist.
    ///
    /// Removing a recipient that was never added is a safe no-op.
    pub fn set_allowlist(env: Env, recipient: Address, allowed: bool) -> Result<(), FactoryError> {
        Self::guard(&env)?;
        let key = DataKey::Allowlist(recipient);
        if allowed {
            env.storage().persistent().set(&key, &true);
            bump_allowlist(&env, &key);
        } else {
            env.storage().persistent().remove(&key);
        }
        Ok(())
    }

    /// Toggle whether the batch creation path enforces the cap per stream.
    pub fn set_batch_cap_enforcement(env: Env, enforced: bool) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.batch_cap_enforced = enforced;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    /// Pause or unpause stream creation.
    pub fn set_factory_paused(env: Env, paused: bool) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.creation_paused = paused;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    /// Set optional per-second rate bounds applied at stream creation.
    ///
    /// Passing `None` for either bound clears it.
    pub fn set_rate_bounds(
        env: Env,
        min_rate_per_second: Option<i128>,
        max_rate_per_second: Option<i128>,
    ) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.min_rate_per_second = min_rate_per_second;
        config.max_rate_per_second = max_rate_per_second;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }
}

/// Structural implementation of the governance interface over the contract's
/// own `#[contractimpl]` entrypoints. Each method simply forwards to the ABI
/// method of the same name, so the trait and the deployed entrypoints can never
/// drift apart.
impl FactoryGovernance for FluxoraFactory {
    fn set_admin(env: Env, new_admin: Address) -> Result<(), FactoryError> {
        FluxoraFactory::set_admin(env, new_admin)
    }

    fn set_stream_contract(env: Env, stream_contract: Address) -> Result<(), FactoryError> {
        FluxoraFactory::set_stream_contract(env, stream_contract)
    }

    fn set_cap(env: Env, max_deposit: i128) -> Result<(), FactoryError> {
        FluxoraFactory::set_cap(env, max_deposit)
    }

    fn set_min_duration(env: Env, min_duration: u64) -> Result<(), FactoryError> {
        FluxoraFactory::set_min_duration(env, min_duration)
    }

    fn set_allowlist(env: Env, recipient: Address, allowed: bool) -> Result<(), FactoryError> {
        FluxoraFactory::set_allowlist(env, recipient, allowed)
    }

    fn set_batch_cap_enforcement(env: Env, enforced: bool) -> Result<(), FactoryError> {
        FluxoraFactory::set_batch_cap_enforcement(env, enforced)
    }

    fn set_factory_paused(env: Env, paused: bool) -> Result<(), FactoryError> {
        FluxoraFactory::set_factory_paused(env, paused)
    }

    fn set_rate_bounds(
        env: Env,
        min_rate_per_second: Option<i128>,
        max_rate_per_second: Option<i128>,
    ) -> Result<(), FactoryError> {
        FluxoraFactory::set_rate_bounds(env, min_rate_per_second, max_rate_per_second)
    }
}

/// Extends the TTL of an allowlist entry when it is (re)written so a populated,
/// actively-queried allowlist stays readable between admin rotations.
fn bump_allowlist(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}
