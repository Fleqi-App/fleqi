//! requestId 幂等判定（architecture.md §8、§12.2）：
//! 同 requestId 同载荷复用结果；不同载荷报 conflict；并发重复由应用层合并。

use serde::{Deserialize, Serialize};

/// 已持久化的请求回执。`fingerprint` 是载荷的规范化摘要，`result_json` 是当时返回的逻辑结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub request_id: String,
    pub fingerprint: String,
    pub result_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptOutcome {
    /// 首次出现：正常执行并写回执。
    Fresh,
    /// 同 ID 同载荷：直接返回当时结果，不重复执行。
    Replay(String),
    /// 同 ID 不同载荷：拒绝，调用方应换新的 requestId。
    Conflict,
}

pub fn evaluate_receipt(
    existing: Option<&Receipt>,
    request_id: &str,
    fingerprint: &str,
) -> ReceiptOutcome {
    match existing {
        None => ReceiptOutcome::Fresh,
        Some(receipt) if receipt.request_id != request_id => ReceiptOutcome::Fresh,
        Some(receipt) if receipt.fingerprint == fingerprint => {
            ReceiptOutcome::Replay(receipt.result_json.clone())
        }
        Some(_) => ReceiptOutcome::Conflict,
    }
}
