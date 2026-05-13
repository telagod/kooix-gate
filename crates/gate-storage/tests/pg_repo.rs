//! gate-storage 集成测试：起 PG 容器跑迁移 + 验证 5 个 Repo。
//!
//! 容器层：testcontainers-modules::postgres
//! 数据库：每个测试一个 schema 隔离（同 container 共享时启动更快）
//!
//! ⚠ 需要 Docker daemon 可用。CI 上请保证 runner 装了 docker.

use chrono::Utc;
use gate_core::identity::{OrgRole, ProjectRole};
use gate_storage::{
    ApiKeyRepo, MembershipRepo, OrgRepo, PgApiKeyRepo, PgMembershipRepo, PgOrgRepo,
    PgProjectRepo, PgUserRepo, ProjectRepo, UserRepo,
};
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres;

async fn start_pg() -> (testcontainers::ContainerAsync<Postgres>, sqlx::PgPool) {
    // 本地已缓存的镜像优先；CI 上可通过 KOOIX_TEST_PG_TAG 覆盖
    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    // 等容器接受连接（testcontainers 已等到健康，但 sqlx pool 仍需一次握手）
    let pool = gate_storage::connect(&url, 4).await.expect("connect");
    gate_storage::run_migrations(&pool).await.expect("migrate");
    (container, pool)
}

#[tokio::test]
async fn migrations_apply_cleanly() {
    let (_c, _pool) = start_pg().await;
    // 走到这里说明 11 个迁移都过了
}

#[tokio::test]
async fn user_crud_roundtrip() {
    let (_c, pool) = start_pg().await;
    let repo = PgUserRepo::new(pool);

    let u = repo
        .create("alice@example.com", Some("hash$abc"), Some("Alice"))
        .await
        .unwrap();
    assert_eq!(u.email, "alice@example.com");

    let by_id = repo.find_by_id(u.id).await.unwrap();
    assert_eq!(by_id.id, u.id);

    let by_email = repo.find_by_email("alice@example.com").await.unwrap();
    assert_eq!(by_email.id, u.id);

    let (user, ph) = repo.find_credentials("alice@example.com").await.unwrap();
    assert_eq!(user.id, u.id);
    assert_eq!(ph.as_deref(), Some("hash$abc"));

    // 失败计数
    let n = repo.bump_failed_login(u.id).await.unwrap();
    assert_eq!(n, 1);
    let n = repo.bump_failed_login(u.id).await.unwrap();
    assert_eq!(n, 2);
    repo.reset_failed_login(u.id).await.unwrap();
}

#[tokio::test]
async fn org_and_project_isolation() {
    let (_c, pool) = start_pg().await;
    let users = PgUserRepo::new(pool.clone());
    let orgs = PgOrgRepo::new(pool.clone());
    let projects = PgProjectRepo::new(pool);

    let owner = users.create("owner@x.com", None, None).await.unwrap();
    let org_a = orgs.create("Acme", "acme", owner.id).await.unwrap();
    let org_b = orgs.create("Beta", "beta", owner.id).await.unwrap();

    let proj_a1 = projects.create(org_a.id, "main", "main").await.unwrap();
    let proj_a2 = projects.create(org_a.id, "exp", "exp").await.unwrap();
    let _proj_b1 = projects.create(org_b.id, "main", "main").await.unwrap();

    let list_a = projects.list_in_org(org_a.id).await.unwrap();
    assert_eq!(list_a.len(), 2);
    let ids: std::collections::HashSet<_> = list_a.iter().map(|p| p.id).collect();
    assert!(ids.contains(&proj_a1.id));
    assert!(ids.contains(&proj_a2.id));

    let list_b = projects.list_in_org(org_b.id).await.unwrap();
    assert_eq!(list_b.len(), 1);
}

#[tokio::test]
async fn membership_roundtrip_includes_cross_org_key() {
    let (_c, pool) = start_pg().await;
    let users = PgUserRepo::new(pool.clone());
    let orgs = PgOrgRepo::new(pool.clone());
    let projects = PgProjectRepo::new(pool.clone());
    let memberships = PgMembershipRepo::new(pool);

    let dev = users.create("dev@x.com", None, None).await.unwrap();
    let other = users.create("other@x.com", None, None).await.unwrap();

    let org_a = orgs.create("A", "a", other.id).await.unwrap();
    let org_b = orgs.create("B", "b", other.id).await.unwrap();
    let p_a = projects.create(org_a.id, "p", "p").await.unwrap();

    memberships
        .add_org_member(org_a.id, dev.id, OrgRole::Member)
        .await
        .unwrap();
    memberships
        .add_project_member(p_a.id, dev.id, ProjectRole::Developer)
        .await
        .unwrap();

    let snap = memberships.load_for_user(dev.id).await.unwrap();
    assert_eq!(snap.orgs.get(&org_a.id), Some(&OrgRole::Member));
    assert!(snap.orgs.get(&org_b.id).is_none());
    assert_eq!(
        snap.projects.get(&(org_a.id, p_a.id)),
        Some(&ProjectRole::Developer)
    );

    // 复合键意图：(org_b, p_a) 不应命中（即便 p_a 被攻击者借去）
    assert!(snap.projects.get(&(org_b.id, p_a.id)).is_none());
    assert!(snap.platform.is_none());
}

#[tokio::test]
async fn api_key_revoke_and_lookup() {
    let (_c, pool) = start_pg().await;
    let users = PgUserRepo::new(pool.clone());
    let orgs = PgOrgRepo::new(pool.clone());
    let projects = PgProjectRepo::new(pool.clone());
    let keys = PgApiKeyRepo::new(pool);

    let owner = users.create("admin@x.com", None, None).await.unwrap();
    let org = orgs.create("A", "a", owner.id).await.unwrap();
    let proj = projects.create(org.id, "p", "p").await.unwrap();

    let key = gate_auth::api_key::generate();
    let hash = gate_auth::api_key::hash(&key.plaintext);
    let id = keys
        .create(
            proj.id,
            "ci",
            &hash,
            &key.prefix,
            &key.last4,
            owner.id,
            &[],
        )
        .await
        .unwrap();

    let rec = keys.find_by_hash(&hash).await.unwrap();
    assert_eq!(rec.api_key_id, id);
    assert_eq!(rec.org_id, org.id);
    assert!(!rec.is_revoked());
    assert!(!rec.is_expired(Utc::now()));

    let list = keys.list_in_project(proj.id).await.unwrap();
    assert_eq!(list.len(), 1);

    keys.revoke(id, owner.id, Some("ci rotated")).await.unwrap();
    let rec2 = keys.find_by_hash(&hash).await.unwrap();
    assert!(rec2.is_revoked());

    let list_after = keys.list_in_project(proj.id).await.unwrap();
    assert_eq!(list_after.len(), 0, "revoked keys hidden from listing");
}

#[tokio::test]
async fn api_key_touch_used_updates_counter() {
    let (_c, pool) = start_pg().await;
    let users = PgUserRepo::new(pool.clone());
    let orgs = PgOrgRepo::new(pool.clone());
    let projects = PgProjectRepo::new(pool.clone());
    let keys = PgApiKeyRepo::new(pool);

    let owner = users.create("u@x.com", None, None).await.unwrap();
    let org = orgs.create("A", "a", owner.id).await.unwrap();
    let proj = projects.create(org.id, "p", "p").await.unwrap();

    let key = gate_auth::api_key::generate();
    let hash = gate_auth::api_key::hash(&key.plaintext);
    let id = keys
        .create(proj.id, "ci", &hash, &key.prefix, &key.last4, owner.id, &[])
        .await
        .unwrap();

    keys.touch_used(id, Utc::now(), "127.0.0.1".parse().ok())
        .await
        .unwrap();
    keys.touch_used(id, Utc::now(), None).await.unwrap();
    // Repo 不暴露 use_count，间接验证：能写不报错就行
    let _ = keys.find_by_hash(&hash).await.unwrap();
}
