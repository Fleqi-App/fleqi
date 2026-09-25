//! 脱敏日志（architecture.md §12.3、NFR-SEC-003）：字段白名单排除完整请求、凭据、
//! 文件内容与终端原始输入；对应用管理凭据与可识别秘密模式脱敏。不承诺识别任意
//! 程序输出中的全部未知秘密。

use regex::Regex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 允许写入日志的键；其余键一律丢弃（不是脱敏，而是不记录）。
pub const ALLOWED_KEYS: &[&str] = &[
    "ts",
    "level",
    "event",
    "component",
    "code",
    "message",
    "requestId",
    "revision",
    "permission",
    "status",
    "window",
    "operationId",
    "state",
    "generation",
    "durationMs",
    "count",
    "schemaVersion",
];

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\bsk-[A-Za-z0-9_-]{8,}").expect("regex"),
            Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]{8,}").expect("regex"),
            Regex::new(
                r"(?i)\b(api[_-]?key|token|secret|password|passwd|authorization)\s*[:=]\s*\S+",
            )
            .expect("regex"),
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("regex"),
            Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{20,}\b").expect("regex"),
        ]
    })
}

/// 把可识别的秘密替换为 `[redacted]`。
pub fn redact(value: &str) -> String {
    let mut text = value.to_owned();
    for pattern in patterns() {
        text = pattern
            .replace_all(&text, |caps: &regex::Captures| {
                if let Some(key) = caps.get(1) {
                    format!("{}=[redacted]", key.as_str())
                } else {
                    "[redacted]".to_owned()
                }
            })
            .into_owned();
    }
    text
}

pub struct SafeLogger {
    file: Mutex<File>,
    dir: PathBuf,
}

impl SafeLogger {
    pub fn open(log_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("fleqi.log"))?;
        Ok(Self {
            file: Mutex::new(file),
            dir: log_dir.to_path_buf(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 只写白名单键，值经脱敏；返回实际写入的行（便于测试）。
    pub fn log(&self, level: &str, event: &str, fields: &[(&str, &str)]) -> String {
        let mut map = serde_json::Map::new();
        map.insert("ts".into(), serde_json::Value::String(now()));
        map.insert("level".into(), serde_json::Value::String(level.into()));
        map.insert("event".into(), serde_json::Value::String(redact(event)));
        for (key, value) in fields {
            if ALLOWED_KEYS.contains(key) {
                map.insert((*key).into(), serde_json::Value::String(redact(value)));
            }
        }
        let line = serde_json::Value::Object(map).to_string();
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{line}");
        }
        line
    }
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}
