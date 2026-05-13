//! gate-core: 领域模型 + Provider 抽象
//!
//! 这一层不依赖任何 I/O，纯类型和 trait。

pub mod error;
pub mod id;
pub mod identity;
pub mod quota;
pub mod rbac;

pub use error::{CoreError, Result};
