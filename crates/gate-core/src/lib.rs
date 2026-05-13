//! gate-core: 领域模型 + Provider 抽象
//!
//! 这一层不依赖任何 I/O，纯类型和 trait。

pub mod id;
pub mod identity;
pub mod rbac;
pub mod quota;
pub mod error;

pub use error::{CoreError, Result};
