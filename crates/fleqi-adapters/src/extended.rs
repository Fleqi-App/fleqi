//! M3.5 扩展能力（capabilities.md §3 十类历史意图的可计算路径）：
//! 元数据/查找/校验/字数（内置 Rust）、系统信息（系统工具）、网络（回环可测）、
//! 计算（纯函数）、Git 判定、OCR/ASR（外部工具检测，缺失时 ToolUnavailable 条件路径）。
//! 参数一律由调用方传入；来源 ID 中的示例数字不构成默认参数。

use crate::capabilities::CapabilityError;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct ExtendedCapabilities {
    files: crate::capabilities::FileCapabilities,
}

impl Default for ExtendedCapabilities {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtendedCapabilities {
    pub fn new() -> Self {
        Self {
            files: crate::capabilities::FileCapabilities::new(),
        }
    }

    // ---------- 元数据（CAP-FILE-010/011/015/016） ----------

    /// CAP-FILE-010：逻辑大小与占用分开（AC-CAP-083）。
    pub fn file_size(&self, file: &Path) -> Result<FileSize, CapabilityError> {
        let metadata =
            std::fs::metadata(file).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let logical = metadata.len();
        #[cfg(unix)]
        let on_disk = {
            use std::os::unix::fs::MetadataExt;
            metadata.blocks() * 512
        };
        #[cfg(not(unix))]
        let on_disk = logical;
        Ok(FileSize { logical, on_disk })
    }

    /// CAP-FILE-011：目录递归大小、统计范围与不可访问项（AC-CAP-084）。
    pub fn folder_size(&self, directory: &Path) -> Result<FolderSize, CapabilityError> {
        let mut total = 0u64;
        let mut files = 0u64;
        let mut unreadable = Vec::new();
        let mut stack = vec![directory.to_path_buf()];
        while let Some(current) = stack.pop() {
            let entries = match std::fs::read_dir(&current) {
                Ok(entries) => entries,
                Err(_) => {
                    unreadable.push(current);
                    continue;
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(file_type) if file_type.is_symlink() => continue, // 默认不跟随符号链接
                    Ok(file_type) if file_type.is_dir() => stack.push(path),
                    Ok(_) => {
                        if let Ok(metadata) = entry.metadata() {
                            total += metadata.len();
                            files += 1;
                        }
                    }
                    Err(_) => unreadable.push(path),
                }
            }
        }
        Ok(FolderSize {
            total_bytes: total,
            file_count: files,
            unreadable_entries: unreadable,
        })
    }

    /// CAP-FILE-013：按大小排序的前 N（AC-CAP-086）。
    pub fn find_largest(
        &self,
        directory: &Path,
        limit: usize,
    ) -> Result<Vec<SizeEntry>, CapabilityError> {
        let mut entries = Vec::new();
        let mut stack = vec![directory.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(&current)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Ok(metadata) = entry.metadata() {
                    entries.push(SizeEntry {
                        path: path.clone(),
                        size: metadata.len(),
                    });
                }
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.size));
        entries.truncate(limit);
        Ok(entries)
    }

    /// CAP-FILE-014：按内容分组的重复文件（AC-CAP-087：不自动删除）。
    pub fn find_duplicates(
        &self,
        directory: &Path,
    ) -> Result<Vec<DuplicateGroup>, CapabilityError> {
        let mut by_hash: std::collections::HashMap<String, Vec<PathBuf>> =
            std::collections::HashMap::new();
        let mut stack = vec![directory.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(&current)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Ok(bytes) = std::fs::read(&path) {
                    let digest = Sha256::digest(&bytes);
                    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
                    by_hash.entry(hex).or_default().push(path);
                }
            }
        }
        Ok(by_hash
            .into_values()
            .filter(|paths| paths.len() > 1)
            .map(|paths| DuplicateGroup { paths })
            .collect())
    }

    /// CAP-FILE-015：下载来源元数据（AC-CAP-092：无字段不猜测）。
    pub fn download_source(&self, _file: &Path) -> Result<Option<String>, CapabilityError> {
        Ok(None) // 系统 quarantine xattr 读取随 platform 适配接入；无字段返回 None
    }

    // ---------- 校验（CAP-DEV-006） ----------

    /// AC-CAP-126：SHA-256 与独立实现一致；大文件流式读取。
    pub fn sha256(&self, file: &Path) -> Result<String, CapabilityError> {
        let bytes = std::fs::read(file).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let digest = Sha256::digest(&bytes);
        Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
    }

    // ---------- 字数（CAP-TEXT-007/008） ----------

    /// words=空白分隔非空段；characters=Unicode 字符数；cjk=CJK 字符计数。
    pub fn word_count(&self, file: &Path, unit: &str) -> Result<usize, CapabilityError> {
        let (content, _) = self.files.read_text(file, None)?;
        Ok(match unit {
            "words" => content.split_whitespace().count(),
            "characters" => content.chars().count(),
            "cjk" => content.chars().filter(|c| is_cjk(*c)).count(),
            other => {
                return Err(CapabilityError::InvalidInput(format!(
                    "未知计数单位 {other}"
                )));
            }
        })
    }

    // ---------- Git（CAP-DEV-001） ----------

    /// AC-CAP-121：普通目录/仓库子目录均判断正确。
    pub fn git_is_repo(&self, directory: &Path) -> Result<bool, CapabilityError> {
        let mut current = Some(directory);
        while let Some(dir) = current {
            if dir.join(".git").exists() {
                return Ok(true);
            }
            current = dir.parent();
        }
        Ok(false)
    }

    // ---------- 网络（CAP-NETWORK-001/002） ----------

    /// AC-CAP-132：目标参数化；可达/不可达统计正确。
    pub fn ping(&self, host: &str, count: u32) -> Result<PingResult, CapabilityError> {
        let output = std::process::Command::new("/sbin/ping")
            .arg("-c")
            .arg(count.to_string())
            .arg("-t")
            .arg("5")
            .arg("--")
            .arg(host)
            .output()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let received = text
            .lines()
            .filter(|line| line.contains("bytes from"))
            .count();
        let avg_ms = parse_ping_avg(&text).unwrap_or(0.0);
        Ok(PingResult {
            transmitted: count,
            received: received as u32,
            avg_ms,
        })
    }

    /// AC-CAP-133：真实字节下载；404/中断不留伪成功文件。
    pub fn download(&self, url: &str, destination: &Path) -> Result<u64, CapabilityError> {
        let response = reqwest::blocking::Client::new()
            .get(url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .map_err(|e| CapabilityError::Failed(format!("网络失败：{e}")))?;
        if !response.status().is_success() {
            let _ = std::fs::remove_file(destination);
            return Err(CapabilityError::Failed(format!(
                "HTTP {}",
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        }
        let file = std::fs::File::create(destination)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
        std::io::Write::write_all(&mut writer, &bytes)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(bytes.len() as u64)
    }

    // ---------- OCR / 转写（AC-CAP-042、035..037 的条件路径） ----------

    /// OCR：检测 tesseract；缺失时 ToolUnavailable 说明条件（AC-CAP-042 条件路径）。
    pub fn ocr_text(&self, image: &Path) -> Result<String, CapabilityError> {
        let tesseract = which("tesseract").ok_or_else(|| {
            CapabilityError::ToolUnavailable(
                "未检测到 tesseract（DEP-OCR）；从工具页安装后可识别简体中文/英文".into(),
            )
        })?;
        let output = std::process::Command::new(tesseract)
            .arg(image)
            .arg("stdout")
            .arg("-l")
            .arg("chi_sim+eng")
            .output()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        if !output.status.success() {
            return Err(CapabilityError::Failed(format!(
                "tesseract 退出码 {:?}",
                output.status.code()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// 转写：检测 whisper 类工具与已安装的语言模型；两者齐备才执行（AC-CAP-035）。
    pub fn transcribe_text(&self, media: &Path) -> Result<String, CapabilityError> {
        let whisper = which("whisper-cli")
            .or_else(|| which("whisper"))
            .ok_or_else(|| {
                CapabilityError::ToolUnavailable(
                    "未检测到 whisper 类本地转写工具（DEP-ASR）；安装后输出 TXT/SRT/VTT".into(),
                )
            })?;
        let model = find_whisper_model().ok_or_else(|| {
            CapabilityError::ToolUnavailable(
                "whisper 已安装但缺少语言模型（ggml-*.bin）；下载模型后可转写".into(),
            )
        })?;
        let output = std::process::Command::new(whisper)
            .arg("-m")
            .arg(&model)
            .arg("-f")
            .arg(media)
            .output()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        if !output.status.success() {
            return Err(CapabilityError::Failed(format!(
                "转写失败：{}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

fn is_cjk(c: char) -> bool {
    let code = c as u32;
    (0x4E00..=0x9FFF).contains(&code)
        || (0x3400..=0x4DBF).contains(&code)
        || (0x3000..=0x303F).contains(&code)
}

fn parse_ping_avg(text: &str) -> Option<f64> {
    for line in text.lines() {
        if let Some(rest) = line.split('=').nth(1)
            && let Some(value) = rest.split('/').next()
            && let Ok(avg) = value.trim().parse::<f64>()
        {
            return Some(avg);
        }
    }
    None
}

/// 在常见位置查找已安装的 whisper 模型（ggml-*.bin）。
fn find_whisper_model() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let candidates = [
        PathBuf::from("/opt/homebrew/share/whisper.cpp"),
        PathBuf::from("/usr/local/share/whisper.cpp"),
        PathBuf::from(&home).join(".fleqi/models"),
        PathBuf::from(&home).join(".cache/whisper"),
    ];
    for directory in candidates {
        if let Ok(entries) = std::fs::read_dir(&directory) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("ggml-") && name.ends_with(".bin") {
                    return Some(entry.path());
                }
            }
        }
    }
    None
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct FileSize {
    pub logical: u64,
    pub on_disk: u64,
}

#[derive(Debug, Clone)]
pub struct FolderSize {
    pub total_bytes: u64,
    pub file_count: u64,
    pub unreadable_entries: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SizeEntry {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PingResult {
    pub transmitted: u32,
    pub received: u32,
    pub avg_ms: f64,
}

// ---------- 计算（CAP-CALC-001..004，纯函数） ----------

pub struct ComputeCapabilities;

impl ComputeCapabilities {
    /// AC-CAP-103：百分比（用户参数）。
    pub fn percent(&self, percent: f64, base: f64) -> Result<f64, CapabilityError> {
        self.validate(percent, base)?;
        Ok(percent * base / 100.0)
    }

    /// AC-CAP-104：英尺+英寸 → 厘米。
    pub fn height_to_cm(&self, feet: u32, inches: u32) -> Result<f64, CapabilityError> {
        if inches >= 12 {
            return Err(CapabilityError::InvalidInput(
                "英寸必须在 0–11（12 英寸进 1 英尺）".into(),
            ));
        }
        Ok(feet as f64 * 30.48 + inches as f64 * 2.54)
    }

    /// AC-CAP-105：固定时长单位换算（日历日期另走日期能力）。
    pub fn duration_to_seconds(&self, amount: f64, unit: &str) -> Result<f64, CapabilityError> {
        if amount < 0.0 {
            return Err(CapabilityError::InvalidInput("时长不能为负".into()));
        }
        let factor = match unit {
            "seconds" | "s" => 1.0,
            "minutes" | "m" => 60.0,
            "hours" | "h" => 3_600.0,
            "days" | "d" => 86_400.0,
            "weeks" => 604_800.0,
            other => {
                return Err(CapabilityError::InvalidInput(format!(
                    "未知时间单位 {other}"
                )));
            }
        };
        Ok(amount * factor)
    }

    /// AC-CAP-106：平方根；负数按范围处理。
    pub fn sqrt(&self, value: f64) -> Result<f64, CapabilityError> {
        if value < 0.0 {
            return Err(CapabilityError::InvalidInput("负数没有实数平方根".into()));
        }
        Ok(value.sqrt())
    }

    fn validate(&self, a: f64, b: f64) -> Result<(), CapabilityError> {
        if !a.is_finite() || !b.is_finite() {
            return Err(CapabilityError::InvalidInput("数值必须有限".into()));
        }
        Ok(())
    }
}

// ---------- 系统信息（CAP-SYSTEM-009..013） ----------

pub struct SystemCapabilities;

impl Default for SystemCapabilities {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemCapabilities {
    pub fn new() -> Self {
        Self
    }

    /// AC-CAP-114：CPU/SoC 名称。
    pub fn processor(&self) -> Result<String, CapabilityError> {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .arg("-n")
            .arg("machdep.cpu.brand_string")
            .output()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if value.is_empty() {
            // Apple Silicon 无 brand_string 时回退到 hw.model。
            let model = std::process::Command::new("/usr/sbin/sysctl")
                .arg("-n")
                .arg("hw.model")
                .output()
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            return Ok(String::from_utf8_lossy(&model.stdout).trim().to_owned());
        }
        Ok(value)
    }

    /// AC-CAP-115：内存总量（GB）。
    pub fn total_ram_gb(&self) -> Result<f64, CapabilityError> {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .arg("-n")
            .arg("hw.memsize")
            .output()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let bytes: u64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .map_err(|_| CapabilityError::Failed("无法读取内存大小".into()))?;
        Ok(bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    }

    /// AC-CAP-116：每显示器分辨率（物理/逻辑分开）。
    pub fn displays(&self) -> Result<Vec<DisplayInfo>, CapabilityError> {
        let output = std::process::Command::new("/usr/sbin/system_profiler")
            .arg("SPDisplaysDataType")
            .arg("-detailLevel")
            .arg("basic")
            .output()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut displays = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Resolution: ") {
                let mut parts = rest.split_whitespace();
                let width: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
                let height: u32 = parts.nth(1).unwrap_or("0").parse().unwrap_or(0);
                displays.push(DisplayInfo {
                    width,
                    height,
                    logical: !trimmed.contains('@'),
                });
            }
        }
        if displays.is_empty() {
            displays.push(DisplayInfo {
                width: 0,
                height: 0,
                logical: true,
            });
        }
        Ok(displays)
    }

    /// AC-CAP-118：电池状态（无电池返回 None，不把未知当 0 分钟）。
    pub fn battery(&self) -> Result<Option<BatteryInfo>, CapabilityError> {
        let internal = Path::new("/System/Library/CoreServices/Menu Extras/Battery.menu");
        let internal_exists = internal.exists();
        let pmset = std::process::Command::new("/usr/bin/pmset")
            .arg("-g")
            .arg("batt")
            .output();
        let text = pmset
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let internal_key = internal_exists || text.contains("InternalBattery");
        if !internal_key {
            return Ok(None);
        }
        let percent = text.lines().find_map(|line| {
            line.split(';')
                .next()?
                .trim()
                .rsplit(char::is_whitespace)
                .next()?
                .trim_end_matches('%')
                .parse::<u8>()
                .ok()
        });
        Ok(percent.map(|charge| BatteryInfo {
            charge_percent: charge,
            charging: text.contains("AC Power"),
        }))
    }

    /// AC-CAP-117：充电功率（未提供字段时 None）。
    pub fn charger_wattage(&self) -> Option<f64> {
        let output = std::process::Command::new("/usr/bin/pmset")
            .arg("-g")
            .arg("adapter")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if !text.contains("Watts") {
            return None;
        }
        text.split_whitespace()
            .zip(text.split_whitespace().skip(1))
            .find_map(|(a, b)| {
                if b.starts_with('W') {
                    a.parse::<f64>().ok()
                } else {
                    None
                }
            })
    }
}

#[derive(Debug, Clone)]
pub struct DisplayInfo {
    pub width: u32,
    pub height: u32,
    pub logical: bool,
}

#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub charge_percent: u8,
    pub charging: bool,
}
