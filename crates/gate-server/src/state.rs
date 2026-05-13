//! AppState: 跨 handler 共享的依赖
//!
//! 用 Arc 实现 Clone 廉价化。

use crate::loader::AuthContextLoader;
use gate_auth::jwt::JwtIssuer;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub jwt: Arc<JwtIssuer>,
    pub loader: Arc<dyn AuthContextLoader>,
}

impl AppState {
    pub fn new(jwt: JwtIssuer, loader: Arc<dyn AuthContextLoader>) -> Self {
        Self {
            jwt: Arc::new(jwt),
            loader,
        }
    }
}
