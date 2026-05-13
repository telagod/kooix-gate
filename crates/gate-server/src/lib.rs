//! gate-server: Axum HTTP 层
//!
//! 模块分工：
//! - [`config`]    figment 配置加载（env + toml）
//! - [`state`]     AppState：JwtIssuer / AuthContextLoader / Clock
//! - [`error`]     AppError → IntoResponse 的统一映射
//! - [`auth`]      AuthContext FromRequestParts 抽取器
//! - [`routes`]    路由组装，按 scope 分目录
//! - [`middleware`] request_id / trace / cors / panic_catcher / metrics / rls
//! - [`billing_emit`] 把 usage 写到计费 outbox 的门面
//! - [`telemetry`] OpenTelemetry OTLP tracing 初始化
//! - [`metrics`]   Prometheus metrics recorder + /metrics endpoint
//! - [`app`]       `build_router(state)` —— 单一入口，便于测试

pub mod app;
pub mod audit;
pub mod auth;
pub mod billing_emit;
pub mod config;
pub mod cost_estimate;
pub mod error;
pub mod inflight;
pub mod loader;
pub mod metrics;
pub mod middleware;
pub mod pg_loader;
pub mod routes;
pub mod state;
pub mod telemetry;

pub use app::build_router;
pub use config::Config;
pub use error::AppError;
pub use pg_loader::PgLoader;
pub use state::AppState;
