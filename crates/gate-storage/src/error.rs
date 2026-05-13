//! 统一的存储层错误。
//!
//! 设计意图：上层只关心 NotFound / Conflict / Internal，不暴露 sqlx 细节。
//! 写一次 [`From<sqlx::Error>`]，所有 Repo 都能 `?`。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("entity not found")]
    NotFound,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("constraint violation: {0}")]
    Constraint(String),

    #[error("internal db error: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(db) => {
                // PostgreSQL 23505 = unique violation, 23503 = FK violation
                match db.code().as_deref() {
                    Some("23505") => Self::Conflict(db.message().to_string()),
                    Some("23503" | "23502" | "23514") => Self::Constraint(db.message().to_string()),
                    _ => Self::Internal(e.to_string()),
                }
            }
            _ => Self::Internal(e.to_string()),
        }
    }
}

pub type DbResult<T> = Result<T, DbError>;
