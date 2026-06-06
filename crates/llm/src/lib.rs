use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{Stream, stream};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmResponse {
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

pub type LlmDeltaStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, messages: &[LlmMessage]) -> Result<LlmResponse>;

    async fn stream_complete(&self, messages: &[LlmMessage]) -> Result<LlmDeltaStream> {
        let response = self.complete(messages).await?;
        Ok(Box::pin(stream::iter(
            response
                .content
                .split_inclusive(['。', '！', '？', '\n'])
                .filter(|part| !part.is_empty())
                .map(|part| Ok(part.to_owned()))
                .collect::<Vec<_>>(),
        )))
    }
}

#[derive(Clone)]
pub struct OpenAiCompatibleClient {
    http: Client,
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: &'a [LlmMessage],
    temperature: f32,
    #[serde(rename = "max_tokens")]
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_thinking: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

impl OpenAiCompatibleClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    pub fn from_env_for_synthesis() -> Option<Self> {
        let base_url = std::env::var("OPENAI_COMPAT_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "https://dashscope.aliyuncs.com/compatible-mode/v1".to_owned());
        let api_key = std::env::var("OPENAI_COMPAT_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| std::env::var("DASHSCOPE_API_KEY").ok())?;
        let model = std::env::var("OPENAI_SYNTHESIS_MODEL")
            .or_else(|_| std::env::var("OPENAI_AGENT_MODEL"))
            .unwrap_or_else(|_| "qwen3.7-plus".to_owned());
        Some(Self::new(base_url, api_key, model))
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleClient {
    async fn complete(&self, messages: &[LlmMessage]) -> Result<LlmResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&ChatCompletionRequest {
                model: &self.model,
                messages,
                temperature: 0.2,
                max_tokens: 1600,
                enable_thinking: read_optional_bool(
                    std::env::var("OPENAI_COMPAT_ENABLE_THINKING")
                        .ok()
                        .as_deref(),
                    std::env::var("DASHSCOPE_ENABLE_THINKING").ok().as_deref(),
                ),
            })
            .send()
            .await
            .context("llm request failed")?
            .error_for_status()
            .context("llm returned non-success status")?
            .json::<ChatCompletionResponse>()
            .await
            .context("failed to parse llm response")?;

        let content = response
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .unwrap_or_default();

        Ok(LlmResponse {
            content,
            tool_calls: Vec::new(),
        })
    }
}

fn read_optional_bool(first: Option<&str>, second: Option<&str>) -> Option<bool> {
    first.or(second).and_then(|value| {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

#[derive(Debug, Clone)]
pub struct DeterministicLlm {
    reply: String,
}

impl DeterministicLlm {
    pub fn new(reply: impl Into<String>) -> Self {
        Self {
            reply: reply.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for DeterministicLlm {
    async fn complete(&self, _messages: &[LlmMessage]) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: self.reply.clone(),
            tool_calls: Vec::new(),
        })
    }
}
