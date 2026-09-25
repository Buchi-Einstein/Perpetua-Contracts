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
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Instance entries are extended to the network-maxmium-adjacent window on every
/// mutating call (same values as the stream and governance contracts). A factory
/// that is merely read never bumps rent, by design — reads have no side effects.
const INSTANCE_LIFETIME_THRESHOLD: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 120_960;
const DEFAULT_CREATIONS_PER_WINDOW: u32 = 100;
const DEFAULT_RATE_LIMIT_WINDOW_LEDGERS: u32 = 17_280;
const DEFAULT_MAX_DURATION: u64 = 157_680_000;

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
    /// Presence of a token address under this key means it is approved.
    TokenAllowlist(Address),
    /// Per-sender creation bucket.
    RateLimit(Address),
}

/// Sliding-window bucket for one sender's factory creations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitBucket {
    pub window_start: u32,
    pub creations: u32,
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
    /// Maximum stream duration in seconds.
    pub max_duration: u64,
    /// When true, the batch creation path enforces `max_deposit` per stream
    /// (not merely per batch).
    pub batch_cap_enforced: bool,
    /// When true, `create_stream` is rejected while the factory is paused.
    pub creation_paused: bool,
    /// Optional lower bound on the per-second stream rate, applied at creation.
    pub min_rate_per_second: Option<i128>,
    /// Optional upper bound on the per-second stream rate, applied at creation.
    pub max_rate_per_second: Option<i128>,
    /// Maximum creations per sender during `rate_limit_window_ledgers`.
    pub max_creations_per_window: u32,
    /// Number of ledgers in each sender rate-limit window.
    pub rate_limit_window_ledgers: u32,
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
    pub max_duration: u64,
    pub batch_cap_enforced: bool,
    pub creation_paused: bool,
    pub min_rate_per_second: Option<i128>,
    pub max_rate_per_second: Option<i128>,
    pub max_creations_per_window: u32,
    pub rate_limit_window_ledgers: u32,
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
    /// The token is not approved by governance.
    TokenNotAllowed = 5,
    /// The sender has exhausted its current creation window.
    RateLimitExceeded = 6,
    /// A rate-limit quota or window must be greater than zero.
    InvalidRateLimit = 7,
    /// The stream duration is outside the configured policy bounds.
    InvalidDuration = 8,
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
        max_duration: config.max_duration,
        batch_cap_enforced: config.batch_cap_enforced,
        creation_paused: config.creation_paused,
        min_rate_per_second: config.min_rate_per_second,
        max_rate_per_second: config.max_rate_per_second,
        max_creations_per_window: config.max_creations_per_window,
        rate_limit_window_ledgers: config.rate_limit_window_ledgers,
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
        if min_duration > DEFAULT_MAX_DURATION {
            return Err(FactoryError::InvalidDuration);
        }

        let config = FactoryConfig {
            admin,
            stream_contract,
            max_deposit,
            min_duration,
            max_duration: DEFAULT_MAX_DURATION,
            // Documented init defaults for the optional axes.
            batch_cap_enforced: true,
            creation_paused: false,
            min_rate_per_second: None,
            max_rate_per_second: None,
            max_creations_per_window: DEFAULT_CREATIONS_PER_WINDOW,
            rate_limit_window_ledgers: DEFAULT_RATE_LIMIT_WINDOW_LEDGERS,
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

    /// True if `token` is approved for stream creation.
    ///
    /// # Errors
    /// - `NotInitialized`: No policy record exists.
    pub fn is_token_allowed(env: Env, token: Address) -> bool {
        load_config(&env).map_or_else(
            |e| panic_with_error!(&env, e),
            |_| {
                env.storage()
                    .persistent()
                    .has(&DataKey::TokenAllowlist(token))
            },
        )
    }

    /// Validate a token before forwarding a stream-creation request.
    pub fn validate_token(env: Env, token: Address) -> Result<(), FactoryError> {
        load_config(&env)?;
        if env
            .storage()
            .persistent()
            .has(&DataKey::TokenAllowlist(token))
        {
            Ok(())
        } else {
            Err(FactoryError::TokenNotAllowed)
        }
    }

    /// Validate a stream duration against the configured inclusive bounds.
    pub fn validate_duration(
        env: Env,
        start_time: u64,
        end_time: u64,
    ) -> Result<(), FactoryError> {
        let config = load_config(&env)?;
        let duration = end_time
            .checked_sub(start_time)
            .ok_or(FactoryError::InvalidDuration)?;
        if duration < config.min_duration || duration > config.max_duration {
            return Err(FactoryError::InvalidDuration);
        }
        Ok(())
    }

    /// Record one sender creation and reject exhausted buckets.
    ///
    /// Creation wrappers must call this before invoking the stream contract.
    /// The sender authorization binds the quota to the party that funds the
    /// creation, while transaction atomicity prevents failed downstream calls
    /// from consuming a slot.
    pub fn record_creation(env: Env, sender: Address) -> Result<(), FactoryError> {
        let config = load_config(&env)?;
        sender.require_auth();

        let key = DataKey::RateLimit(sender);
        let current_ledger = env.ledger().sequence();
        let mut bucket = env
            .storage()
            .persistent()
            .get::<_, RateLimitBucket>(&key)
            .unwrap_or(RateLimitBucket {
                window_start: current_ledger,
                creations: 0,
            });

        if current_ledger.saturating_sub(bucket.window_start)
            >= config.rate_limit_window_ledgers
        {
            bucket.window_start = current_ledger;
            bucket.creations = 0;
        }
        if bucket.creations >= config.max_creations_per_window {
            return Err(FactoryError::RateLimitExceeded);
        }

        bucket.creations += 1;
        env.storage().persistent().set(&key, &bucket);
        bump_allowlist(&env, &key);
        Ok(())
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
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.admin = new_admin;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
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
        if min_duration > config.max_duration {
            return Err(FactoryError::InvalidDuration);
        }
        config.min_duration = min_duration;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    /// Update the maximum stream duration in seconds.
    pub fn set_max_duration(env: Env, max_duration: u64) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        if max_duration < config.min_duration {
            return Err(FactoryError::InvalidDuration);
        }
        config.max_duration = max_duration;
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

    /// Add or remove a token from the approved-token allowlist.
    ///
    /// Removing a token that was never added is a safe no-op.
    pub fn set_token_allowed(env: Env, token: Address, allowed: bool) -> Result<(), FactoryError> {
        Self::guard(&env)?;
        let key = DataKey::TokenAllowlist(token);
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

    /// Pause or unpause all stream creation through the factory.
    pub fn set_pause(env: Env, paused: bool) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        config.creation_paused = paused;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
        Ok(())
    }

    /// Backward-compatible alias for `set_pause`.
    pub fn set_factory_paused(env: Env, paused: bool) -> Result<(), FactoryError> {
        Self::set_pause(env, paused)
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

    /// Configure the per-sender creation bucket.
    pub fn set_rate_limit(
        env: Env,
        max_creations_per_window: u32,
        rate_limit_window_ledgers: u32,
    ) -> Result<(), FactoryError> {
        let mut config = Self::guard(&env)?;
        if max_creations_per_window == 0 || rate_limit_window_ledgers == 0 {
            return Err(FactoryError::InvalidRateLimit);
        }
        config.max_creations_per_window = max_creations_per_window;
        config.rate_limit_window_ledgers = rate_limit_window_ledgers;
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance(&env);
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
