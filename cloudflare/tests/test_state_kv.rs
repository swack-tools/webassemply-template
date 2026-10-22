use cf_worker_kv_template::auth::{
    extract_cookie_value, sign_session_token, verify_session_token, AuthError, SessionClaims,
    SESSION_COOKIE_NAME,
};
use cf_worker_kv_template::reconciler::run_reconciliation_tick;
use cf_worker_kv_template::state::{
    acquire_lease, compute_state_fingerprint, invalidate_state_cache, load_state, release_lease,
    save_state_if_changed, CacheSource, InMemoryKvStore, StateError, StateItem, WasiKeyValueBucket,
    WasmCloudKvAdapter, WriteOutcome, STATE_CACHE_TTL_SECS, WRITER_SLOT_EDGE,
};
use cf_worker_kv_template::ui::DASHBOARD_HTML;
use cf_worker_kv_template::{is_auth_path, is_control_path, normalize_path_for_blocking};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default)]
struct MockWasmCloudBucket {
    data: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl WasiKeyValueBucket for MockWasmCloudBucket {
    fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.data.borrow().get(key).cloned())
    }

    fn set_bytes(&self, key: &str, value: &[u8]) -> Result<(), String> {
        self.data
            .borrow_mut()
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn delete_key(&self, key: &str) -> Result<(), String> {
        self.data.borrow_mut().remove(key);
        Ok(())
    }
}

#[tokio::test]
async fn test_consolidated_state_cache_and_diff_gated_kv_writes() {
    invalidate_state_cache();
    let kv = InMemoryKvStore::new();
    let t0 = 1_760_000_000;

    // 1. Initial load creates default state in isolate memory with 1 KV read and 0 KV writes.
    let (mut bundle, source) = load_state(&kv, t0).await.expect("initial load");
    assert_eq!(source, CacheSource::DefaultCreated);
    assert_eq!(bundle.state_version, 0);
    assert_eq!(kv.read_count(), 1);
    assert_eq!(kv.write_count(), 0);

    // 2. Subsequent load within 60-second TTL hits isolate memory with 0 additional KV reads.
    let (_, source2) = load_state(&kv, t0 + 15).await.expect("warm cache load");
    assert_eq!(source2, CacheSource::MemoryHit);
    assert_eq!(kv.read_count(), 1);

    // 3. First save persists version 1 to KV.
    let outcome1 = save_state_if_changed(&kv, &mut bundle, WRITER_SLOT_EDGE, t0 + 20)
        .await
        .expect("first save");
    assert_eq!(outcome1, WriteOutcome::Written);
    assert_eq!(bundle.state_version, 1);
    assert_eq!(kv.write_count(), 1);

    // 4. Saving again without mutating state skips KV write completely.
    let outcome2 = save_state_if_changed(&kv, &mut bundle, WRITER_SLOT_EDGE, t0 + 30)
        .await
        .expect("unchanged save");
    assert_eq!(outcome2, WriteOutcome::SkippedUnchanged);
    assert_eq!(bundle.state_version, 1);
    assert_eq!(kv.write_count(), 1);

    // 5. Mutating an item changes the SHA-256 fingerprint and increments state_version to 2.
    let prev_fp = bundle.state_fingerprint.clone();
    bundle.state.items.insert(
        "runner-floor".to_string(),
        StateItem {
            key: "runner-floor".to_string(),
            value: "2".to_string(),
            updated_at: t0 + 40,
            updated_by: "swackhamer".to_string(),
        },
    );
    let outcome3 = save_state_if_changed(&kv, &mut bundle, WRITER_SLOT_EDGE, t0 + 40)
        .await
        .expect("mutated save");
    assert_eq!(outcome3, WriteOutcome::Written);
    assert_eq!(bundle.state_version, 2);
    assert_ne!(bundle.state_fingerprint, prev_fp);
    assert_eq!(kv.write_count(), 2);

    // 6. Advancing past the 60s TTL forces a fresh KV read.
    let (reloaded, source3) = load_state(&kv, t0 + 40 + STATE_CACHE_TTL_SECS + 1)
        .await
        .expect("expired cache load");
    assert_eq!(source3, CacheSource::KvRead);
    assert_eq!(reloaded.state_version, 2);
    assert_eq!(reloaded.state.items.get("runner-floor").unwrap().value, "2");
    assert_eq!(
        compute_state_fingerprint(&reloaded.state).unwrap(),
        reloaded.state_fingerprint
    );
}

#[tokio::test]
async fn test_distributed_kv_lease_contention_and_expiration() {
    invalidate_state_cache();
    let kv = InMemoryKvStore::new();
    let now = 1_760_000_000;

    // Writer A acquires a 120-second lease.
    let lease_a = acquire_lease(&kv, "cron-worker-a", now, 120)
        .await
        .expect("acquire lease A");
    assert_eq!(lease_a.holder, "cron-worker-a");
    assert_eq!(lease_a.expires_at, now + 120);

    // Writer B is blocked while Writer A's lease is active.
    let err = acquire_lease(&kv, "cron-worker-b", now + 30, 120)
        .await
        .expect_err("writer B should fail while lease active");
    assert!(matches!(
        err,
        StateError::LeaseHeld { ref holder, expires_at }
        if holder == "cron-worker-a" && expires_at == now + 120
    ));

    // Writer B cannot release Writer A's lease.
    let released_by_b = release_lease(&kv, "cron-worker-b")
        .await
        .expect("release attempt by B");
    assert!(!released_by_b);

    // After expiration, Writer B can take over the lease.
    let lease_b = acquire_lease(&kv, "cron-worker-b", now + 121, 120)
        .await
        .expect("writer B acquires expired lease");
    assert_eq!(lease_b.holder, "cron-worker-b");

    // Writer B releases its own lease cleanly.
    let released_by_b_after = release_lease(&kv, "cron-worker-b")
        .await
        .expect("release by B");
    assert!(released_by_b_after);
}

#[tokio::test]
async fn test_reconciler_zero_kv_state_writes_on_idle_tick() {
    invalidate_state_cache();
    let kv = InMemoryKvStore::new();
    let t0 = 1_760_000_000;

    // First tick initializes the default state bundle in KV (version 0 -> 1).
    let report1 = run_reconciliation_tick(&kv, "cron-1", t0, false)
        .await
        .expect("first tick");
    assert_eq!(report1.status, "ok");
    assert_eq!(report1.write_outcome, WriteOutcome::Written);
    assert_eq!(report1.state_version, 1);

    let writes_after_init = kv.write_count();

    // Second idle tick (record_heartbeat = false) acquires and releases the lease
    // but performs ZERO writes to `app:state` (`WriteOutcome::SkippedUnchanged`).
    let report2 = run_reconciliation_tick(&kv, "cron-2", t0 + 300, false)
        .await
        .expect("second idle tick");
    assert_eq!(report2.status, "ok");
    assert_eq!(report2.write_outcome, WriteOutcome::SkippedUnchanged);
    assert_eq!(report2.state_version, 1);
    // Only the 1 temporary `app:lease` put occurred; `app:state` was not rewritten.
    assert_eq!(kv.write_count(), writes_after_init + 1);
    assert_eq!(kv.delete_count(), 2);
}

#[test]
fn test_hmac_sha256_session_auth_and_revocation() {
    let secret = b"0123456789abcdef0123456789abcdef";
    let claims = SessionClaims {
        login: "swackhamer".to_string(),
        uid: 619624,
        iat: 1_760_000_000,
        exp: 1_760_003_600,
    };

    let token = sign_session_token(&claims, secret).expect("sign session");
    let cookie_header = format!("theme=dark; {}={}; Path=/", SESSION_COOKIE_NAME, token);
    let extracted = extract_cookie_value(&cookie_header, SESSION_COOKIE_NAME).expect("cookie");
    assert_eq!(extracted, token);

    // Valid verification
    let verified =
        verify_session_token(&extracted, secret, 1_760_001_000, None).expect("verify valid");
    assert_eq!(verified, claims);

    // Wrong secret fails constant-time signature check
    assert_eq!(
        verify_session_token(&extracted, b"wrong-secret-key", 1_760_001_000, None),
        Err(AuthError::InvalidSignature)
    );

    // Expired token rejected
    assert_eq!(
        verify_session_token(&extracted, secret, 1_760_003_600, None),
        Err(AuthError::Expired(1_760_003_600))
    );

    // SESSION_NOT_BEFORE cut-off revokes older tokens immediately
    assert_eq!(
        verify_session_token(&extracted, secret, 1_760_001_000, Some(1_760_000_500)),
        Err(AuthError::Revoked(1_760_000_500))
    );
}

#[test]
fn test_path_normalization_and_control_route_blocking() {
    assert_eq!(
        normalize_path_for_blocking("/API//State/../health").as_deref(),
        Some("/api/health")
    );
    assert!(is_control_path("/api/control"));
    assert!(is_control_path("/api/control/drain"));
    assert!(is_control_path("/api/%2e%2e/api/control/status"));
    assert!(!is_control_path("/api/controller-public"));

    assert!(is_auth_path("/auth/github/login"));
    assert!(is_auth_path("/api/auth/status"));
    assert!(!is_auth_path("/api/state"));
}

#[test]
fn test_embedded_ui_under_size_budget() {
    assert!(DASHBOARD_HTML.len() < 200 * 1024);
    assert!(DASHBOARD_HTML.contains("Cloudflare Worker + KV State Dashboard"));
}

#[tokio::test]
async fn test_wasmcloud_wasi_keyvalue_adapter_parity() {
    invalidate_state_cache();
    let kv = WasmCloudKvAdapter::new(MockWasmCloudBucket::default());
    let now = 1_760_000_000;

    let (mut bundle, source) = load_state(&kv, now).await.expect("wasmcloud initial load");
    assert_eq!(source, CacheSource::DefaultCreated);

    bundle.state.items.insert(
        "k8s-cluster".to_string(),
        StateItem {
            key: "k8s-cluster".to_string(),
            value: "wasmcloud-jetstream".to_string(),
            updated_at: now + 10,
            updated_by: "swackhamer".to_string(),
        },
    );

    let outcome = save_state_if_changed(&kv, &mut bundle, WRITER_SLOT_EDGE, now + 10)
        .await
        .expect("wasmcloud save");
    assert_eq!(outcome, WriteOutcome::Written);
    assert_eq!(bundle.state_version, 1);

    // Expire in-isolate cache and verify round-trip hydration from the wasmCloud byte bucket.
    invalidate_state_cache();
    let (hydrated, source2) = load_state(&kv, now + 20)
        .await
        .expect("wasmcloud reload from bucket");
    assert_eq!(source2, CacheSource::KvRead);
    assert_eq!(hydrated.state_version, 1);
    assert_eq!(
        hydrated.state.items.get("k8s-cluster").unwrap().value,
        "wasmcloud-jetstream"
    );
}
