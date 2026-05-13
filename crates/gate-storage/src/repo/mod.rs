//! Repository 实现模块。
//!
//! 每个领域一个 trait + 一个 sqlx 实现：
//! - `UserRepo`          — 用户读写、状态变更
//! - `OrgRepo`           — Org CRUD
//! - `ProjectRepo`       — Project CRUD（按 Org 过滤）
//! - `MembershipRepo`    — Org/Project/Platform 三类成员
//! - `ApiKeyRepo`        — 按 hash 查、撤销、列表
//! - `ChannelRepo`       — 渠道查询、分组内健康渠道列表
//! - `ChannelGroupRepo`  — 渠道分组 + Project 默认分组

pub mod api_key;
pub mod audit;
pub mod billing;
pub mod channel;
pub mod channel_key;
pub mod identity;
pub mod membership;
pub mod memory;
pub mod model_alias;
pub mod org;
pub mod project;
pub mod quota;
pub mod usage;
pub mod user;
