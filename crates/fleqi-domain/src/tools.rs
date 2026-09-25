//! 工具设施领域类型（architecture.md §9.2；FR-TOOLS-001..005）。
//!
//! ToolManifest 记录 ID、版本、平台/架构、可执行文件、来源、校验值、许可证、
//! 安装目录关联与所有者；应用管理的包位于应用数据目录，系统工具通过版本探测登记。
//! 下载先到 staging、验证后解压、原子发布；失败/取消保留已安装可用版本。
//! 边界：本模块不执行 IO；探测/下载/解压在适配层实现。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 工具所有者：Fleqi 受管安装或用户系统已有（architecture.md §9.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ToolOwner {
    /// Fleqi 管理的包，位于应用数据目录，可由工具页卸载。
    Fleqi,
    /// 系统已有工具（PATH/包管理器安装），仅登记不接管。
    System,
}

/// 工具来源：受管包必须给出 URL 与 SHA-256；系统工具仅探测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ToolSource {
    /// 受管下载包：staging 下载 + 完整性校验 + 安全解压 + 原子安装。
    Managed { url: String, sha256: String },
    /// 系统 PATH 探测登记，不提供卸载。
    System,
}

/// 工具清单：版本、平台映射与检测方式由受管目录确定（FR-TOOLS-005）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ToolManifest {
    pub id: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
    /// 受管包内可执行文件相对路径；系统工具为可执行名（PATH 查找）。
    pub executable: String,
    pub source: ToolSource,
    pub license: Option<String>,
    /// 依赖此工具的能力 ID（用于影响展示）。
    pub capabilities: Vec<String>,
    /// 检测参数（如 `--version`），退出码 0 视为可用。
    #[serde(default)]
    pub detection_args: Vec<String>,
}

/// 工具状态：检测中由 UI 表达；这里只有可判定的三种结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ToolStatus {
    /// 可用：探测退出码 0，携带真实版本输出与来源归属。
    Available {
        version: String,
        owner: ToolOwner,
        path: String,
    },
    /// 未安装：受管清单存在但没有可用安装。
    NotInstalled,
    /// 需处理：安装失败或检测不通过（不得伪报可用，FR-TOOLS-002/005）。
    Unavailable { reason: String },
}

/// 已安装记录（installed_tools 表；重开应用后据此恢复状态）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct InstalledTool {
    pub manifest: ToolManifest,
    pub installed_at: String,
    pub install_dir: String,
}

impl ToolStatus {
    /// 可用状态下的探测路径（无则 None）。
    pub fn available_path(&self) -> Option<String> {
        match self {
            ToolStatus::Available { path, .. } => Some(path.clone()),
            ToolStatus::NotInstalled | ToolStatus::Unavailable { .. } => None,
        }
    }
}

impl ToolManifest {
    /// 清单校验：受管包 URL 仅允许 HTTPS 或回环地址（测试/本地服务），
    /// SHA-256 必须是 64 位十六进制；系统工具不允许携带下载源。
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.id.contains('/') {
            return Err("工具 ID 不能为空且不能包含路径分隔符".into());
        }
        if self.version.trim().is_empty() {
            return Err(format!("工具 {} 缺少版本", self.id));
        }
        if self.executable.trim().is_empty() {
            return Err(format!("工具 {} 缺少可执行文件", self.id));
        }
        if self.executable.contains("..") {
            return Err(format!("工具 {} 的可执行路径不允许包含 ..", self.id));
        }
        match &self.source {
            ToolSource::Managed { url, sha256 } => {
                if !url_is_allowed(url) {
                    return Err(format!(
                        "工具 {} 的来源必须是 HTTPS（回环地址除外）：{}",
                        self.id, url
                    ));
                }
                if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(format!("工具 {} 的 SHA-256 校验值无效", self.id));
                }
            }
            ToolSource::System => {
                if self.executable.contains('/') {
                    return Err(format!(
                        "系统工具 {} 的可执行名必须是相对 PATH 的名称",
                        self.id
                    ));
                }
            }
        }
        Ok(())
    }
}

/// 仅允许 https；http 只对回环地址放行（本地测试服务），不放宽其它明文来源。
fn url_is_allowed(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    match scheme {
        "https" => true,
        "http" => {
            let host = rest
                .split(['/', ':', '?'])
                .find(|part| !part.is_empty())
                .unwrap_or_default();
            matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        }
        _ => false,
    }
}

/// 内建工具登记（FR-TOOLS-001 检测/缺失列表）：
/// Existing executables are reused; missing tools use the fixed platform package mapping.
pub fn builtin_tool_manifests() -> Vec<ToolManifest> {
    [
        ("git", "--version", vec!["Git 仓库操作"]),
        ("ffmpeg", "-version", vec!["音频/视频转换"]),
        ("ffprobe", "-version", vec!["媒体信息与结果校验"]),
        ("tesseract", "--version", vec!["OCR 文本识别"]),
        ("whisper-cli", "--help", vec!["本地语音转写"]),
        ("qpdf", "--version", vec!["PDF 处理"]),
        ("pdfimages", "-v", vec!["PDF 图片提取"]),
        ("pdftotext", "-v", vec!["PDF 正文提取"]),
        ("pdfinfo", "-v", vec!["PDF 页数与元数据"]),
        ("pdftoppm", "-v", vec!["PDF 页面渲染"]),
        ("magick", "--version", vec!["图片文字与扩展处理"]),
        ("exiftool", "-ver", vec!["媒体元数据处理"]),
    ]
    .into_iter()
    .map(|(id, argument, capabilities)| ToolManifest {
        id: id.to_owned(),
        version: "system".to_owned(),
        platform: "macos".to_owned(),
        arch: "any".to_owned(),
        executable: id.to_owned(),
        source: ToolSource::System,
        license: None,
        capabilities: capabilities.into_iter().map(str::to_owned).collect(),
        detection_args: vec![argument.to_owned()],
    })
    .collect()
}

/// Never accept an arbitrary formula or installer command from a model or IPC caller.
pub fn system_package(tool: &str) -> Option<&'static str> {
    match tool {
        "git" => Some("git"),
        "ffmpeg" | "ffprobe" => Some("ffmpeg"),
        "tesseract" => Some("tesseract"),
        "whisper-cli" => Some("whisper-cpp"),
        "qpdf" => Some("qpdf"),
        "pdfimages" | "pdftoppm" | "pdftotext" | "pdfinfo" => Some("poppler"),
        "magick" => Some("imagemagick"),
        "exiftool" => Some("exiftool"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed(url: &str, sha256: &str) -> ToolManifest {
        ToolManifest {
            id: "demo-tool".into(),
            version: "1.0".into(),
            platform: "macos".into(),
            arch: "aarch64".into(),
            executable: "bin/demo-tool".into(),
            source: ToolSource::Managed {
                url: url.into(),
                sha256: sha256.into(),
            },
            license: Some("MIT".into()),
            capabilities: vec!["cap.demo".into()],
            detection_args: vec!["--version".into()],
        }
    }

    #[test]
    fn managed_manifest_requires_https_or_loopback() {
        let https = managed("https://example.com/demo-1.0.zip", &"a".repeat(64));
        assert!(https.validate().is_ok());
        let loopback = managed("http://127.0.0.1:8080/demo.zip", &"a".repeat(64));
        assert!(loopback.validate().is_ok());
        let plain_http = managed("http://example.com/demo.zip", &"a".repeat(64));
        assert!(plain_http.validate().is_err());
        let file_scheme = managed("file:///tmp/demo.zip", &"a".repeat(64));
        assert!(file_scheme.validate().is_err());
    }

    #[test]
    fn managed_manifest_requires_hex_sha256() {
        let short = managed("https://example.com/demo.zip", &"a".repeat(63));
        assert!(short.validate().is_err());
        let non_hex = managed("https://example.com/demo.zip", &"z".repeat(64));
        assert!(non_hex.validate().is_err());
    }

    #[test]
    fn system_manifest_is_path_name_only_and_builtin_registry_valid() {
        let mut system = ToolManifest {
            id: "sys-tool".into(),
            version: "system".into(),
            platform: "macos".into(),
            arch: "any".into(),
            executable: "sys-tool/bin/x".into(),
            source: ToolSource::System,
            license: None,
            capabilities: vec![],
            detection_args: vec![],
        };
        assert!(system.validate().is_err());
        system.executable = "sys-tool".into();
        assert!(system.validate().is_ok());
        for manifest in builtin_tool_manifests() {
            assert!(
                manifest.validate().is_ok(),
                "内建清单 {:?} 不合规",
                manifest.id
            );
        }
        for name in ["pdftotext", "pdfinfo"] {
            assert!(
                builtin_tool_manifests()
                    .iter()
                    .any(|tool| tool.executable == name)
            );
            assert_eq!(system_package(name), Some("poppler"));
        }
    }

    #[test]
    fn executable_may_not_traverse() {
        let escaping = managed("https://example.com/demo.zip", &"a".repeat(64));
        let mut evil = escaping;
        evil.executable = "../../bin/sh".into();
        assert!(evil.validate().is_err());
    }
}
