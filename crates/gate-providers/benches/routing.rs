//! Criterion micro-benchmarks for ProviderRouter hot paths.
//!
//! These benchmarks use InMemory repos (no I/O) to isolate CPU-bound routing
//! logic: strategy selection, alias resolution, channel filtering, decrypted
//! channel key cache hit/miss cost.
//!
//! Run:  cargo bench --package gate-providers -- routing

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use gate_core::id::{ChannelGroupId, ChannelId, ChannelKeyId, ProjectId};
use gate_providers::{InflightTracker, ProviderRouter};
use gate_storage::{
    ChannelGroupRecord, ChannelKeyRecord, ChannelRecord, InMemoryChannelGroupRepo,
    InMemoryChannelKeyRepo, InMemoryChannelRepo, InMemoryModelAliasRepo, ModelAliasRecord,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_channel(code: &str, models: Vec<String>) -> ChannelRecord {
    let now = Utc::now();
    ChannelRecord {
        channel_id: ChannelId::from(Uuid::now_v7()),
        code: code.to_string(),
        name: code.to_string(),
        provider_type: "openai".to_string(),
        base_url: "http://localhost:9999".to_string(),
        supported_models: models,
        status: "active".to_string(),
        health: "healthy".to_string(),
        timeout_ms: 60000,
        max_retries: 2,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::Value::Object(Default::default()),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    }
}

/// Build a router with N channels using the given strategy.
fn build_router(
    n_channels: usize,
    strategy: &str,
    with_alias: bool,
) -> (ProjectId, ProviderRouter) {
    let project_id = ProjectId::from(Uuid::now_v7());
    let group_id = ChannelGroupId::from(Uuid::now_v7());

    let channel_repo = Arc::new(InMemoryChannelRepo::new());
    let group_repo = Arc::new(InMemoryChannelGroupRepo::new());

    let now = Utc::now();
    group_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "bench-group".to_string(),
        description: String::new(),
        strategy: strategy.to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
        use_health_score: false,
        health_weights: None,
    });
    group_repo.seed_default(project_id, group_id);

    for i in 0..n_channels {
        // Half support specific models, half are wildcards
        let models = if i % 2 == 0 {
            vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]
        } else {
            vec![] // wildcard
        };
        let ch = make_channel(&format!("ch-{i}"), models);
        let ch_id = ch.channel_id;
        channel_repo.seed_channel(ch);
        channel_repo.seed_binding(group_id, ch_id, i as i32, (i as i32 + 1).max(1));
    }

    let mut router = ProviderRouter::new(channel_repo, group_repo);

    if with_alias {
        let alias_repo = Arc::new(InMemoryModelAliasRepo::new());
        let pid_uuid = *project_id.as_uuid();
        let now = Utc::now();
        alias_repo.seed(ModelAliasRecord {
            id: Uuid::now_v7(),
            project_id: pid_uuid,
            alias: "fast-model".to_string(),
            target_model: "gpt-4o-mini".to_string(),
            group_id: None,
            params_override: serde_json::Value::Object(Default::default()),
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        alias_repo.seed(ModelAliasRecord {
            id: Uuid::now_v7(),
            project_id: pid_uuid,
            alias: "smart-model".to_string(),
            target_model: "gpt-4o".to_string(),
            group_id: None,
            params_override: serde_json::Value::Object(Default::default()),
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        router = router.with_model_alias_repo(alias_repo);
    }

    (project_id, router)
}

/// Build a router with one DB-backed encrypted channel key.
fn build_router_with_channel_key_cache(
    rt: &tokio::runtime::Runtime,
    ttl: Duration,
) -> (ProjectId, ProviderRouter) {
    let project_id = ProjectId::from(Uuid::now_v7());
    let group_id = ChannelGroupId::from(Uuid::now_v7());

    let channel_repo = Arc::new(InMemoryChannelRepo::new());
    let group_repo = Arc::new(InMemoryChannelGroupRepo::new());

    let now = Utc::now();
    group_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "key-cache-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
        use_health_score: false,
        health_weights: None,
    });
    group_repo.seed_default(project_id, group_id);

    let channel = make_channel("key-cache-ch", vec!["gpt-4o".to_string()]);
    let channel_id = channel.channel_id;
    channel_repo.seed_channel(channel);
    channel_repo.seed_binding(group_id, channel_id, 1, 100);

    let kms =
        gate_crypto::kms::EnvKms::from_b64(&gate_crypto::kms::generate_master_key_b64(), "bench")
            .unwrap();
    let sealer = Arc::new(gate_crypto::EnvelopeKms::new(kms));
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let key_enc = rt
        .block_on(sealer.seal(b"sk-bench-channel-key-secret", &aad))
        .unwrap();

    let key_repo = Arc::new(InMemoryChannelKeyRepo::new());
    key_repo.seed(ChannelKeyRecord {
        id: ChannelKeyId::from(Uuid::now_v7()),
        channel_id,
        label: Some("primary".to_string()),
        key_enc,
        key_fingerprint: "fp-bench-channel-key".to_string(),
        weight: 1,
        health: "healthy".to_string(),
        consecutive_errors: 0,
        total_requests: 0,
        total_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: now,
        updated_at: now,
    });

    let router = ProviderRouter::new(channel_repo, group_repo)
        .with_channel_key_repo(key_repo)
        .with_crypto(sealer)
        .with_channel_key_cache_ttl(ttl);

    (project_id, router)
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_route_priority(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("route_priority");
    for n in [1, 5, 10, 50, 100] {
        let (pid, router) = build_router(n, "priority", false);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                black_box(router.route(pid, "gpt-4o").await.unwrap());
            });
        });
    }
    group.finish();
}

fn bench_route_weighted_random(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("route_weighted_random");
    for n in [5, 10, 50] {
        let (pid, router) = build_router(n, "weighted_random", false);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                black_box(router.route(pid, "gpt-4o").await.unwrap());
            });
        });
    }
    group.finish();
}

fn bench_route_round_robin(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("route_round_robin");
    for n in [5, 10, 50] {
        let (pid, router) = build_router(n, "round_robin", false);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                black_box(router.route(pid, "gpt-4o").await.unwrap());
            });
        });
    }
    group.finish();
}

fn bench_route_least_conn(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("route_least_conn");
    for n in [5, 10, 50] {
        let (pid, router) = build_router(n, "least_conn", false);
        // Pre-warm inflight tracker with some load
        let tracker = router.inflight_tracker();
        for i in 0..n / 2 {
            // Simulate some channels having inflight requests
            tracker.acquire(ChannelId::from(Uuid::now_v7()));
            let _ = i;
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                let result = router.route(pid, "gpt-4o").await.unwrap();
                // Release so inflight doesn't grow unbounded
                if let Some(ref r) = result {
                    router.release_channel(r.channel_id);
                }
                black_box(result);
            });
        });
    }
    group.finish();
}

fn bench_route_with_alias(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (pid, router) = build_router(10, "priority", true);

    c.bench_function("route_with_alias_10ch", |b| {
        b.to_async(&rt).iter(|| async {
            black_box(router.route(pid, "fast-model").await.unwrap());
        });
    });
}

fn bench_channel_key_decrypt_cache(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("channel_key_decrypt_cache");

    for (name, ttl, warm_cache) in [
        ("cache_hit_30s", Duration::from_secs(30), true),
        ("cache_disabled", Duration::ZERO, false),
    ] {
        let (pid, router) = build_router_with_channel_key_cache(&rt, ttl);
        if warm_cache {
            rt.block_on(router.route(pid, "gpt-4o")).unwrap().unwrap();
        }
        group.bench_function(name, |b| {
            b.to_async(&rt).iter(|| async {
                black_box(router.route(pid, "gpt-4o").await.unwrap());
            });
        });
    }

    group.finish();
}

fn bench_route_model_not_found(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // All channels have specific models — requesting an unknown model
    let project_id = ProjectId::from(Uuid::now_v7());
    let group_id = ChannelGroupId::from(Uuid::now_v7());
    let channel_repo = Arc::new(InMemoryChannelRepo::new());
    let group_repo = Arc::new(InMemoryChannelGroupRepo::new());

    let now = Utc::now();
    group_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "bench-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
        use_health_score: false,
        health_weights: None,
    });
    group_repo.seed_default(project_id, group_id);

    for i in 0..20 {
        let ch = make_channel(&format!("ch-{i}"), vec!["specific-model".to_string()]);
        let ch_id = ch.channel_id;
        channel_repo.seed_channel(ch);
        channel_repo.seed_binding(group_id, ch_id, i, 1);
    }

    let router = ProviderRouter::new(channel_repo, group_repo);

    c.bench_function("route_model_not_found_20ch", |b| {
        b.to_async(&rt).iter(|| async {
            // Will scan all 20 channels, find none, then try fallback chain
            black_box(router.route(project_id, "nonexistent-model").await.unwrap());
        });
    });
}

fn bench_inflight_tracker(c: &mut Criterion) {
    let tracker = InflightTracker::new();
    let channels: Vec<ChannelId> = (0..100).map(|_| ChannelId::from(Uuid::now_v7())).collect();

    // Warm up — all channels known
    for ch in &channels {
        tracker.acquire(*ch);
        tracker.release(*ch);
    }

    let mut group = c.benchmark_group("inflight_tracker");

    group.bench_function("acquire_release", |b| {
        let mut idx = 0usize;
        b.iter(|| {
            let ch = channels[idx % channels.len()];
            tracker.acquire(ch);
            tracker.release(ch);
            idx += 1;
        });
    });

    group.bench_function("current_lookup", |b| {
        let mut idx = 0usize;
        b.iter(|| {
            let ch = channels[idx % channels.len()];
            black_box(tracker.current(ch));
            idx += 1;
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Group + main
// ---------------------------------------------------------------------------

criterion_group!(
    routing_benches,
    bench_route_priority,
    bench_route_weighted_random,
    bench_route_round_robin,
    bench_route_least_conn,
    bench_route_with_alias,
    bench_channel_key_decrypt_cache,
    bench_route_model_not_found,
    bench_inflight_tracker,
);

criterion_main!(routing_benches);
