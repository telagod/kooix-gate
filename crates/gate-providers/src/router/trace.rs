//! Route trace types — candidate / skip / decision / miss / runtime snapshot.
//!
//! 这一层是 `ProviderRouter` 的可观测产出：每次路由决策都会写一份 `RouteDecisionTrace`，
//! miss 走 `RouteMissReason` → `RouteMiss::provider_error()`。

use crate::capabilities::ProviderCapability;
use crate::error::{NormalizedProviderErrorKind, ProviderError, ProviderErrorMetadata};
use gate_core::id::{ChannelGroupId, ChannelId};
use gate_storage::ChannelBinding;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// 单个候选 channel 在一次路由决策中的快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteCandidateTrace {
    pub group_id: ChannelGroupId,
    pub group_name: String,
    pub channel_id: ChannelId,
    pub channel_code: String,
    pub provider_type: String,
    pub priority: i32,
    pub weight: i32,
    pub canary_percent_bps: Option<i32>,
}

/// 候选 channel 被跳过的原因。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteSkipTrace {
    pub channel_id: ChannelId,
    pub channel_code: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RouteMissReason {
    NoDefaultGroup,
    NoHealthyChannels,
    ModelUnsupported,
    MissingCapability,
    NoActiveSecret,
    RateLimited,
    FallbackExhausted,
}

impl RouteMissReason {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            RouteMissReason::NoDefaultGroup => "no_default_group",
            RouteMissReason::NoHealthyChannels => "no_healthy_channels",
            RouteMissReason::ModelUnsupported => "model_unsupported",
            RouteMissReason::MissingCapability => "missing_capability",
            RouteMissReason::NoActiveSecret => "no_active_secret",
            RouteMissReason::RateLimited => "rate_limited",
            RouteMissReason::FallbackExhausted => "fallback_exhausted",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RouteMiss {
    pub(super) reason: RouteMissReason,
    pub(super) message: String,
    pub(super) capability: Option<ProviderCapability>,
    pub(super) selected_model: String,
    pub(super) trace: RouteDecisionTrace,
}

impl RouteMiss {
    pub(super) fn provider_error(self) -> ProviderError {
        let _ = (&self.capability, &self.selected_model, &self.trace);
        let metadata = ProviderErrorMetadata {
            kind: match self.reason {
                RouteMissReason::RateLimited => NormalizedProviderErrorKind::RateLimit,
                _ => NormalizedProviderErrorKind::ModelNotFound,
            },
            retryable: false,
            cooldown_ms: None,
            circuit_breaker_failures: None,
            retry_after_ms: None,
        };
        let status = match self.reason {
            RouteMissReason::RateLimited => Some(429),
            _ => Some(404),
        };
        let code = match self.reason {
            RouteMissReason::NoHealthyChannels => "no_healthy_channel",
            other => other.as_str(),
        };
        ProviderError::Mapped {
            status,
            code: Some(code.to_string()),
            message: self.message,
            metadata,
        }
    }
}

pub(super) enum RouteAttempt<T> {
    Routed(T),
    Miss(Box<RouteMiss>),
}

pub(super) fn route_not_found_message(kind: &str, model: &str, reason: RouteMissReason) -> String {
    format!(
        "no healthy {kind} channel found for model '{model}' ({})",
        reason.as_str()
    )
}

/// Provider 路由决策轨迹。
///
/// 当前 router 仍是 repo-backed lazy routing；`snapshot_version` 是热路径可观测钩子，
/// 后续切到 compiled `ProviderRuntimeSnapshot` 时无需改调用方契约。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteDecisionTrace {
    pub snapshot_version: u64,
    pub requested_model: String,
    /// alias 命中后的 target_model；未命中为 None。
    pub alias_target_model: Option<String>,
    /// alias 解析后的初始路由模型；未命中 alias 时等于 requested_model。
    pub initial_model: String,
    /// 实际选中的路由模型；可能来自 fallback model chain。
    pub selected_model: Option<String>,
    /// 经过 channel.model_mapping 后真正交给 provider 的模型名。
    pub provider_model: Option<String>,
    pub selected_group_id: Option<ChannelGroupId>,
    pub selected_group_name: Option<String>,
    pub selected_strategy: Option<String>,
    pub selected_channel_id: Option<ChannelId>,
    pub selected_channel_code: Option<String>,
    pub selected_provider_type: Option<String>,
    pub candidates: Vec<RouteCandidateTrace>,
    pub skipped: Vec<RouteSkipTrace>,
    pub fallbacks: Vec<String>,
}

/// Compiled provider runtime metadata snapshot.
///
/// The current router still resolves providers from repos lazily, but this
/// snapshot is already atomically replaceable and versioned. Control-plane code
/// can publish compiled channel/key metadata here before the route path fully
/// switches away from repo-backed reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRuntimeSnapshot {
    pub version: u64,
    pub compiled_at_unix_ms: u128,
    pub channels: Vec<ProviderRuntimeChannelSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRuntimeChannelSnapshot {
    pub channel_id: ChannelId,
    pub channel_code: String,
    pub provider_type: String,
    pub supported_models: Vec<String>,
    pub status: String,
    pub health: String,
}

impl ProviderRuntimeSnapshot {
    pub(super) fn new(version: u64, channels: Vec<ProviderRuntimeChannelSnapshot>) -> Self {
        Self {
            version,
            compiled_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or_default(),
            channels,
        }
    }
}

impl RouteDecisionTrace {
    pub(super) fn new(
        snapshot_version: u64,
        requested_model: &str,
        alias_target_model: Option<String>,
        initial_model: &str,
    ) -> Self {
        Self {
            snapshot_version,
            requested_model: requested_model.to_string(),
            alias_target_model,
            initial_model: initial_model.to_string(),
            selected_model: None,
            provider_model: None,
            selected_group_id: None,
            selected_group_name: None,
            selected_strategy: None,
            selected_channel_id: None,
            selected_channel_code: None,
            selected_provider_type: None,
            candidates: Vec::new(),
            skipped: Vec::new(),
            fallbacks: Vec::new(),
        }
    }

    pub(super) fn record_candidates(
        &mut self,
        group: &gate_storage::ChannelGroupRecord,
        ordered: &[&ChannelBinding],
    ) {
        self.candidates
            .extend(ordered.iter().map(|candidate| RouteCandidateTrace {
                group_id: group.group_id,
                group_name: group.name.clone(),
                channel_id: candidate.channel.channel_id,
                channel_code: candidate.channel.code.clone(),
                provider_type: candidate.channel.provider_type.clone(),
                priority: candidate.priority,
                weight: candidate.weight,
                canary_percent_bps: candidate.canary_percent_bps,
            }));
    }

    pub(super) fn record_skip(&mut self, candidate: &ChannelBinding, reason: &str) {
        self.skipped.push(RouteSkipTrace {
            channel_id: candidate.channel.channel_id,
            channel_code: candidate.channel.code.clone(),
            reason: reason.to_string(),
        });
    }

    pub(super) fn record_selected(
        &mut self,
        group: &gate_storage::ChannelGroupRecord,
        candidate: &ChannelBinding,
        selected_model: &str,
        provider_model: &str,
    ) {
        self.selected_model = Some(selected_model.to_string());
        self.provider_model = Some(provider_model.to_string());
        self.selected_group_id = Some(group.group_id);
        self.selected_group_name = Some(group.name.clone());
        self.selected_strategy = Some(group.strategy.clone());
        self.selected_channel_id = Some(candidate.channel.channel_id);
        self.selected_channel_code = Some(candidate.channel.code.clone());
        self.selected_provider_type = Some(candidate.channel.provider_type.clone());
    }
}
