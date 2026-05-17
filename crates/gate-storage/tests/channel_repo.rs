//! gate-storage channel/channel_group 集成测试。

use gate_core::id::{ChannelGroupId, ChannelId};
use gate_storage::{
    ChannelGroupRepo, ChannelRepo, OrgRepo, PgChannelGroupRepo, PgChannelRepo, PgOrgRepo,
    PgProjectRepo, PgUserRepo, ProjectRepo, UserRepo,
};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

async fn start_pg() -> (testcontainers::ContainerAsync<Postgres>, sqlx::PgPool) {
    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = gate_storage::connect(&url, 4).await.expect("connect");
    gate_storage::run_migrations(&pool).await.expect("migrate");
    (container, pool)
}

/// seed 两个 channel 到一个 group，一个 enabled+healthy，一个 disabled。
/// list_healthy_in_group 只应返回 enabled+healthy 那条。
#[tokio::test]
async fn list_healthy_in_group_filters_disabled() {
    let (_c, pool) = start_pg().await;

    // 插入两个 channel
    let ch_enabled_id: Uuid = sqlx::query_scalar(
        "INSERT INTO channels (code, name, provider_type, base_url, config_enc, status, health) \
         VALUES ('ch-enabled', 'Enabled Ch', 'openai', 'https://api.openai.com/v1', '\\x'::bytea, 'active', 'healthy') \
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let ch_disabled_id: Uuid = sqlx::query_scalar(
        "INSERT INTO channels (code, name, provider_type, base_url, config_enc, status, health) \
         VALUES ('ch-disabled', 'Disabled Ch', 'openai', 'https://api.openai.com/v1', '\\x'::bytea, 'disabled', 'healthy') \
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // 插入 group
    let group_id: Uuid = sqlx::query_scalar(
        "INSERT INTO channel_groups (name, strategy) VALUES ('test-group', 'priority') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // 绑定两个 channel（enabled binding）
    sqlx::query(
        "INSERT INTO channel_group_bindings (group_id, channel_id, priority, weight, enabled) \
         VALUES ($1, $2, 10, 1, TRUE), ($1, $3, 20, 1, TRUE)",
    )
    .bind(group_id)
    .bind(ch_enabled_id)
    .bind(ch_disabled_id)
    .execute(&pool)
    .await
    .unwrap();

    let repo = PgChannelRepo::new(pool.clone());
    let gid = ChannelGroupId::from(group_id);
    let bindings = repo.list_healthy_in_group(gid).await.unwrap();

    // 只有 enabled+healthy 的 channel
    assert_eq!(bindings.len(), 1, "expected 1 healthy channel");
    assert_eq!(
        bindings[0].channel.channel_id,
        ChannelId::from(ch_enabled_id)
    );
}

/// find_default_for_project 通过 projects.default_group_id 找到分组。
#[tokio::test]
async fn find_default_for_project() {
    let (_c, pool) = start_pg().await;

    let users = PgUserRepo::new(pool.clone());
    let orgs = PgOrgRepo::new(pool.clone());
    let projects = PgProjectRepo::new(pool.clone());

    let owner = users
        .create("owner@grp.com", None, None, None)
        .await
        .unwrap();
    let org = orgs.create("GrpOrg", "grporg", owner.id).await.unwrap();
    let proj = projects.create(org.id, "GrpProj", "grpproj").await.unwrap();

    // 插入 group
    let group_id: Uuid = sqlx::query_scalar(
        "INSERT INTO channel_groups (name, strategy) VALUES ('proj-group', 'weighted_random') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // 设置 project 默认 group
    sqlx::query("UPDATE projects SET default_group_id = $1 WHERE id = $2")
        .bind(group_id)
        .bind(proj.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let group_repo = PgChannelGroupRepo::new(pool.clone());
    let found = group_repo.find_default_for_project(proj.id).await.unwrap();
    assert_eq!(found.group_id, ChannelGroupId::from(group_id));
    assert_eq!(found.name, "proj-group");
}

/// list_healthy_in_group 按 priority ASC 排序。
#[tokio::test]
async fn list_healthy_in_group_priority_order() {
    let (_c, pool) = start_pg().await;

    // 插入三个 healthy channel
    let mut ids: Vec<Uuid> = Vec::new();
    for i in 1u8..=3 {
        let id: Uuid = sqlx::query_scalar(&format!(
            "INSERT INTO channels (code, name, provider_type, base_url, config_enc, status, health) \
             VALUES ('ch-prio-{i}', 'Ch {i}', 'openai', 'https://api.openai.com/v1', '\\x'::bytea, 'active', 'healthy') \
             RETURNING id"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        ids.push(id);
    }

    let group_id: Uuid = sqlx::query_scalar(
        "INSERT INTO channel_groups (name, strategy) VALUES ('prio-group', 'priority') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // 故意乱序插入 priority
    for (ch_id, prio) in ids.iter().zip([30i32, 10, 20]) {
        sqlx::query(
            "INSERT INTO channel_group_bindings (group_id, channel_id, priority, weight, enabled) \
             VALUES ($1, $2, $3, 1, TRUE)",
        )
        .bind(group_id)
        .bind(ch_id)
        .bind(prio)
        .execute(&pool)
        .await
        .unwrap();
    }

    let repo = PgChannelRepo::new(pool.clone());
    let bindings = repo
        .list_healthy_in_group(ChannelGroupId::from(group_id))
        .await
        .unwrap();

    assert_eq!(bindings.len(), 3);
    let priorities: Vec<i32> = bindings.iter().map(|b| b.priority).collect();
    assert_eq!(priorities, vec![10, 20, 30], "should be sorted ASC");
}
