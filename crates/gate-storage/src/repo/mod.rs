//! Repository 实现模块。
//!
//! 每个领域一个 trait + 一个 sqlx 实现：
//! - `UserRepo`        — 用户读写、状态变更
//! - `OrgRepo`         — Org CRUD
//! - `ProjectRepo`     — Project CRUD（按 Org 过滤）
//! - `MembershipRepo`  — Org/Project/Platform 三类成员
//! - `ApiKeyRepo`      — 按 hash 查、撤销、列表

pub mod api_key;
pub mod membership;
pub mod org;
pub mod project;
pub mod user;
