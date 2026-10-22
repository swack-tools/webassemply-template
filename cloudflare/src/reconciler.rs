//! Scheduled Cron Reconciler with Lease Locking and Diff-Gated KV Persistence.

use crate::state::{
    acquire_lease, load_state, release_lease, save_state_if_changed, CacheSource, KvStoreBackend,
    StateError, WriteOutcome, DEFAULT_LEASE_TTL_SECS, WRITER_SLOT_RECONCILER,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcileReport {
    pub status: String,
    pub cache_source: CacheSource,
    pub write_outcome: WriteOutcome,
    pub state_version: u64,
    pub state_fingerprint: String,
    pub items_count: usize,
}

/// Runs an idempotent reconciliation tick guarded by a distributed KV lease.
/// When `mutate_items` is false and the state bundle is already initialized in KV,
/// the canonical SHA-256 fingerprint does not change and zero KV state writes occur.
pub async fn run_reconciliation_tick<K: KvStoreBackend>(
    kv: &K,
    holder: &str,
    now: u64,
    record_heartbeat: bool,
) -> Result<ReconcileReport, StateError> {
    match acquire_lease(kv, holder, now, DEFAULT_LEASE_TTL_SECS).await {
        Ok(_lease) => {}
        Err(StateError::LeaseHeld { .. }) => {
            let (bundle, cache_source) = load_state(kv, now).await?;
            return Ok(ReconcileReport {
                status: "skipped_lease_active".to_string(),
                cache_source,
                write_outcome: WriteOutcome::SkippedUnchanged,
                state_version: bundle.state_version,
                state_fingerprint: bundle.state_fingerprint,
                items_count: bundle.state.items.len(),
            });
        }
        Err(err) => return Err(err),
    }

    let result = async {
        let (mut bundle, cache_source) = load_state(kv, now).await?;

        if record_heartbeat {
            bundle.state.updated_at = now;
            bundle.state.reconciler_telemetry.total_runs += 1;
            bundle.state.reconciler_telemetry.last_success_at = Some(now);
            bundle.state.reconciler_telemetry.last_status = "ok".to_string();
            bundle.state.reconciler_telemetry.stale = false;
        }

        let write_outcome =
            save_state_if_changed(kv, &mut bundle, WRITER_SLOT_RECONCILER, now).await?;

        Ok(ReconcileReport {
            status: "ok".to_string(),
            cache_source,
            write_outcome,
            state_version: bundle.state_version,
            state_fingerprint: bundle.state_fingerprint,
            items_count: bundle.state.items.len(),
        })
    }
    .await;

    let _ = release_lease(kv, holder).await;
    result
}
