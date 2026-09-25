//! M3.2 模型适配测试：OpenAI 兼容端点的请求构造、流式解析、错误分类与探测。
//! 用本地 spin-up 的 mock HTTP 服务验证真实网络路径（不经 Fleqi 代理）。

use fleqi_adapters::model::{
    ChatMessage, ModelAdapter, ModelError, OpenAiCompatibleAdapter, ProbeOutcome, ProviderConfig,
    StreamChunk,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

struct MockServer {
    base_url: String,
}

fn spawn_server(
    response: &'static str,
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::fs::write("/tmp/fleqi-model-mock-port", port.to_string()).unwrap();
    std::thread::spawn(move || {
        // 每个连接处理一个请求：按 Content-Length 读完 body 后应答并半关连接。
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut raw = Vec::new();
            let mut buffer = [0u8; 8192];
            let header_end = loop {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => {
                        raw.extend_from_slice(&buffer[..n]);
                        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(pos + 4);
                        }
                    }
                }
            };
            let Some(header_end) = header_end else {
                continue;
            };
            let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
            let content_length: usize = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|v| v.trim().parse().unwrap_or(0))
                })
                .unwrap_or(0);
            while raw.len() < header_end + content_length {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&buffer[..n]),
                }
            }
            requests
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&raw).into_owned());
            // Connection: close 语义：应答后关闭连接，让客户端读到 EOF（无 Content-Length 的响应体）。
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    MockServer {
        base_url: format!("http://127.0.0.1:{port}"),
    }
}

fn provider(base_url: &str) -> ProviderConfig {
    ProviderConfig {
        id: "test-provider".into(),
        display_name: "测试端点".into(),
        base_url: base_url.into(),
        api_key: Some("test-key".into()),
        model: "test-model".into(),
        timeout_ms: 5_000,
    }
}

const SSE_RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"，世界\"}}]}\n\n\
data: [DONE]\n\n";

const ERROR_401: &str = "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\r\n\
{\"error\":{\"message\":\"bad key\"}}";

const PROBE_OK: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n\
{\"data\":[{\"id\":\"test-model\"},{\"id\":\"other-model\"}]}";

#[test]
fn streams_sse_chunks_and_carries_authorization_header() {
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = spawn_server(SSE_RESPONSE, Arc::clone(&requests));
    let adapter = OpenAiCompatibleAdapter::new(provider(&server.base_url));
    let (tx, rx): (_, Receiver<Result<StreamChunk, ModelError>>) = std::sync::mpsc::channel();
    adapter.chat_stream(
        vec![ChatMessage {
            role: "user".into(),
            content: "总结".into(),
        }],
        None,
        tx,
    );
    let mut text = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(StreamChunk::Delta { content })) => text.push_str(&content),
            Ok(Ok(StreamChunk::Done)) => break,
            Ok(Err(error)) => panic!("流错误：{error:?}"),
            Err(_) => continue,
        }
    }
    assert_eq!(text, "你好，世界");
    let recorded = requests.lock().unwrap().join("\n");
    let recorded_lower = recorded.to_ascii_lowercase();
    assert!(
        recorded_lower.contains("authorization: bearer test-key"),
        "请求应携带用户密钥"
    );
    assert!(recorded.contains("\"model\":\"test-model\""));
}

#[test]
fn http_error_maps_to_auth_failed() {
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = spawn_server(ERROR_401, Arc::clone(&requests));
    let adapter = OpenAiCompatibleAdapter::new(provider(&server.base_url));
    let (tx, rx): (_, Receiver<Result<StreamChunk, ModelError>>) = std::sync::mpsc::channel();
    adapter.chat_stream(
        vec![ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
        None,
        tx,
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut failure = None;
    while std::time::Instant::now() < deadline {
        if let Ok(Err(error)) = rx.recv_timeout(Duration::from_millis(100)) {
            failure = Some(error);
            break;
        }
    }
    let failure = failure.expect("401 应返回错误");
    assert!(
        matches!(failure, ModelError::AuthFailed { .. }),
        "{failure:?}"
    );
}

#[test]
fn probe_lists_models_without_account() {
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = spawn_server(PROBE_OK, Arc::clone(&requests));
    let adapter = OpenAiCompatibleAdapter::new(provider(&server.base_url));
    let outcome = adapter.probe();
    assert!(
        matches!(outcome, ProbeOutcome::Ok { ref models } if models.len() == 2),
        "{outcome:?}"
    );
}

#[test]
fn model_gateway_accumulates_stream_into_complete_reply() {
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = spawn_server(SSE_RESPONSE, Arc::clone(&requests));
    let gateway = fleqi_adapters::model::PlanningModelGateway;
    let request = fleqi_application::ports::ModelChatRequest {
        base_url: server.base_url.clone(),
        api_key: Some("test-key".into()),
        model: "test-model".into(),
        timeout_ms: 10_000,
        system: "只输出 JSON".into(),
        user: "计划".into(),
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let reply = fleqi_application::ports::ModelGateway::complete(&gateway, &request, &cancel)
        .expect("聚合回复");
    assert_eq!(reply, "你好，世界");
    let recorded = requests.lock().unwrap().join("\n");
    assert!(recorded.contains("只输出 JSON"));
    assert!(recorded.contains("计划"));
}

#[test]
fn model_gateway_maps_auth_failure() {
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = spawn_server(ERROR_401, Arc::clone(&requests));
    let gateway = fleqi_adapters::model::PlanningModelGateway;
    let request = fleqi_application::ports::ModelChatRequest {
        base_url: server.base_url.clone(),
        api_key: Some("test-key".into()),
        model: "test-model".into(),
        timeout_ms: 10_000,
        system: "s".into(),
        user: "u".into(),
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let error = fleqi_application::ports::ModelGateway::complete(&gateway, &request, &cancel)
        .expect_err("401 应失败");
    assert!(
        matches!(
            error,
            fleqi_application::ports::ModelGatewayError::Auth { .. }
        ),
        "{error:?}"
    );
}

#[test]
fn model_gateway_cancel_before_request_short_circuits() {
    let gateway = fleqi_adapters::model::PlanningModelGateway;
    let request = fleqi_application::ports::ModelChatRequest {
        // 指向保证拒绝连接的回环端口，不依赖外网。
        base_url: "http://127.0.0.1:9/v1".into(),
        api_key: None,
        model: "m".into(),
        timeout_ms: 10_000,
        system: "s".into(),
        user: "u".into(),
    };
    let cancel = std::sync::atomic::AtomicBool::new(true);
    let error = fleqi_application::ports::ModelGateway::complete(&gateway, &request, &cancel)
        .expect_err("预置取消应失败");
    assert!(
        matches!(
            error,
            fleqi_application::ports::ModelGatewayError::Cancelled
        ),
        "{error:?}"
    );
}
