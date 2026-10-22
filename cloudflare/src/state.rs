//! Consolidated Cloudflare Workers KV State Engine.
//!
//! Enforces four KV quota-conservation invariants:
//! 1. Single consolidated KV document (`app:state`, schema `cf-kv-state-snapshot-v1`).
//! 2. In-isolate Wasm memory cache (`STATE_CACHE`) with 60-second TTL fronting `STATE_KV.get`.
//! 3. Canonical SHA-256 `state_fingerprint` diff-gating so unchanged ticks perform zero KV writes.
//! 4. Distributed KV lease locking (`app:lease`) and monotonic `state_version` increments.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;

pub const KV_STATE_KEY: &str = "app:state";
pub const KV_LEASE_KEY: &str = "app:lease";
pub const SNAPSHOT_SCHEMA: &str = "cf-kv-state-snapshot-v1";
pub const WRITER_SLOT_EDGE: &str = "edge";
pub const WRITER_SLOT_RECONCILER: &str = "reconciler";
pub const STATE_CACHE_TTL_SECS: u64 = 60;
pub const DEFAULT_LEASE_TTL_SECS: u64 = 120;

// ============================================================================
// Consolidated Document Envelope & Domain Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateBundle {
    pub schema: String,
    pub state_version: u64,
    pub writer_slot: String,
    pub written_at: u64,
    #[serde(default)]
    pub state_fingerprint: String,
    pub state: AppState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppState {
    pub organization: String,
    pub target_repo: String,
    pub updated_at: u64,
    /// Using `BTreeMap` guarantees deterministic key ordering for canonical SHA-256 hashing.
    #[serde(default)]
    pub items: BTreeMap<String, StateItem>,
    #[serde(default)]
    pub reconciler_telemetry: ReconcilerTelemetry,
    #[serde(default)]
    pub admins: Vec<AdminRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateItem {
    pub key: String,
    pub value: String,
    pub updated_at: u64,
    pub updated_by: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcilerTelemetry {
    pub total_runs: u64,
    pub last_success_at: Option<u64>,
    pub last_status: String,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdminRecord {
    pub login: String,
    pub uid: u64,
    pub added_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseRecord {
    pub holder: String,
    pub acquired_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheSource {
    MemoryHit,
    KvRead,
    DefaultCreated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteOutcome {
    Written,
    SkippedUnchanged,
}

// ============================================================================
// In-Isolate Wasm Memory Cache (`thread_local!`)
// ============================================================================

thread_local! {
    static STATE_CACHE: RefCell<Option<(u64, StateBundle)>> = const { RefCell::new(None) };
}

pub fn get_cached_state(now: u64) -> Option<StateBundle> {
    STATE_CACHE.with(|cell| {
        let borrow = cell.borrow();
        if let Some((cached_at, ref bundle)) = *borrow {
            if now.saturating_sub(cached_at) < STATE_CACHE_TTL_SECS {
                return Some(bundle.clone());
            }
        }
        None
    })
}

pub fn set_cached_state(now: u64, bundle: StateBundle) {
    STATE_CACHE.with(|cell| {
        *cell.borrow_mut() = Some((now, bundle));
    });
}

pub fn invalidate_state_cache() {
    STATE_CACHE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

// ============================================================================
// Storage Backend Trait & Implementations
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("KV error: {0}")]
    Kv(String),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Lease conflict: held by '{holder}' until {expires_at}")]
    LeaseHeld { holder: String, expires_at: u64 },
}

#[async_trait::async_trait(?Send)]
pub trait KvStoreBackend {
    async fn get(&self, key: &str) -> Result<Option<String>, StateError>;
    async fn put(&self, key: &str, value: &str) -> Result<(), StateError>;
    async fn delete(&self, key: &str) -> Result<(), StateError>;
}

#[async_trait::async_trait(?Send)]
impl KvStoreBackend for worker::kv::KvStore {
    async fn get(&self, key: &str) -> Result<Option<String>, StateError> {
        self.get(key)
            .text()
            .await
            .map_err(|e| StateError::Kv(e.to_string()))
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), StateError> {
        self.put(key, value)
            .map_err(|e| StateError::Kv(e.to_string()))?
            .execute()
            .await
            .map_err(|e| StateError::Kv(e.to_string()))
    }

    async fn delete(&self, key: &str) -> Result<(), StateError> {
        self.delete(key)
            .await
            .map_err(|e| StateError::Kv(e.to_string()))
    }
}

/// Portable byte-oriented bucket trait matching `wasi:keyvalue/store` (`wasmcloud:keyvalue`)
/// so the same Rust state engine runs on both Cloudflare Workers (`worker::kv::KvStore` on
/// `wasm32-unknown-unknown`) and wasmCloud on Kubernetes (`wasm32-wasip2`).
pub trait WasiKeyValueBucket {
    fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn set_bytes(&self, key: &str, value: &[u8]) -> Result<(), String>;
    fn delete_key(&self, key: &str) -> Result<(), String>;
}

/// Adapter bridging any `wasi:keyvalue/store` (`wasmcloud:keyvalue`) bucket implementation
/// to [`KvStoreBackend`].
#[derive(Debug)]
pub struct WasmCloudKvAdapter<B> {
    bucket: B,
}

impl<B: WasiKeyValueBucket> WasmCloudKvAdapter<B> {
    pub fn new(bucket: B) -> Self {
        Self { bucket }
    }

    pub fn into_inner(self) -> B {
        self.bucket
    }
}

#[async_trait::async_trait(?Send)]
impl<B: WasiKeyValueBucket> KvStoreBackend for WasmCloudKvAdapter<B> {
    async fn get(&self, key: &str) -> Result<Option<String>, StateError> {
        match self.bucket.get_bytes(key).map_err(StateError::Kv)? {
            Some(bytes) => String::from_utf8(bytes).map(Some).map_err(|e| {
                StateError::Validation(format!("Invalid UTF-8 in KV key '{key}': {e}"))
            }),
            None => Ok(None),
        }
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), StateError> {
        self.bucket
            .set_bytes(key, value.as_bytes())
            .map_err(StateError::Kv)
    }

    async fn delete(&self, key: &str) -> Result<(), StateError> {
        self.bucket.delete_key(key).map_err(StateError::Kv)
    }
}

/// In-memory KV store for fast native unit and concurrency testing (`cargo test`)
/// with exact read, write, and delete operation counters.
#[derive(Debug, Default)]
pub struct InMemoryKvStore {
    entries: RefCell<BTreeMap<String, String>>,
    reads: RefCell<u64>,
    writes: RefCell<u64>,
    deletes: RefCell<u64>,
}

impl InMemoryKvStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_count(&self) -> u64 {
        *self.reads.borrow()
    }

    pub fn write_count(&self) -> u64 {
        *self.writes.borrow()
    }

    pub fn delete_count(&self) -> u64 {
        *self.deletes.borrow()
    }
}

#[async_trait::async_trait(?Send)]
impl KvStoreBackend for InMemoryKvStore {
    async fn get(&self, key: &str) -> Result<Option<String>, StateError> {
        *self.reads.borrow_mut() += 1;
        Ok(self.entries.borrow().get(key).cloned())
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), StateError> {
        *self.writes.borrow_mut() += 1;
        self.entries
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), StateError> {
        *self.deletes.borrow_mut() += 1;
        self.entries.borrow_mut().remove(key);
        Ok(())
    }
}

// ============================================================================
// State Hydration, Fingerprinting, and Diff-Gated Persistence
// ============================================================================

pub fn compute_state_fingerprint(state: &AppState) -> Result<String, StateError> {
    let canonical_json = serde_json::to_vec(state)?;
    let mut hasher = Sha256::new();
    hasher.update(&canonical_json);
    Ok(hex::encode(hasher.finalize()))
}

pub fn default_state_bundle(now: u64) -> Result<StateBundle, StateError> {
    let state = AppState {
        organization: "swack-tools".to_string(),
        target_repo: "swack-tools/oxidex".to_string(),
        updated_at: now,
        items: BTreeMap::new(),
        reconciler_telemetry: ReconcilerTelemetry {
            total_runs: 0,
            last_success_at: None,
            last_status: "idle".to_string(),
            stale: false,
        },
        admins: vec![AdminRecord {
            login: "swackhamer".to_string(),
            uid: 619624,
            added_at: now,
        }],
    };
    let fingerprint = compute_state_fingerprint(&state)?;
    Ok(StateBundle {
        schema: SNAPSHOT_SCHEMA.to_string(),
        state_version: 0,
        writer_slot: WRITER_SLOT_EDGE.to_string(),
        written_at: now,
        state_fingerprint: fingerprint,
        state,
    })
}

pub async fn load_state<K: KvStoreBackend>(
    kv: &K,
    now: u64,
) -> Result<(StateBundle, CacheSource), StateError> {
    if let Some(bundle) = get_cached_state(now) {
        return Ok((bundle, CacheSource::MemoryHit));
    }

    match kv.get(KV_STATE_KEY).await? {
        Some(raw) => {
            let bundle: StateBundle = serde_json::from_str(&raw)?;
            if bundle.schema != SNAPSHOT_SCHEMA {
                return Err(StateError::Validation(format!(
                    "Invalid schema '{}', expected '{}'",
                    bundle.schema, SNAPSHOT_SCHEMA
                )));
            }
            set_cached_state(now, bundle.clone());
            Ok((bundle, CacheSource::KvRead))
        }
        None => {
            let bundle = default_state_bundle(now)?;
            set_cached_state(now, bundle.clone());
            Ok((bundle, CacheSource::DefaultCreated))
        }
    }
}

/// Persists `bundle` only if its canonical SHA-256 `state_fingerprint` changed
/// (or if it has never been written to KV yet, `state_version == 0`).
pub async fn save_state_if_changed<K: KvStoreBackend>(
    kv: &K,
    bundle: &mut StateBundle,
    writer_slot: &str,
    now: u64,
) -> Result<WriteOutcome, StateError> {
    let next_fingerprint = compute_state_fingerprint(&bundle.state)?;
    if bundle.state_version > 0 && bundle.state_fingerprint == next_fingerprint {
        set_cached_state(now, bundle.clone());
        return Ok(WriteOutcome::SkippedUnchanged);
    }

    bundle.state_version += 1;
    bundle.written_at = now;
    bundle.writer_slot = writer_slot.to_string();
    bundle.state_fingerprint = next_fingerprint;

    let raw = serde_json::to_string(bundle)?;
    kv.put(KV_STATE_KEY, &raw).await?;
    set_cached_state(now, bundle.clone());
    Ok(WriteOutcome::Written)
}

// ============================================================================
// Distributed KV Lease Locking
// ============================================================================

pub async fn acquire_lease<K: KvStoreBackend>(
    kv: &K,
    holder: &str,
    now: u64,
    ttl_secs: u64,
) -> Result<LeaseRecord, StateError> {
    if holder.trim().is_empty() {
        return Err(StateError::Validation(
            "Lease holder must not be empty".to_string(),
        ));
    }

    if let Some(raw) = kv.get(KV_LEASE_KEY).await? {
        if let Ok(existing) = serde_json::from_str::<LeaseRecord>(&raw) {
            if existing.expires_at > now && existing.holder != holder {
                return Err(StateError::LeaseHeld {
                    holder: existing.holder,
                    expires_at: existing.expires_at,
                });
            }
        }
    }

    let lease = LeaseRecord {
        holder: holder.to_string(),
        acquired_at: now,
        expires_at: now.saturating_add(ttl_secs),
    };
    let serialized = serde_json::to_string(&lease)?;
    kv.put(KV_LEASE_KEY, &serialized).await?;
    Ok(lease)
}

pub async fn release_lease<K: KvStoreBackend>(kv: &K, holder: &str) -> Result<bool, StateError> {
    if let Some(raw) = kv.get(KV_LEASE_KEY).await? {
        if let Ok(existing) = serde_json::from_str::<LeaseRecord>(&raw) {
            if existing.holder != holder {
                return Ok(false);
            }
        }
        kv.delete(KV_LEASE_KEY).await?;
        return Ok(true);
    }
    Ok(false)
}
