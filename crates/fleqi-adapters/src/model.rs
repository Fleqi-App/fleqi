//! OpenAI 兼容模型适配（architecture.md §9.1）：用户配置端点直连，无 Fleqi 代理。
//! 流式 SSE 解析、错误分类（认证/网络/限流/模型）、探测；密钥只经此处发往用户端点。

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::mpsc::Sender;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub display_name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamChunk {
    Delta { content: String },
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("认证失败：{message}")]
    AuthFailed { message: String },
    #[error("网络失败：{message}")]
    Network { message: String },
    #[error("限流：{message}")]
    RateLimited { message: String },
    #[error("模型不存在：{model}")]
    ModelNotFound { model: String },
    #[error("响应无效：{message}")]
    InvalidResponse { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Ok { models: Vec<String> },
    AuthFailed,
    Network { message: String },
}

pub trait ModelAdapter: Send + Sync {
    /// 流式对话；通道发送 Delta，最终 Done 或首个错误后关闭。
    fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        temperature: Option<f64>,
        events: Sender<Result<StreamChunk, ModelError>>,
    );
    /// 连接测试：只验证用户配置，不要求 App 账号。
    fn probe(&self) -> ProbeOutcome;
}

pub struct OpenAiCompatibleAdapter {
    config: ProviderConfig,
    client: reqwest::blocking::Client,
}

impl ProviderConfig {
    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim_end_matches('/'))
    }
}

fn classify_status(status: reqwest::StatusCode, body: &str, model: &str) -> ModelError {
    let brief: String = body.chars().take(200).collect();
    match status.as_u16() {
        401 | 403 => ModelError::AuthFailed { message: brief },
        404 => ModelError::ModelNotFound {
            model: model.to_owned(),
        },
        429 => ModelError::RateLimited { message: brief },
        _ => ModelError::InvalidResponse {
            message: format!("HTTP {status}: {brief}"),
        },
    }
}

impl OpenAiCompatibleAdapter {
    pub fn new(config: ProviderConfig) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms.max(1_000)))
            .build()
            .unwrap_or_default();
        Self { config, client }
    }
}

impl ModelAdapter for OpenAiCompatibleAdapter {
    fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        temperature: Option<f64>,
        events: Sender<Result<StreamChunk, ModelError>>,
    ) {
        let config = self.config.clone();
        let client = self.client.clone();
        std::thread::spawn(move || {
            let mut body =
                serde_json::json!({ "model": config.model, "messages": messages, "stream": true });
            if let Some(t) = temperature {
                body["temperature"] = serde_json::json!(t);
            }
            let mut request = client
                .post(config.endpoint("/chat/completions"))
                .json(&body);
            if let Some(key) = &config.api_key {
                request = request.bearer_auth(key);
            }
            let response = match request.send() {
                Ok(response) => response,
                Err(error) => {
                    let _ = events.send(Err(ModelError::Network {
                        message: error.to_string(),
                    }));
                    return;
                }
            };
            let status = response.status();
            if !status.is_success() {
                let body = response.text().unwrap_or_default();
                let _ = events.send(Err(classify_status(status, &body, &config.model)));
                return;
            }
            let mut sse = String::new();
            let mut reader = response;
            if let Err(error) = reader.read_to_string(&mut sse) {
                let _ = events.send(Err(ModelError::Network {
                    message: error.to_string(),
                }));
                return;
            }
            for line in sse.lines() {
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data.trim() == "[DONE]" {
                    let _ = events.send(Ok(StreamChunk::Done));
                    return;
                }
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(data)
                    && let Some(content) = payload["choices"][0]["delta"]["content"].as_str()
                {
                    let _ = events.send(Ok(StreamChunk::Delta {
                        content: content.to_owned(),
                    }));
                }
            }
            let _ = events.send(Ok(StreamChunk::Done));
        });
    }

    fn probe(&self) -> ProbeOutcome {
        let mut request = self.client.get(self.config.endpoint("/models"));
        if let Some(key) = &self.config.api_key {
            request = request.bearer_auth(key);
        }
        match request.send() {
            Ok(response) => {
                let status = response.status();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return ProbeOutcome::AuthFailed;
                }
                if !status.is_success() {
                    return ProbeOutcome::Network {
                        message: format!("HTTP {status}"),
                    };
                }
                match response.json::<serde_json::Value>() {
                    Ok(payload) => {
                        let models = payload["data"]
                            .as_array()
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(|item| item["id"].as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        ProbeOutcome::Ok { models }
                    }
                    Err(error) => ProbeOutcome::Network {
                        message: error.to_string(),
                    },
                }
            }
            Err(error) => ProbeOutcome::Network {
                message: error.to_string(),
            },
        }
    }
}

/// 应用层 ModelGateway 实现：按请求构造一次性适配器，流式通道聚合成完整回复，
/// 块间检查取消。密钥只随请求发往用户配置的端点（Bearer），不落日志、不进快照。
pub struct PlanningModelGateway;

impl fleqi_application::ports::ModelGateway for PlanningModelGateway {
    fn complete(
        &self,
        request: &fleqi_application::ports::ModelChatRequest,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<String, fleqi_application::ports::ModelGatewayError> {
        use fleqi_application::ports::ModelGatewayError;
        let adapter = OpenAiCompatibleAdapter::new(ProviderConfig {
            id: "planning".into(),
            display_name: "planning".into(),
            base_url: request.base_url.clone(),
            api_key: request.api_key.clone(),
            model: request.model.clone(),
            timeout_ms: request.timeout_ms,
        });
        let (tx, rx) = std::sync::mpsc::channel::<Result<StreamChunk, ModelError>>();
        ModelAdapter::chat_stream(
            &adapter,
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: request.system.clone(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: request.user.clone(),
                },
            ],
            None,
            tx,
        );
        let mut text = String::new();
        while let Ok(event) = rx.recv() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(ModelGatewayError::Cancelled);
            }
            match event {
                Ok(StreamChunk::Delta { content }) => text.push_str(&content),
                Ok(StreamChunk::Done) => return Ok(text),
                Err(ModelError::AuthFailed { message }) => {
                    return Err(ModelGatewayError::Auth { message });
                }
                Err(ModelError::Network { message }) => {
                    return Err(ModelGatewayError::Network { message });
                }
                Err(ModelError::RateLimited { message }) => {
                    return Err(ModelGatewayError::RateLimited { message });
                }
                Err(ModelError::ModelNotFound { model }) => {
                    return Err(ModelGatewayError::InvalidResponse {
                        message: format!("模型不存在：{model}"),
                    });
                }
                Err(ModelError::InvalidResponse { message }) => {
                    return Err(ModelGatewayError::InvalidResponse { message });
                }
            }
        }
        // 通道在 Done 之前关闭视为断流。
        if text.is_empty() {
            Err(ModelGatewayError::Network {
                message: "连接在收到任何内容前关闭".into(),
            })
        } else {
            Ok(text)
        }
    }
}
