//! WASM auto-mount integration tests (0.4.168-171).
//!
//! 验 ProviderRouter::try_auto_mount_wasm_for_channel 4 类路径：
//! - 无 wasm 配置 → Ok(None)
//! - 无 blob store → Err(NoBlobStore)
//! - blob 找到 + sha256 一致 → Ok(Some(bytes))
//! - blob 找到 + sha256 篡改 → Err(Sha256Mismatch)

use std::sync::Arc;

use chrono::Utc;
use gate_core::id::ChannelId;
use gate_providers::router::{AutoMountError, ProviderRouter};
use gate_storage::ChannelRecord;
use gate_wasm::{LocalFsBlobStore, WasmBlobStore};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// In-memory blob store for tests — 不写盘
struct MemBlobStore {
    map: std::collections::HashMap<String, Vec<u8>>,
}

#[async_trait::async_trait]
impl WasmBlobStore for MemBlobStore {
    async fn fetch(&self, sha256: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.map.get(sha256).cloned())
    }
    fn name(&self) -> &'static str {
        "mem-test"
    }
}

fn make_channel(model_mapping: serde_json::Value) -> ChannelRecord {
    let now = Utc::now();
    ChannelRecord {
        channel_id: ChannelId::from(Uuid::now_v7()),
        code: "ch-wasm-test".into(),
        name: "ch-wasm-test".into(),
        provider_type: "plugin".into(),
        base_url: "http://localhost:9999".into(),
        supported_models: vec!["m1".into()],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60000,
        max_retries: 2,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping,
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn router_with_store(store: Arc<dyn WasmBlobStore>) -> ProviderRouter {
    use gate_storage::InMemoryChannelRepo;
    use gate_storage::InMemoryChannelGroupRepo;
    let ch = Arc::new(InMemoryChannelRepo::new());
    let gr = Arc::new(InMemoryChannelGroupRepo::new());
    ProviderRouter::new(ch, gr).with_wasm_blob_store(store)
}

fn router_without_store() -> ProviderRouter {
    use gate_storage::InMemoryChannelRepo;
    use gate_storage::InMemoryChannelGroupRepo;
    let ch = Arc::new(InMemoryChannelRepo::new());
    let gr = Arc::new(InMemoryChannelGroupRepo::new());
    ProviderRouter::new(ch, gr)
}

fn manifest_with_wasm(sha256: &str) -> serde_json::Value {
    serde_json::json!({
        "plugin": {
            "name": "test-plugin",
            "version": "0.1.0",
            "capabilities": { "chat": true },
            "security": {
                "wasm": {
                    "module": "modules/test.wasm",
                    "module_sha256": sha256,
                    "hooks": []
                }
            }
        }
    })
}

#[tokio::test]
async fn auto_mount_returns_none_when_channel_has_no_wasm() {
    let ch = make_channel(serde_json::json!({
        "plugin": {
            "name": "p",
            "version": "0.1.0",
            "capabilities": {"chat": true}
        }
    }));
    let router = router_with_store(Arc::new(MemBlobStore { map: Default::default() }));
    let res = router.try_auto_mount_wasm_for_channel(&ch).await;
    assert!(matches!(res, Ok(None)));
}

#[tokio::test]
async fn auto_mount_errors_when_no_blob_store() {
    let fake_sha = "0".repeat(64);
    let ch = make_channel(manifest_with_wasm(&fake_sha));
    let router = router_without_store();
    let res = router.try_auto_mount_wasm_for_channel(&ch).await;
    assert!(matches!(res, Err(AutoMountError::NoBlobStore)), "got: {res:?}");
}

#[tokio::test]
async fn auto_mount_succeeds_when_sha256_matches() {
    let bytes = b"\x00asm\x01\x00\x00\x00".to_vec(); // 极简 wasm header（不需真模块）
    let sha = hex::encode(Sha256::digest(&bytes));
    let mut map = std::collections::HashMap::new();
    map.insert(sha.clone(), bytes.clone());
    let store = Arc::new(MemBlobStore { map });

    let ch = make_channel(manifest_with_wasm(&sha));
    let router = router_with_store(store);
    let res = router.try_auto_mount_wasm_for_channel(&ch).await.expect("ok");
    assert_eq!(res, Some(bytes));
}

#[tokio::test]
async fn auto_mount_rejects_when_blob_bytes_tampered() {
    let real_bytes = b"\x00asm\x01\x00\x00\x00".to_vec();
    let claimed_sha = hex::encode(Sha256::digest(&real_bytes));
    // 篡改：blob store 在 claimed_sha 槽位放别的字节
    let mut map = std::collections::HashMap::new();
    let tampered = b"\x00asm\x01\x00\x00\x00XX".to_vec();
    map.insert(claimed_sha.clone(), tampered);
    let store = Arc::new(MemBlobStore { map });

    let ch = make_channel(manifest_with_wasm(&claimed_sha));
    let router = router_with_store(store);
    let res = router.try_auto_mount_wasm_for_channel(&ch).await;
    assert!(
        matches!(res, Err(AutoMountError::Sha256Mismatch { .. })),
        "got: {res:?}"
    );
}

#[tokio::test]
async fn auto_mount_not_found_when_sha_absent_from_store() {
    let absent_sha = "f".repeat(64);
    let store = Arc::new(MemBlobStore { map: Default::default() });
    let ch = make_channel(manifest_with_wasm(&absent_sha));
    let router = router_with_store(store);
    let res = router.try_auto_mount_wasm_for_channel(&ch).await;
    assert!(matches!(res, Err(AutoMountError::NotFound(_))), "got: {res:?}");
}

#[tokio::test]
async fn auto_mount_localfs_blob_store_real_disk_roundtrip() {
    // 真实 fs blob store — 写入 sha256.wasm 后 fetch
    let tmp = std::env::temp_dir().join(format!("kooix-wasm-test-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&tmp).await.expect("mkdir");
    let bytes = b"\x00asm\x01\x00\x00\x00real".to_vec();
    let sha = hex::encode(Sha256::digest(&bytes));
    tokio::fs::write(tmp.join(format!("{sha}.wasm")), &bytes)
        .await
        .expect("write");

    let store = Arc::new(LocalFsBlobStore::new(&tmp));
    let ch = make_channel(manifest_with_wasm(&sha));
    let router = router_with_store(store);
    let res = router.try_auto_mount_wasm_for_channel(&ch).await.expect("ok");
    assert_eq!(res, Some(bytes));

    let _ = tokio::fs::remove_dir_all(&tmp).await; // 清理
}

// ============================================================================
// 0.4.169（第四刀 #5 step 2）：batch auto-mount summary tests
// ============================================================================

#[tokio::test]
async fn batch_auto_mount_skipped_mounted_failed_counted_correctly() {
    use gate_providers::router::AutoMountOutcome;

    let bytes = b"\x00asm\x01\x00\x00\x00".to_vec();
    let sha_ok = hex::encode(Sha256::digest(&bytes));
    let sha_missing = "f".repeat(64);
    let sha_tampered = hex::encode(Sha256::digest(b"original"));

    let mut map = std::collections::HashMap::new();
    map.insert(sha_ok.clone(), bytes.clone());
    // tampered: 在 sha_tampered 槽位放别的字节
    map.insert(sha_tampered.clone(), b"DIFFERENT".to_vec());
    let store = Arc::new(MemBlobStore { map });

    let ch_no_wasm = make_channel(serde_json::json!({
        "plugin": { "name": "p", "version": "0.1.0", "capabilities": {"chat": true} }
    }));
    let ch_ok = make_channel(manifest_with_wasm(&sha_ok));
    let ch_missing = make_channel(manifest_with_wasm(&sha_missing));
    let ch_tampered = make_channel(manifest_with_wasm(&sha_tampered));

    let router = router_with_store(store);
    let channels = vec![ch_no_wasm, ch_ok, ch_missing, ch_tampered];
    let summary = router.auto_mount_wasm_for_channels(&channels).await;

    assert_eq!(summary.total(), 4);
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.mounted, 1);
    assert_eq!(summary.failed, 2, "missing + tampered = 2 failed");
    assert_eq!(summary.per_channel.len(), 4);

    // 验顺序保持 + 每个 outcome 类型对得上
    assert!(matches!(summary.per_channel[0].1, AutoMountOutcome::Skipped));
    assert!(matches!(
        summary.per_channel[1].1,
        AutoMountOutcome::Mounted { ref sha256, bytes: 8 } if *sha256 == sha_ok
    ));
    assert!(matches!(
        summary.per_channel[2].1,
        AutoMountOutcome::Failed { sha256: Some(ref s), .. } if *s == sha_missing
    ));
    assert!(matches!(
        summary.per_channel[3].1,
        AutoMountOutcome::Failed { sha256: Some(ref s), .. } if *s == sha_tampered
    ));
}

#[tokio::test]
async fn batch_auto_mount_empty_channels_returns_zero_summary() {
    let router = router_with_store(Arc::new(MemBlobStore { map: Default::default() }));
    let summary = router.auto_mount_wasm_for_channels(&[]).await;
    assert_eq!(summary.total(), 0);
    assert!(summary.per_channel.is_empty());
}
