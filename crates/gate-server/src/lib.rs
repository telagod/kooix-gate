//! gate-server: Axum HTTP 层
//!
//! 模块分工：
//! - [`config`]    figment 配置加载（env + toml）
//! - [`state`]     AppState：JwtIssuer / AuthContextLoader / Clock
//! - [`error`]     AppError → IntoResponse 的统一映射
//! - [`auth`]      AuthContext FromRequestParts 抽取器
//! - [`routes`]    路由组装，按 scope 分目录
//! - [`middleware`] request_id / trace / cors / panic_catcher
//! - [`app`]       `build_router(state)` —— 单一入口，便于测试

pub mod config;
pub mod state;
pub mod error;
pub mod auth;
pub mod middleware;
pub mod routes;
pub mod app;
pub mod loader;
pub mod pg_loader;

pub use app::build_router;
pub use pg_loader::PgLoader;
pub use config::Config;
pub use error::AppError;
pub use state::AppState;
