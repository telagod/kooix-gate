use crate::error::{AppError, AppResult};
use gate_providers::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, ContentType, MessageContent, Role,
    ToolDef, Usage,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ResponsesRequest {
    pub(super) model: String,
    input: ResponseInput,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default, alias = "max_completion_tokens")]
    max_output_tokens: Option<u32>,
    #[serde(default)]
    tools: Option<Vec<ToolDef>>,
    #[serde(default)]
    tool_choice: Option<serde_json::Value>,
    #[serde(default, flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ResponseInput {
    Text(String),
    Items(Vec<ResponseInputItem>),
}

#[derive(Debug, Clone, Deserialize)]
struct ResponseInputItem {
    #[serde(default)]
    role: Option<Role>,
    #[serde(default)]
    content: Option<ResponseInputContent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ResponseInputContent {
    Text(String),
    Parts(Vec<ResponseContentPart>),
}

#[derive(Debug, Clone, Deserialize)]
struct ResponseContentPart {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    image_url: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ResponsesResponse {
    id: String,
    object: &'static str,
    created_at: i64,
    status: &'static str,
    model: String,
    output: Vec<ResponseOutputItem>,
    output_text: String,
    #[serde(default)]
    usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
struct ResponseOutputItem {
    #[serde(rename = "type")]
    kind: &'static str,
    id: String,
    status: &'static str,
    role: &'static str,
    content: Vec<ResponseOutputContent>,
}

#[derive(Debug, Clone, Serialize)]
struct ResponseOutputContent {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

pub(super) fn responses_to_chat_request(req: ResponsesRequest) -> AppResult<ChatRequest> {
    let mut messages = Vec::new();
    if let Some(instructions) = req.instructions
        && !instructions.trim().is_empty()
    {
        messages.push(ChatMessage::text(Role::System, instructions));
    }
    messages.extend(response_input_to_messages(req.input)?);
    if messages.is_empty() {
        return Err(AppError::BadRequest(
            "responses input must not be empty".into(),
        ));
    }

    Ok(ChatRequest {
        model: req.model,
        messages,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_output_tokens,
        stream: req.stream,
        tools: req.tools,
        tool_choice: req.tool_choice,
        extra: req.extra,
    })
}

fn response_input_to_messages(input: ResponseInput) -> AppResult<Vec<ChatMessage>> {
    match input {
        ResponseInput::Text(text) => Ok(vec![ChatMessage::text(Role::User, text)]),
        ResponseInput::Items(items) => items
            .into_iter()
            .map(|item| {
                Ok(ChatMessage {
                    role: item.role.unwrap_or(Role::User),
                    content: Some(match item.content {
                        Some(content) => response_content_to_message_content(content)?,
                        None => MessageContent::Text(String::new()),
                    }),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                })
            })
            .collect(),
    }
}

fn response_content_to_message_content(content: ResponseInputContent) -> AppResult<MessageContent> {
    match content {
        ResponseInputContent::Text(text) => Ok(MessageContent::Text(text)),
        ResponseInputContent::Parts(parts) => parts
            .into_iter()
            .map(|part| match part.kind.as_str() {
                "input_text" | "text" => Ok(ContentPart::Text {
                    r#type: ContentType::Text,
                    text: part.text.unwrap_or_default(),
                }),
                "input_image" | "image_url" => Ok(ContentPart::ImageUrl {
                    r#type: ContentType::ImageUrl,
                    image_url: gate_providers::ImageUrl {
                        url: response_image_url(part.image_url)?,
                        detail: None,
                    },
                }),
                other => Err(AppError::BadRequest(format!(
                    "unsupported responses input content type '{other}'"
                ))),
            })
            .collect::<AppResult<Vec<_>>>()
            .map(MessageContent::Parts),
    }
}

fn response_image_url(value: Option<serde_json::Value>) -> AppResult<String> {
    value
        .and_then(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| value.get("url")?.as_str().map(ToOwned::to_owned))
        })
        .ok_or_else(|| AppError::BadRequest("responses image input requires image_url".into()))
}

pub(super) fn chat_to_responses_response(resp: ChatResponse) -> ResponsesResponse {
    let output_text = resp
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_ref())
        .map(MessageContent::to_text)
        .unwrap_or_default();
    ResponsesResponse {
        id: resp.id.clone(),
        object: "response",
        created_at: chrono::Utc::now().timestamp(),
        status: "completed",
        model: resp.model,
        output: vec![ResponseOutputItem {
            kind: "message",
            id: format!("msg_{}", Uuid::now_v7().simple()),
            status: "completed",
            role: "assistant",
            content: vec![ResponseOutputContent {
                kind: "output_text",
                text: output_text.clone(),
            }],
        }],
        output_text,
        usage: resp.usage,
    }
}
