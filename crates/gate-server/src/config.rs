//! 配置加载：env (KOOIX_*) + 可选 kooix-gate.toml

use figment::providers::{Env, Format, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub public_url: String,
    pub database_url: String,
    pub redis_url: String,

    #[serde(default = "default_jwt_issuer")]
    pub jwt_issuer: String,
    #[serde(default = "default_jwt_audience")]
    pub jwt_audience: String,

    #[serde(default = "default_access_ttl_min")]
    pub token_access_ttl_min: i64,
    #[serde(default = "default_refresh_ttl_day")]
    pub token_refresh_ttl_day: i64,
}

fn default_jwt_issuer() -> String { "kooix-gate".into() }
fn default_jwt_audience() -> String { "kooix-gate-console".into() }
fn default_access_ttl_min() -> i64 { 15 }
fn default_refresh_ttl_day() -> i64 { 30 }

impl Config {
    /// 加载顺序：默认值 → kooix-gate.toml (可选) → KOOIX_* 环境变量
    pub fn load() -> Result<Self, figment::Error> {
        Figment::new()
            .merge(Toml::file("kooix-gate.toml"))
            .merge(Env::prefixed("KOOIX_"))
            .extract()
    }
}
