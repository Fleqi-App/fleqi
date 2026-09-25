//! 操作栏输入模式解析（FR-TERM-001/002、ui-design §4.3）：
//! 首个非空白字符是半角 `!` 时进入手动终端；只删除该标记，其余原样。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ComposerMode {
    Ai,
    Terminal,
}

/// 返回模式与去标记后的文本；全角 `！` 与文本中间的 `!` 不触发。
pub fn parse_composer_input(input: &str) -> (ComposerMode, String) {
    let trimmed_start = input.trim_start_matches(|c: char| c.is_whitespace());
    if let Some(rest) = trimmed_start.strip_prefix('!') {
        return (ComposerMode::Terminal, rest.to_owned());
    }
    (ComposerMode::Ai, input.to_owned())
}

/// 手动命令是否可提交：去标记后非空白。
pub fn manual_command_is_submittable(text_without_marker: &str) -> bool {
    !text_without_marker.trim().is_empty()
}
