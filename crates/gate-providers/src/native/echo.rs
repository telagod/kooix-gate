//! echo —— native plane 通路验证 fixture（仅测试编译）。
//!
//! 不连任何上游，把最后一条 user message 原样回显。用于证明 `Provider` trait
//! 这层在 native 命名空间下能被路由选中、构造、调用——即 ADR-0005 "第一刀：先通路"。
//! 真实重渠道（kiro/windsurf）以此为模板，把 echo 的 body 换成上游协议逻辑。

use super::{NativeBuildContext, NativeProviderRegistration};
use crate::Provider;
use crate::capabilities::ProviderCapabilities;
use crate::error::ProviderResult;
use crate::types::{
    ChatChoice, ChatDelta, ChatMessage, ChatRequest, ChatResponse, ChatStreamChoice,
    ChatStreamChunk, FinishReason, Role, Usage,
};
use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use std::sync::Arc;

pub(super) fn registration() -> NativeProviderRegistration {
    NativeProviderRegistration {
        name: "echo",
        capabilities: ProviderCapabilities::chat_stream(),
        factory: Arc::new(|_ctx: &NativeBuildContext<'_>| {
            Ok(Arc::new(EchoProvider) as Arc<dyn Provider>)
        }),
    }
}

struct EchoProvider;

impl EchoProvider {
    fn last_user_text(req: &ChatRequest) -> String {
        req.messages
            .last()
            .map(|m| m.content_text().to_string())
            .unwrap_or_default()
    }
}

#[async_trait]
impl Provider for EchoProvider {
    fn name(&self) -> &'static str {
        "native:echo"
    }

    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
        let echoed = Self::last_user_text(&req);
        Ok(ChatResponse {
            id: "echo-0".to_string(),
            model: req.model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage::text(Role::Assistant, echoed),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: Usage::default(),
            request_id: None,
            upstream_metadata: None,
        })
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        let echoed = Self::last_user_text(&req);
        let chunk = ChatStreamChunk {
            id: "echo-0".to_string(),
            model: req.model,
            choices: vec![ChatStreamChoice {
                index: 0,
                delta: ChatDelta {
                    role: Some(Role::Assistant),
                    content: Some(echoed),
                    tool_calls: None,
                },
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: None,
        };
        Ok(Box::pin(stream::iter(vec![Ok(chunk)])))
    }
}
