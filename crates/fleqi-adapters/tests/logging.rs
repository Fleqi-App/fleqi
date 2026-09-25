//! 脱敏日志测试：白名单键与可识别秘密模式。样本字符串在运行时拼接，源码中不出现完整密钥形状。

use fleqi_adapters::logging::{SafeLogger, redact};

fn sample(prefix: &str, body: &str) -> String {
    format!("{prefix}{body}")
}

#[test]
fn redacts_known_secret_shapes() {
    let openai_like = sample("sk-", &"a".repeat(16));
    assert_eq!(
        redact(&format!("key {openai_like} rest")),
        "key [redacted] rest"
    );
    let bearer = sample("Bearer ", &format!("{}.{}", "x".repeat(20), "y".repeat(8)));
    assert_eq!(
        redact(&format!("Authorization: {bearer}")),
        "Authorization=[redacted]"
    );
    assert_eq!(redact("api_key=12345678 done"), "api_key=[redacted] done");
    assert_eq!(redact("password: hunter2!"), "password=[redacted]");
    let aws_like = sample("AKIA", &"Q".repeat(16));
    assert_eq!(redact(&aws_like), "[redacted]");
    let github_like = sample("ghp_", &"z".repeat(30));
    assert_eq!(redact(&github_like), "[redacted]");
    assert_eq!(
        redact("普通目录切换 /Users/me/Docs"),
        "普通目录切换 /Users/me/Docs"
    );
}

#[test]
fn logger_writes_only_allowlisted_keys_with_redaction() {
    let dir = tempfile::tempdir().unwrap();
    let logger = SafeLogger::open(dir.path()).unwrap();
    let secret = sample("sk-", &"s".repeat(12));
    let payload = format!("{{\"apiKey\":\"{secret}\"}}");
    let line = logger.log(
        "info",
        "settings.update",
        &[
            ("requestId", "r1"),
            ("payload", &payload),
            ("terminalInput", "ls\n"),
            ("message", "token=abcd1234"),
        ],
    );
    let json: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(json["requestId"], "r1");
    assert!(json.get("payload").is_none(), "完整请求不记录");
    assert!(json.get("terminalInput").is_none(), "终端原始输入不记录");
    assert_eq!(json["message"], "token=[redacted]");
    let content = std::fs::read_to_string(dir.path().join("fleqi.log")).unwrap();
    assert!(content.contains("settings.update"));
    assert!(!content.contains(&secret));
}
