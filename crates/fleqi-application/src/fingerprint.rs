//! 请求载荷指纹：规范化 JSON（键排序）的 SHA-256，用于 requestId 幂等比对。

use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn fingerprint<T: Serialize>(value: &T) -> String {
    let canonical = serde_json::to_value(value)
        .and_then(|v| serde_json::to_string(&v))
        .unwrap_or_else(|_| "<unserializable>".to_owned());
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
