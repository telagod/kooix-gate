//! Routed provider — 路由命中后返回给调用方的 4 种 wrapper（chat/embedding/image/audio）。

use super::metrics::ChannelMetrics;
use super::trace::RouteDecisionTrace;
use crate::{AudioProvider, EmbeddingProvider, ImageProvider, Provider};
use gate_core::id::{ChannelGroupId, ChannelId, ChannelKeyId};
use std::sync::Arc;

#[derive(Clone)]
pub struct RoutedProvider {
    pub provider: Arc<dyn Provider>,
    pub channel_id: ChannelId,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 从 channel 记录构造的 retry 配置。
    pub retry_config: crate::retry::RetryConfig,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
    /// params_override from model alias (empty object `{}` if no alias or no override).
    pub params_override: serde_json::Value,
    /// 命中 channel 的 provider_type（"anthropic", "bedrock", "gemini" 等）。
    /// 供调用方做参数适配（adapt_for_provider）。
    pub provider_type: String,
    /// 指向全局 ChannelMetrics，供调用方上报结果（auto-disable 机制）。
    pub metrics: Option<Arc<ChannelMetrics>>,
    /// 本次命中的路由决策轨迹，供审计、debug 与后续 snapshot 热更新验证。
    pub decision_trace: RouteDecisionTrace,
}

/// Embedding 路由命中结果：EmbeddingProvider + 绑定的 channel_id。
#[derive(Clone)]
pub struct RoutedEmbeddingProvider {
    pub provider: Arc<dyn EmbeddingProvider>,
    pub channel_id: ChannelId,
    pub group_id: ChannelGroupId,
    /// 命中 channel 的 provider_type，供 metrics / audit 使用。
    pub provider_type: String,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
}

/// Image 路由命中结果：ImageProvider + 绑定的 channel_id。
#[derive(Clone)]
pub struct RoutedImageProvider {
    pub provider: Arc<dyn ImageProvider>,
    pub channel_id: ChannelId,
    pub group_id: ChannelGroupId,
    /// 命中 channel 的 provider_type，供 metrics / audit 使用。
    pub provider_type: String,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
}

/// Audio 路由命中结果：AudioProvider + 绑定的 channel_id。
#[derive(Clone)]
pub struct RoutedAudioProvider {
    pub provider: Arc<dyn AudioProvider>,
    pub channel_id: ChannelId,
    pub group_id: ChannelGroupId,
    /// 命中 channel 的 provider_type，供 metrics / audit 使用。
    pub provider_type: String,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
}
