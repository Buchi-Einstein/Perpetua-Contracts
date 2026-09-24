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

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Bytes, BytesN, Env,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Storage entries are extended to the network-maximum-adjacent window on every
/// policy interaction (same values as the stream and governance contracts).
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
    pub stream_wasm_hash: BytesN<32>,
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
    pub stream_wasm_hash: BytesN<32>,
    pub max_deposit: i128,
    pub min_duration: u64,
    pub batch_cap_enforced: bool,
    pub creation_paused: bool,
    pub min_rate_per_second: Option<i128>,
    pub max_rate_per_second: Option<i128>,
}

/// A stream creation proxied by this factory.
///
/// The topic layout intentionally matches the stream contract's creation
/// event namespace while adding the factory address as the first indexed
/// context field: `stream_created, factory_id, sender, recipient`.
#[contractevent]
pub struct StreamCreated {
    #[topic]
    pub factory_id: Address,
    #[topic]
    pub sender: Address,
    #[topic]
    pub recipient: Address,
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
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

/// Derives the deterministic salt used for a child stream deployment.
///
/// The serialized preimage is domain-separated and length-fixed after the
/// sender address, so distinct `(sender, nonce, stream_id)` tuples cannot be
/// confused by concatenation. The tuple is hashed to the 32-byte salt format
/// accepted by Soroban deployment APIs.
pub fn derive_stream_salt(env: &Env, sender: &Address, nonce: u64, stream_id: u64) -> BytesN<32> {
    let mut preimage = Bytes::from_slice(env, b"perpetua-factory-stream-salt-v1");
    preimage.append(&sender.to_xdr(env));
    preimage.append(&Bytes::from_slice(env, &nonce.to_be_bytes()));
    preimage.append(&Bytes::from_slice(env, &stream_id.to_be_bytes()));
    env.crypto().sha256(&preimage)
}

/// Extends the factory instance entry to the network maximum.
fn bump_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Returns the stored config or `NotInitialized`.
fn load_config(env: &Env) -> Result<FactoryConfig, FactoryError> {
    let config = env
        .storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(FactoryError::NotInitialized)?;
    bump_ttl(env);
    Ok(config)
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
        stream_wasm_hash: config.stream_wasm_hash,
        max_deposit: config.max_deposit,
        min_duration: config.min_duration,
        batch_cap_enforced: config.batch_cap_enforced,
        creation_paused: config.creation_paused,
        min_rate_per_second: config.min_rate_per_second,
        max_rate_per_second: config.max_rate_per_second,
    })
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
    /// - `stream_wasm_hash`: Reviewed WASM hash that the stream contract must
    ///   have been deployed from.
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
        stream_wasm_hash: BytesN<32>,
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
            stream_wasm_hash,
            max_deposit,
            min_duration,
            // Documented init defaults for the optional axes.
            batch_cap_enforced: true,
            creation_paused: false,
            min_rate_per_second: None,
            max_rate_per_second: None,
        };
        env.storage().instance().set(&DataKey::Config, &config);
        bump_ttl(&env);
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
                let key = DataKey::Allowlist(recipient);
                let allowed = env.storage().persistent().has(&key);
                if allowed {
                    bump_allowlist(&env, &key);
                }
                allowed
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

    /// Return the canonical salt for a child stream deployment.
    pub fn derive_stream_salt(
        env: Env,
        sender: Address,
        nonce: u64,
        stream_id: u64,
    ) -> BytesN<32> {
        crate::derive_stream_salt(&env, &sender, nonce, stream_id)
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
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.admin = new_admin;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_ttl(&env);
        Ok(())
    }

    /// Point the factory at a different `fluxora-stream` contract.
    pub fn set_stream_contract(env: Env, stream_contract: Address) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.stream_contract = stream_contract;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_ttl(&env);
        Ok(())
    }

    /// Update the reviewed WASM hash for the configured stream contract.
    pub fn set_stream_wasm_hash(
        env: Env,
        stream_wasm_hash: BytesN<32>,
    ) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.stream_wasm_hash = stream_wasm_hash;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_ttl(&env);
        Ok(())
    }

    /// Update the per-stream capacity cap.
    pub fn set_cap(env: Env, max_deposit: i128) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.max_deposit = max_deposit;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_ttl(&env);
        Ok(())
    }

    /// Update the minimum stream duration.
    pub fn set_min_duration(env: Env, min_duration: u64) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.min_duration = min_duration;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_ttl(&env);
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
        bump_ttl(&env);
        Ok(())
    }

    /// Pause or unpause stream creation.
    pub fn set_factory_paused(env: Env, paused: bool) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.creation_paused = paused;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_ttl(&env);
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
        bump_ttl(&env);
        Ok(())
    }
}

/// Extends the TTL of an allowlist entry when it is (re)written so a populated,
/// actively-queried allowlist stays readable between admin rotations.
fn bump_allowlist(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}
