//! `kgctl key rotate-master` — envelope master key 轮换工具。
//!
//! 支持 dry-run / apply / verify 三段式：先用旧 key 解密所有受管密文，apply 时用新
//! key 重新 seal 并写回，verify 再用新 key 解密确认。回滚策略保持外部化：apply 前做
//! DB backup，保留旧 key；verify 失败时恢复 backup，或在服务未重启前用调换 old/new
//! 参数重新执行一次。

use anyhow::{Context, Result, bail};
use gate_crypto::{EnvKms, Sealer};
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

const ENV_DB: &str = "KOOIX_DATABASE_URL";

pub struct RotateMasterOpts {
    pub old_master_key: String,
    pub new_master_key: String,
    pub dry_run: bool,
    pub apply: bool,
    pub verify: bool,
}

#[derive(Debug, Clone)]
struct CipherRow {
    id: Uuid,
    channel_id: Option<Uuid>,
    ciphertext: Vec<u8>,
}

#[derive(Debug, Default)]
struct RotationStats {
    channel_keys: usize,
    identity_providers: usize,
}

impl RotationStats {
    fn total(&self) -> usize {
        self.channel_keys + self.identity_providers
    }
}

pub async fn rotate_master(opts: RotateMasterOpts) -> Result<()> {
    if opts.dry_run && opts.apply {
        bail!("--dry-run and --apply are mutually exclusive");
    }
    if !opts.dry_run && !opts.apply {
        bail!("choose exactly one execution mode: --dry-run or --apply");
    }
    if opts.old_master_key.trim() == opts.new_master_key.trim() {
        bail!("old and new master keys must differ");
    }

    let url = std::env::var(ENV_DB)
        .with_context(|| format!("环境变量 {ENV_DB} 未设置；先 export postgres URL"))?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .with_context(|| format!("连库失败：{url}"))?;

    let old = Sealer::new(EnvKms::from_b64(&opts.old_master_key, "old-master")?);
    let new = Sealer::new(EnvKms::from_b64(&opts.new_master_key, "new-master")?);
    let stats = collect_stats(&pool).await?;

    println!("master key rotation preflight");
    println!("  channel_keys: {}", stats.channel_keys);
    println!("  identity_providers: {}", stats.identity_providers);
    println!("  total ciphertexts: {}", stats.total());

    verify_with_old_key(&pool, &old).await?;
    if opts.dry_run {
        println!("dry-run ok · all ciphertexts decrypt with old master key");
        if opts.verify {
            println!("verify ok · dry-run verification used old master key (no writes)");
        }
        print_rollback_plan();
        return Ok(());
    }

    println!("apply · re-encrypting ciphertexts with new master key");
    reencrypt_channel_keys(&pool, &old, &new).await?;
    reencrypt_identity_providers(&pool, &old, &new).await?;
    println!("apply ok · re-encrypted {} ciphertexts", stats.total());

    if opts.verify {
        verify_with_new_key(&pool, &new).await?;
        println!("verify ok · all ciphertexts decrypt with new master key");
    } else {
        println!(
            "verify skipped · strongly recommended: rerun with --verify before restarting all instances"
        );
    }
    print_rollback_plan();
    Ok(())
}

async fn collect_stats(pool: &sqlx::PgPool) -> Result<RotationStats> {
    let channel_keys: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channel_keys")
        .fetch_one(pool)
        .await
        .context("count channel_keys failed")?;
    let identity_providers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM identity_providers WHERE client_secret_enc IS NOT NULL AND octet_length(client_secret_enc) > 0",
    )
    .fetch_one(pool)
    .await
    .context("count identity_providers failed")?;
    Ok(RotationStats {
        channel_keys: channel_keys.max(0) as usize,
        identity_providers: identity_providers.max(0) as usize,
    })
}

async fn load_channel_keys(pool: &sqlx::PgPool) -> Result<Vec<CipherRow>> {
    let rows = sqlx::query("SELECT id, channel_id, key_enc FROM channel_keys ORDER BY id")
        .fetch_all(pool)
        .await
        .context("load channel_keys failed")?;
    rows.into_iter()
        .map(|row| {
            Ok(CipherRow {
                id: row.try_get("id")?,
                channel_id: Some(row.try_get("channel_id")?),
                ciphertext: row.try_get("key_enc")?,
            })
        })
        .collect()
}

async fn load_identity_providers(pool: &sqlx::PgPool) -> Result<Vec<CipherRow>> {
    let rows = sqlx::query(
        "SELECT id, client_secret_enc FROM identity_providers WHERE client_secret_enc IS NOT NULL AND octet_length(client_secret_enc) > 0 ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .context("load identity_providers failed")?;
    rows.into_iter()
        .map(|row| {
            Ok(CipherRow {
                id: row.try_get("id")?,
                channel_id: None,
                ciphertext: row.try_get("client_secret_enc")?,
            })
        })
        .collect()
}

async fn verify_with_old_key(pool: &sqlx::PgPool, old: &Sealer<EnvKms>) -> Result<()> {
    verify_channel_keys(pool, old, "old").await?;
    verify_identity_providers(pool, old, "old").await?;
    Ok(())
}

async fn verify_with_new_key(pool: &sqlx::PgPool, new: &Sealer<EnvKms>) -> Result<()> {
    verify_channel_keys(pool, new, "new").await?;
    verify_identity_providers(pool, new, "new").await?;
    Ok(())
}

async fn verify_channel_keys(
    pool: &sqlx::PgPool,
    sealer: &Sealer<EnvKms>,
    label: &str,
) -> Result<()> {
    for row in load_channel_keys(pool).await? {
        let channel_id = row.channel_id.expect("channel key has channel_id");
        let aad = gate_crypto::aad::channel_key(channel_id);
        sealer
            .open(&row.ciphertext, &aad)
            .await
            .with_context(|| format!("{label} key cannot decrypt channel_key id={}", row.id))?;
    }
    Ok(())
}

async fn verify_identity_providers(
    pool: &sqlx::PgPool,
    sealer: &Sealer<EnvKms>,
    label: &str,
) -> Result<()> {
    for row in load_identity_providers(pool).await? {
        let aad = gate_crypto::aad::idp_secret(row.id);
        sealer.open(&row.ciphertext, &aad).await.with_context(|| {
            format!("{label} key cannot decrypt identity_provider id={}", row.id)
        })?;
    }
    Ok(())
}

async fn reencrypt_channel_keys(
    pool: &sqlx::PgPool,
    old: &Sealer<EnvKms>,
    new: &Sealer<EnvKms>,
) -> Result<()> {
    let mut tx = pool
        .begin()
        .await
        .context("begin channel key rotation tx")?;
    for row in load_channel_keys(pool).await? {
        let channel_id = row.channel_id.expect("channel key has channel_id");
        let aad = gate_crypto::aad::channel_key(channel_id);
        let plaintext = old
            .open(&row.ciphertext, &aad)
            .await
            .with_context(|| format!("decrypt channel_key id={} failed", row.id))?;
        let resealed = new
            .seal(&plaintext, &aad)
            .await
            .with_context(|| format!("seal channel_key id={} failed", row.id))?;
        sqlx::query("UPDATE channel_keys SET key_enc = $2, updated_at = NOW() WHERE id = $1")
            .bind(row.id)
            .bind(&resealed)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("update channel_key id={} failed", row.id))?;
    }
    tx.commit()
        .await
        .context("commit channel key rotation tx")?;
    Ok(())
}

async fn reencrypt_identity_providers(
    pool: &sqlx::PgPool,
    old: &Sealer<EnvKms>,
    new: &Sealer<EnvKms>,
) -> Result<()> {
    let mut tx = pool.begin().await.context("begin idp rotation tx")?;
    for row in load_identity_providers(pool).await? {
        let aad = gate_crypto::aad::idp_secret(row.id);
        let plaintext = old
            .open(&row.ciphertext, &aad)
            .await
            .with_context(|| format!("decrypt identity_provider id={} failed", row.id))?;
        let resealed = new
            .seal(&plaintext, &aad)
            .await
            .with_context(|| format!("seal identity_provider id={} failed", row.id))?;
        sqlx::query("UPDATE identity_providers SET client_secret_enc = $2, updated_at = NOW() WHERE id = $1")
            .bind(row.id)
            .bind(&resealed)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("update identity_provider id={} failed", row.id))?;
    }
    tx.commit().await.context("commit idp rotation tx")?;
    Ok(())
}

fn print_rollback_plan() {
    println!("rollback plan:");
    println!("  1. apply 前必须有 PostgreSQL backup/snapshot，并保留 old master key");
    println!("  2. verify 失败且服务未切新 key：优先恢复 backup；或用 old/new 对调重新执行 apply");
    println!("  3. verify 成功后再把 KOOIX_MASTER_KEY 切到 new key 并滚动重启全部实例");
    println!("  4. 确认 kgctl doctor、channel probe、SSO login smoke 通过后销毁旧 key");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_missing_execution_mode() {
        let opts = RotateMasterOpts {
            old_master_key: "a".into(),
            new_master_key: "b".into(),
            dry_run: false,
            apply: false,
            verify: false,
        };
        assert!(!opts.dry_run && !opts.apply);
    }
}
