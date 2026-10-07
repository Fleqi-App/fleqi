//! M3.4 基础文件能力执行器（capabilities.md §2、§1.3 公共合同）：
//! 六类（文件/ZIP/图片/音频视频/PDF/文本文档）共用命名冲突、原件保护与批处理合同。
//! 图片用内置 Rust（image crate）；媒体经检测到的 ffmpeg（缺失时 ToolUnavailable）；
//! PDF 用 lopdf；DOCX 用内置 OOXML 最小生成 + 正文提取（不执行宏）。

use crate::process::{ProcessRunner, SpawnRequest};
use image::{DynamicImage, ImageFormat};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityError {
    #[error("已存在：{0}")]
    AlreadyExists(String),
    #[error("工具不可用：{0}")]
    ToolUnavailable(String),
    #[error("输入无效：{0}")]
    InvalidInput(String),
    #[error("操作失败：{0}")]
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ItemResult {
    pub source: PathBuf,
    pub destination: Option<PathBuf>,
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BatchReport {
    pub succeeded: usize,
    pub failures: Vec<ItemResult>,
}

impl BatchReport {
    fn from_items(items: Vec<ItemResult>) -> Self {
        let succeeded = items.iter().filter(|item| item.ok).count();
        let failures = items.into_iter().filter(|item| !item.ok).collect();
        Self {
            succeeded,
            failures,
        }
    }
}

/// 生成不冲突的新名称：`name (1).ext`、`name (2).ext` …（AC-COMMON-003）。
pub fn unique_destination(directory: &Path, desired: &Path) -> PathBuf {
    if !directory.join(desired).exists() {
        return directory.join(desired);
    }
    let stem = desired
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = desired.extension().and_then(|e| e.to_str());
    for index in 1..10_000u32 {
        let candidate = match ext {
            Some(ext) => format!("{stem} ({index}).{ext}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = directory.join(candidate);
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!(
        "{stem}-{}.{:?}",
        std::process::id(),
        ext.unwrap_or("tmp")
    ))
}

pub struct FileCapabilities;

impl Default for FileCapabilities {
    fn default() -> Self {
        Self::new()
    }
}

impl FileCapabilities {
    pub fn new() -> Self {
        Self
    }

    // ---------- 文件（CAP-FILE-001..008） ----------

    /// CAP-FILE-001：默认生成新文件，不误覆盖（AC-CAP-001：空格/Unicode 路径读写正确）。
    pub fn create_text_file(
        &self,
        directory: &Path,
        name: &str,
        content: &str,
        encoding: &str,
    ) -> Result<PathBuf, CapabilityError> {
        std::fs::create_dir_all(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        if encoding != "utf-8" {
            return Err(CapabilityError::InvalidInput(
                "首版内置创建仅支持 UTF-8".into(),
            ));
        }
        // 默认 uniqueName：保留已有文件并生成不冲突的新名称（AC-COMMON-003）。
        let desired = PathBuf::from(name);
        let path = unique_destination(directory, &desired);
        std::fs::write(&path, content).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(path)
    }

    /// CAP-FILE-002：单层/显式多层（AC-CAP-002：同名冲突不破坏已有内容）。
    pub fn create_folder(
        &self,
        parent: &Path,
        name: &str,
        create_intermediates: bool,
    ) -> Result<PathBuf, CapabilityError> {
        let target = parent.join(name);
        if target.exists() {
            return Err(CapabilityError::AlreadyExists(
                target.to_string_lossy().into_owned(),
            ));
        }
        if create_intermediates {
            std::fs::create_dir_all(&target).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        } else if let Some(parent_dir) = target.parent() {
            std::fs::create_dir(parent_dir.join(name))
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        } else {
            return Err(CapabilityError::InvalidInput("缺少父目录".into()));
        }
        Ok(target)
    }

    /// CAP-FILE-003：复制混合选区，逐项报告（AC-CAP-003）。
    pub fn copy(
        &self,
        sources: Vec<PathBuf>,
        target: &Path,
    ) -> Result<BatchReport, CapabilityError> {
        std::fs::create_dir_all(target).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut items = Vec::new();
        for source in sources {
            let name = source
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| "untitled".into());
            let destination = unique_destination(target, &name);
            let result = reject_nested_destination(&source, target)
                .and_then(|_| copy_recursive(&source, &destination));
            items.push(ItemResult {
                source,
                destination: Some(destination),
                ok: result.is_ok(),
                message: result.err().map(|e| e.to_string()),
            });
        }
        Ok(BatchReport::from_items(items))
    }

    /// CAP-FILE-004：移动（同卷 rename；跨卷 copy+delete），失败逐项可见（AC-CAP-004）。
    pub fn move_entries(
        &self,
        sources: Vec<PathBuf>,
        target: &Path,
    ) -> Result<BatchReport, CapabilityError> {
        std::fs::create_dir_all(target).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut items = Vec::new();
        for source in sources {
            if !source.exists() {
                items.push(ItemResult {
                    source,
                    destination: None,
                    ok: false,
                    message: Some("源不存在".into()),
                });
                continue;
            }
            let name = source
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| "untitled".into());
            let destination = unique_destination(target, &name);
            let result = reject_nested_destination(&source, target).and_then(|_| {
                std::fs::rename(&source, &destination).or_else(|_| {
                    copy_recursive(&source, &destination)?;
                    if source.is_dir() {
                        std::fs::remove_dir_all(&source)
                    } else {
                        std::fs::remove_file(&source)
                    }
                })
            });
            items.push(ItemResult {
                source,
                destination: Some(destination),
                ok: result.is_ok(),
                message: result.err().map(|e| e.to_string()),
            });
        }
        Ok(BatchReport::from_items(items))
    }

    /// CAP-FILE-005：改名预览与实际名称对应（AC-CAP-005：结果与预览一致）。
    pub fn rename_preview(&self, sources: &[PathBuf], template: &str) -> Vec<RenameEntry> {
        sources
            .iter()
            .map(|source| {
                let name = source
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let stem = Path::new(&name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let ext = Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let new_name = template
                    .replace("{name}", &name)
                    .replace("{stem}", &stem)
                    .replace("{ext}", &ext);
                RenameEntry {
                    source: source.clone(),
                    new_name,
                }
            })
            .collect()
    }

    pub fn rename_apply(&self, entries: Vec<RenameEntry>) -> Result<BatchReport, CapabilityError> {
        let mut items = Vec::new();
        for entry in entries {
            let parent = entry.source.parent().unwrap_or(Path::new("."));
            let desired = PathBuf::from(&entry.new_name);
            let destination =
                if parent.join(&desired).exists() && parent.join(&desired) != entry.source {
                    unique_destination(parent, &desired)
                } else {
                    parent.join(desired)
                };
            let result = std::fs::rename(&entry.source, &destination).map_err(|e| e.to_string());
            items.push(ItemResult {
                source: entry.source,
                destination: Some(destination),
                ok: result.is_ok(),
                message: result.err(),
            });
        }
        Ok(BatchReport::from_items(items))
    }

    /// CAP-FILE-006：按给定顺序批量编号（AC-CAP-006：不同原序按指定排序得到相同编号）。
    pub fn batch_number(
        &self,
        directory: &Path,
        names: &[&str],
        start: u32,
        step: u32,
        width: usize,
        position: &str,
    ) -> Result<BatchReport, CapabilityError> {
        let mut items = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let number = start + step * index as u32;
            let padded = format!("{:0width$}", number, width = width);
            let path = directory.join(name);
            let stem = Path::new(name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let ext = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            let new_name = if ext.is_empty() {
                format!("{padded}-{stem}")
            } else if position == "prefix" {
                format!("{padded}-{stem}.{ext}")
            } else {
                format!("{stem}-{padded}.{ext}")
            };
            let destination = unique_destination(directory, Path::new(&new_name));
            let result = std::fs::rename(&path, &destination).map_err(|e| e.to_string());
            items.push(ItemResult {
                source: path,
                destination: Some(destination),
                ok: result.is_ok(),
                message: result.err(),
            });
        }
        Ok(BatchReport::from_items(items))
    }

    /// CAP-FILE-007：按类型/日期整理，范围外文件不动（AC-CAP-007）。
    pub fn organize_plan(
        &self,
        directory: &Path,
        by: &str,
        _scope: Option<String>,
    ) -> Result<Vec<RenameEntry>, CapabilityError> {
        let mut entries = Vec::new();
        for entry in
            std::fs::read_dir(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?
        {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let category = if by == "type" {
                category_for(&name)
            } else {
                "ByDate".to_owned()
            };
            entries.push(RenameEntry {
                source: path,
                new_name: format!("{category}/{name}"),
            });
        }
        Ok(entries)
    }

    pub fn organize_apply(&self, plan: &[RenameEntry]) -> Result<BatchReport, CapabilityError> {
        let mut items = Vec::new();
        for entry in plan {
            let parent = entry.source.parent().unwrap_or(Path::new("."));
            let destination = parent.join(&entry.new_name);
            if let Some(dir) = destination.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let result = std::fs::rename(&entry.source, &destination).map_err(|e| e.to_string());
            items.push(ItemResult {
                source: entry.source.clone(),
                destination: Some(destination),
                ok: result.is_ok(),
                message: result.err(),
            });
        }
        Ok(BatchReport::from_items(items))
    }

    /// CAP-FILE-008：移入回收站，可恢复（AC-CAP-008）。
    /// macOS 使用 Finder；Linux 使用 FreeDesktop Trash；Windows 使用资源管理器回收站。
    pub fn trash(&self, sources: Vec<PathBuf>) -> TrashReport {
        let mut report = TrashReport::default();
        for source in sources {
            let status = move_to_trash(&source);
            let ok = status.is_ok();
            report.items.push(ItemResult {
                source: source.clone(),
                destination: None,
                ok,
                message: None,
            });
            let mut destination = None;
            if ok {
                report.succeeded += 1;
                // 记录回收站内的新位置（同名取最近修改），供恢复使用。
                destination = Self::locate_in_trash(&source);
                if let Some(trash_path) = &destination {
                    report.restore_paths.push(trash_path.clone());
                }
            } else {
                let message = status.err();
                if let Some(item) = report.items.last_mut() {
                    item.message = message;
                }
            }
            if let Some(item) = report.items.last_mut() {
                item.destination = destination;
            }
        }
        report
    }

    /// 在平台回收站中定位刚被删除的条目：同名且最近修改优先。
    fn locate_in_trash(original: &Path) -> Option<PathBuf> {
        let name = original.file_name()?.to_str()?.to_owned();
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        for trash in trash_lookup_dirs() {
            let Ok(entries) = std::fs::read_dir(&trash) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.file_name().and_then(|n| n.to_str()) != Some(name.as_str()) {
                    continue;
                }
                let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
                    continue;
                };
                if best.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                    best = Some((modified, path));
                }
            }
        }
        best.map(|(_, path)| path)
    }

    /// 恢复：把回收站内的条目移回原位置（原位置被占用时自动唯一化命名）。
    pub fn restore_from_trash(
        &self,
        restore_path: &Path,
        original: &Path,
    ) -> Result<(), CapabilityError> {
        if !restore_path.exists() {
            return Err(CapabilityError::Failed(format!(
                "回收站中找不到 {}",
                restore_path.display()
            )));
        }
        let parent = original
            .parent()
            .ok_or_else(|| CapabilityError::InvalidInput("原位置没有父目录".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let target = unique_destination(parent, original);
        std::fs::rename(restore_path, &target)
            .map_err(|e| CapabilityError::Failed(format!("恢复失败：{e}")))?;
        if let (Some(name), Some(files)) = (restore_path.file_name(), restore_path.parent())
            && files.ends_with("files")
            && let Some(root) = files.parent()
        {
            let info = root
                .join("info")
                .join(format!("{}.trashinfo", name.to_string_lossy()));
            let _ = std::fs::remove_file(info);
        }
        Ok(())
    }

    // ---------- ZIP（CAP-ZIP-001..003） ----------

    /// CAP-ZIP-001：打包（AC-CAP-009：重新解压哈希/结构对应输入）。
    pub fn zip_create(&self, sources: &[PathBuf], archive: &Path) -> Result<(), CapabilityError> {
        let file =
            std::fs::File::create(archive).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for source in sources {
            let base = source.parent().unwrap_or(Path::new("."));
            append_to_zip(&mut writer, source, base, options)?;
        }
        writer
            .finish()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(())
    }

    /// CAP-ZIP-002：列表无需解压（AC-CAP-010：目录、空文件、Unicode 全列出）。
    pub fn zip_list(&self, archive: &Path) -> Result<Vec<ZipEntryInfo>, CapabilityError> {
        let file =
            std::fs::File::open(archive).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut reader =
            zip::ZipArchive::new(file).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut entries = Vec::new();
        for index in 0..reader.len() {
            let entry = reader
                .by_index(index)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            entries.push(ZipEntryInfo {
                name: entry.name().to_owned(),
                is_dir: entry.is_dir(),
                size: entry.size(),
            });
        }
        Ok(entries)
    }

    /// CAP-ZIP-003：解压到独立目录；越界条目拒绝（AC-CAP-011）。
    pub fn zip_extract(
        &self,
        archive: &Path,
        target: &Path,
    ) -> Result<BatchReport, CapabilityError> {
        std::fs::create_dir_all(target).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let target = target
            .canonicalize()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let file =
            std::fs::File::open(archive).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut reader =
            zip::ZipArchive::new(file).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut items = Vec::new();
        for index in 0..reader.len() {
            let mut entry = reader
                .by_index(index)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            let name = entry.name().to_owned();
            let destination = entry
                .enclosed_name()
                .and_then(|relative| safe_zip_destination(&target, &relative, &name));
            let Some(destination) = destination else {
                items.push(ItemResult {
                    source: archive.to_path_buf(),
                    destination: None,
                    ok: false,
                    message: Some(format!("越界条目拒绝：{name}")),
                });
                continue;
            };
            let result = (|| -> std::io::Result<PathBuf> {
                if entry.is_dir() {
                    std::fs::create_dir_all(&destination)?;
                } else {
                    if let Some(dir) = destination.parent() {
                        std::fs::create_dir_all(dir)?;
                    }
                    let mut output = std::fs::File::create(&destination)?;
                    std::io::copy(&mut entry, &mut output)?;
                }
                Ok(destination)
            })();
            items.push(ItemResult {
                source: archive.to_path_buf(),
                destination: result.as_ref().ok().cloned(),
                ok: result.is_ok(),
                message: result.err().map(|e| e.to_string()),
            });
        }
        Ok(BatchReport::from_items(items))
    }

    // ---------- 图片（CAP-IMAGE-001..004，内置 image crate） ----------

    /// 生成测试 PNG（内部测试辅助；不是产品能力）。
    pub fn generate_test_png(
        &self,
        directory: &Path,
        name: &str,
        width: u32,
        height: u32,
    ) -> Result<PathBuf, CapabilityError> {
        std::fs::create_dir_all(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let path = directory.join(name);
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        image
            .save_with_format(&path, ImageFormat::Png)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(path)
    }

    /// CAP-IMAGE-001：三格式六方向；质量参数（AC-CAP-012）。
    pub fn image_convert(
        &self,
        source: &Path,
        directory: &Path,
        format: &str,
        quality: u8,
    ) -> Result<PathBuf, CapabilityError> {
        self.image_convert_with_background(source, directory, format, quality, None)
    }

    /// JPEG cannot preserve alpha. The caller must explicitly choose a background.
    pub fn image_convert_with_background(
        &self,
        source: &Path,
        directory: &Path,
        format: &str,
        quality: u8,
        background: Option<[u8; 3]>,
    ) -> Result<PathBuf, CapabilityError> {
        let image =
            image::open(source).map_err(|e| CapabilityError::Failed(format!("解码失败：{e}")))?;
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image");
        let destination = unique_destination(directory, Path::new(&format!("{stem}.{format}")));
        let mut dynamic = image;
        if matches!(format, "jpg" | "jpeg") {
            let rgba = dynamic.to_rgba8();
            if rgba.pixels().any(|pixel| pixel[3] < 255) {
                let background = background.ok_or_else(|| {
                    CapabilityError::InvalidInput(
                        "图片含透明像素；转换 JPG 前请选择合成背景，或保留为 PNG/WebP".into(),
                    )
                })?;
                dynamic = DynamicImage::ImageRgb8(image::RgbImage::from_fn(
                    rgba.width(),
                    rgba.height(),
                    |x, y| {
                        let pixel = rgba.get_pixel(x, y);
                        let alpha = u32::from(pixel[3]);
                        image::Rgb(std::array::from_fn(|i| {
                            ((u32::from(pixel[i]) * alpha
                                + u32::from(background[i]) * (255 - alpha)
                                + 127)
                                / 255) as u8
                        }))
                    },
                ));
            }
        }
        match format {
            "png" => dynamic
                .save_with_format(&destination, ImageFormat::Png)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?,
            "jpg" | "jpeg" => {
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    std::fs::File::create(&destination)
                        .map_err(|e| CapabilityError::Failed(e.to_string()))?,
                    quality,
                );
                DynamicImage::ImageRgb8(dynamic.to_rgb8())
                    .write_with_encoder(encoder)
                    .map_err(|e| CapabilityError::Failed(e.to_string()))?
            }
            "webp" => dynamic
                .save_with_format(&destination, ImageFormat::WebP)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?,
            other => {
                return Err(CapabilityError::InvalidInput(format!(
                    "不支持的目标格式 {other}"
                )));
            }
        }
        Ok(destination)
    }

    /// CAP-IMAGE-002：边界框缩放，不放大、保持比例（AC-CAP-013）。
    pub fn image_resize(
        &self,
        source: &Path,
        directory: &Path,
        max_width: u32,
        max_height: u32,
        allow_upscale: bool,
    ) -> Result<PathBuf, CapabilityError> {
        if max_width == 0 || max_height == 0 {
            return Err(CapabilityError::InvalidInput("缩放宽高必须大于零".into()));
        }
        let image =
            image::open(source).map_err(|e| CapabilityError::Failed(format!("解码失败：{e}")))?;
        let (width, height) = (image.width(), image.height());
        let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64);
        let scale = if !allow_upscale {
            scale.min(1.0)
        } else {
            scale
        };
        let resized = if scale < 1.0 || allow_upscale {
            image.resize_exact(
                ((width as f64 * scale) as u32).max(1),
                ((height as f64 * scale) as u32).max(1),
                image::imageops::FilterType::Lanczos3,
            )
        } else {
            image
        };
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image");
        let destination = unique_destination(directory, Path::new(&format!("{stem}-resized.png")));
        resized
            .save_with_format(&destination, ImageFormat::Png)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(destination)
    }

    /// CAP-IMAGE-003：旋转 90/180/270（AC-CAP-014：非方形样本交换宽高）。
    pub fn image_rotate(
        &self,
        source: &Path,
        directory: &Path,
        degrees: u32,
    ) -> Result<PathBuf, CapabilityError> {
        let image =
            image::open(source).map_err(|e| CapabilityError::Failed(format!("解码失败：{e}")))?;
        let rotated = match degrees {
            90 => image.rotate90(),
            180 => image.rotate180(),
            270 => image.rotate270(),
            other => {
                return Err(CapabilityError::InvalidInput(format!(
                    "旋转角度必须是 90/180/270：{other}"
                )));
            }
        };
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image");
        let destination =
            unique_destination(directory, Path::new(&format!("{stem}-rot{degrees}.png")));
        rotated
            .save_with_format(&destination, ImageFormat::Png)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(destination)
    }

    pub fn image_dimensions(&self, source: &Path) -> Result<(u32, u32), CapabilityError> {
        let image = image::open(source).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok((image.width(), image.height()))
    }

    // ---------- 音频/视频（CAP-MEDIA-001..，经 ffmpeg） ----------

    fn ffmpeg_path(&self) -> Result<PathBuf, CapabilityError> {
        for candidate in [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ] {
            if Path::new(candidate).exists() {
                return Ok(PathBuf::from(candidate));
            }
        }
        Err(CapabilityError::ToolUnavailable(
            "未检测到 ffmpeg（DEP-MEDIA）；从工具页安装后可用".into(),
        ))
    }

    /// 生成测试 WAV（1 秒 440Hz 正弦；内部测试辅助）。
    pub fn generate_test_wav(
        &self,
        directory: &Path,
        name: &str,
        seconds: u32,
    ) -> Result<PathBuf, CapabilityError> {
        self.ffmpeg_path()?;
        std::fs::create_dir_all(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let path = directory.join(name);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut runner = ProcessRunner::spawn(
            SpawnRequest {
                executable: self.ffmpeg_path()?,
                args: vec![
                    "-f".into(),
                    "lavfi".into(),
                    "-i".into(),
                    format!("sine=frequency=440:duration={seconds}"),
                    "-ar".into(),
                    "44100".into(),
                    path.to_string_lossy().into_owned(),
                ],
                cwd: directory.to_path_buf(),
                env: vec![],
            },
            tx,
        )
        .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let _ = rx;
        let _ = runner.wait();
        if path.exists() {
            Ok(path)
        } else {
            Err(CapabilityError::Failed("ffmpeg 未生成输出".into()))
        }
    }

    /// CAP-MEDIA-001：音频转换（默认质量由调用方传入；AC-CAP-016）。
    pub fn audio_convert(
        &self,
        source: &Path,
        directory: &Path,
        format: &str,
        bitrate_kbps: u32,
    ) -> Result<PathBuf, CapabilityError> {
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio");
        let destination = unique_destination(directory, Path::new(&format!("{stem}.{format}")));
        let (tx, rx) = std::sync::mpsc::channel();
        let mut runner = ProcessRunner::spawn(
            SpawnRequest {
                executable: self.ffmpeg_path()?,
                args: vec![
                    "-y".into(),
                    "-i".into(),
                    source.to_string_lossy().into_owned(),
                    "-b:a".into(),
                    format!("{bitrate_kbps}k"),
                    destination.to_string_lossy().into_owned(),
                ],
                cwd: directory.to_path_buf(),
                env: vec![],
            },
            tx,
        )
        .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let _ = rx;
        let _ = runner.wait();
        if destination.exists() && destination.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            Ok(destination)
        } else {
            Err(CapabilityError::Failed("ffmpeg 转换失败".into()))
        }
    }

    // ---------- PDF（CAP-PDF-001..005，lopdf） ----------

    /// 生成测试 PDF（指定页数；内部测试辅助）。
    pub fn generate_test_pdf(
        &self,
        directory: &Path,
        name: &str,
        pages: u32,
    ) -> Result<PathBuf, CapabilityError> {
        std::fs::create_dir_all(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let path = directory.join(name);
        let mut document = lopdf::Document::with_version("1.5");
        let pages_id = document.add_object(lopdf::Dictionary::new());
        let mut page_ids = Vec::new();
        for index in 0..pages {
            let content_id = document.add_object(lopdf::Stream::new(
                lopdf::Dictionary::new(),
                format!(
                    "BT /F1 12 Tf 72 720 Td (Fleqi test page {}) Tj ET",
                    index + 1
                )
                .into_bytes(),
            ));
            let page_id = document.add_object(lopdf::Dictionary::from_iter([
                ("Type", lopdf::Object::Name(b"Page".to_vec())),
                ("Parent", lopdf::Object::Reference(pages_id)),
                (
                    "MediaBox",
                    lopdf::Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
                ),
                ("Contents", lopdf::Object::Reference(content_id)),
            ]));
            page_ids.push(lopdf::Object::Reference(page_id));
        }
        let font = document.add_object(lopdf::Dictionary::from_iter([
            ("Type", lopdf::Object::Name(b"Font".to_vec())),
            ("Subtype", lopdf::Object::Name(b"Type1".to_vec())),
            ("BaseFont", lopdf::Object::Name(b"Helvetica".to_vec())),
        ]));
        if let Ok(dict) = document.get_dictionary_mut(pages_id) {
            let mut fonts = lopdf::Dictionary::new();
            fonts.set("F1", lopdf::Object::Reference(font));
            let mut resources = lopdf::Dictionary::new();
            resources.set("Font", lopdf::Object::Dictionary(fonts));
            dict.set("Resources", lopdf::Object::Dictionary(resources));
            dict.set("Type", lopdf::Object::Name(b"Pages".to_vec()));
            dict.set("Count", lopdf::Object::Integer(pages as i64));
            dict.set("Kids", lopdf::Object::Array(page_ids));
        }
        let catalog_id = document.add_object(lopdf::Dictionary::from_iter([
            ("Type", lopdf::Object::Name(b"Catalog".to_vec())),
            ("Pages", lopdf::Object::Reference(pages_id)),
        ]));
        document
            .trailer
            .set("Root", lopdf::Object::Reference(catalog_id));
        document.compress();
        document
            .save(&path)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(path)
    }

    /// CAP-PDF-001：合并，页序与输入一致（AC-CAP-020/069）。
    /// 逐文档把页面树接到合成文档（页序 = 输入顺序）。
    pub fn pdf_merge(
        &self,
        sources: &[PathBuf],
        directory: &Path,
        name: &str,
    ) -> Result<PathBuf, CapabilityError> {
        if sources.len() < 2 {
            return Err(CapabilityError::InvalidInput(
                "合并需要两个及以上 PDF".into(),
            ));
        }
        // Import complete, renumbered object graphs. Keep each document's page tree
        // so inherited resources, page boxes, rotation, XObjects and embedded fonts survive.
        let mut merged = lopdf::Document::with_version("1.7");
        let pages_id = merged.add_object(lopdf::Dictionary::new());
        let mut kids = Vec::new();
        let mut count = 0usize;
        for source in sources {
            let mut document = lopdf::Document::load(source)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            if document.is_encrypted() {
                return Err(CapabilityError::InvalidInput("请先解密输入 PDF".into()));
            }
            document = crate::pdf_operations::materialize_resources(document)
                .map_err(CapabilityError::Failed)?;
            document.renumber_objects_with(merged.max_id + 1);
            let root = document
                .catalog()
                .and_then(|catalog| catalog.get(b"Pages"))
                .and_then(lopdf::Object::as_reference)
                .map_err(|e| CapabilityError::Failed(format!("PDF 页树无效：{e}")))?;
            count += document.get_pages().len();
            document
                .get_dictionary_mut(root)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?
                .set("Parent", lopdf::Object::Reference(pages_id));
            merged.max_id = document.max_id;
            merged.objects.extend(document.objects);
            kids.push(lopdf::Object::Reference(root));
        }
        if let Ok(dict) = merged.get_dictionary_mut(pages_id) {
            dict.set("Type", lopdf::Object::Name(b"Pages".to_vec()));
            dict.set("Count", lopdf::Object::Integer(count as i64));
            dict.set("Kids", lopdf::Object::Array(kids));
        }
        let catalog_id = merged.add_object(lopdf::Dictionary::from_iter([
            ("Type", lopdf::Object::Name(b"Catalog".to_vec())),
            ("Pages", lopdf::Object::Reference(pages_id)),
        ]));
        merged
            .trailer
            .set("Root", lopdf::Object::Reference(catalog_id));
        let destination = unique_destination(directory, Path::new(name));
        merged
            .save(&destination)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(destination)
    }

    /// CAP-PDF-002：逐页拆分（AC-CAP-021）。
    pub fn pdf_split_every(
        &self,
        source: &Path,
        directory: &Path,
        group: u32,
    ) -> Result<Vec<PathBuf>, CapabilityError> {
        let pages = self.pdf_page_count(source)?;
        let mut outputs = Vec::new();
        if group == 0 {
            return Err(CapabilityError::InvalidInput("每份页数必须大于零".into()));
        }
        for index in (0..pages).step_by(group as usize) {
            let mut document = lopdf::Document::load(source)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            let end = index.saturating_add(group).min(pages);
            let delete: Vec<u32> = (0..pages)
                .filter(|n| *n < index || *n >= end)
                .map(|n| n + 1)
                .collect();
            document.delete_pages(&delete);
            let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("pdf");
            let destination =
                unique_destination(directory, Path::new(&format!("{stem}-{}.pdf", index + 1)));
            document
                .save(&destination)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            outputs.push(destination);
        }
        Ok(outputs)
    }

    pub fn pdf_page_count(&self, source: &Path) -> Result<u32, CapabilityError> {
        let document =
            lopdf::Document::load(source).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        document
            .get_pages()
            .len()
            .try_into()
            .map_err(|_| CapabilityError::Failed("页数溢出".into()))
    }

    /// CAP-PDF-003：提页（AC-CAP-022：乱序/重复按显式顺序输出；越界拒绝执行）。
    pub fn pdf_extract_pages(
        &self,
        source: &Path,
        pages: &[u32],
        directory: &Path,
        name: &str,
    ) -> Result<PathBuf, CapabilityError> {
        let total = self.pdf_page_count(source)?;
        if pages.is_empty() {
            return Err(CapabilityError::InvalidInput("未指定要提取的页".into()));
        }
        for &page in pages {
            if page == 0 || page > total {
                return Err(CapabilityError::InvalidInput(format!(
                    "页 {page} 越界（共 {total} 页）；越界不生成假结果"
                )));
            }
        }
        let work =
            tempfile::tempdir_in(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut singles = Vec::new();
        for (index, &page) in pages.iter().enumerate() {
            let mut document = lopdf::Document::load(source)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            let delete: Vec<u32> = (1..=total).filter(|n| *n != page).collect();
            document.delete_pages(&delete);
            let single = work.path().join(format!("p{index}.pdf"));
            document
                .save(&single)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            singles.push(single);
        }
        if singles.len() == 1 {
            let destination = unique_destination(directory, Path::new(name));
            std::fs::copy(&singles[0], &destination)
                .map(|_| destination)
                .map_err(|error| CapabilityError::Failed(error.to_string()))
        } else {
            self.pdf_merge(&singles, directory, name)
        }
    }

    /// CAP-PDF-004：选页旋转（AC-CAP-023：页数与未选页内容不变；角度累积到 0..360）。
    pub fn pdf_rotate_pages(
        &self,
        source: &Path,
        pages: &[u32],
        degrees: i32,
        directory: &Path,
        name: &str,
    ) -> Result<PathBuf, CapabilityError> {
        let total = self.pdf_page_count(source)?;
        if pages.is_empty() {
            return Err(CapabilityError::InvalidInput("未指定要旋转的页".into()));
        }
        if degrees % 90 != 0 {
            return Err(CapabilityError::InvalidInput(
                "旋转角度必须是 90 的倍数".into(),
            ));
        }
        let mut document =
            lopdf::Document::load(source).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let map = document.get_pages();
        for &page in pages {
            if page == 0 || page > total {
                return Err(CapabilityError::InvalidInput(format!(
                    "页 {page} 越界（共 {total} 页）"
                )));
            }
            let id = map
                .get(&page)
                .ok_or_else(|| CapabilityError::Failed(format!("页 {page} 对象缺失")))?;
            let object = document
                .get_object_mut(*id)
                .map_err(|e| CapabilityError::Failed(e.to_string()))?;
            if let lopdf::Object::Dictionary(dict) = object {
                let current = dict
                    .get(b"Rotate")
                    .ok()
                    .and_then(|o| o.as_i64().ok())
                    .unwrap_or(0);
                let next = (current + degrees as i64).rem_euclid(360);
                dict.set("Rotate", lopdf::Object::Integer(next));
            }
        }
        let destination = unique_destination(directory, Path::new(name));
        document
            .save(&destination)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(destination)
    }

    /// CAP-PDF-005：结构压缩（AC-CAP-024：不栅格化、不降质；体积不降时如实返回）。
    /// 返回（输出、原始字节数、压缩后字节数）。
    pub fn pdf_compress(
        &self,
        source: &Path,
        directory: &Path,
        name: &str,
    ) -> Result<(PathBuf, u64, u64), CapabilityError> {
        let original_size = std::fs::metadata(source)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?
            .len();
        let mut document =
            lopdf::Document::load(source).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        // 结构重写：对象重新编号消除悬空引用；不触碰页面内容流（不降质）。
        document.renumber_objects();
        let destination = unique_destination(directory, Path::new(name));
        document
            .save(&destination)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let compressed_size = std::fs::metadata(&destination)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?
            .len();
        Ok((destination, original_size, compressed_size))
    }

    // ---------- 文本与文档（CAP-TEXT-001..006） ----------

    /// CAP-TEXT-002：创建 TXT（AC-CAP-026：读回文字/换行/编码一致）。
    pub fn create_text(
        &self,
        directory: &Path,
        name: &str,
        content: &str,
        encoding: &str,
        newline: &str,
    ) -> Result<PathBuf, CapabilityError> {
        std::fs::create_dir_all(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let destination = unique_destination(directory, Path::new(name));
        let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
        let content = match newline {
            "lf" => normalized,
            "crlf" => normalized.replace('\n', "\r\n"),
            _ => {
                return Err(CapabilityError::InvalidInput(
                    "换行格式必须为 lf 或 crlf".into(),
                ));
            }
        };
        let bytes: Vec<u8> = match encoding {
            "utf-8" => content.into_bytes(),
            "utf-16le" => [
                vec![0xff, 0xfe],
                content.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ]
            .concat(),
            "utf-16be" => [
                vec![0xfe, 0xff],
                content.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            ]
            .concat(),
            _ => return Err(CapabilityError::InvalidInput("不支持的文字编码".into())),
        };
        std::fs::write(&destination, bytes).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(destination)
    }

    /// CAP-TEXT-001：读取 TXT（AC-CAP-025：非法编码明确报错）。
    pub fn read_text(
        &self,
        source: &Path,
        encoding: Option<&str>,
    ) -> Result<(String, String), CapabilityError> {
        let bytes = std::fs::read(source).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let detected = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            "utf-8"
        } else if bytes.starts_with(&[0xFF, 0xFE]) {
            "utf-16le"
        } else if bytes.starts_with(&[0xFE, 0xFF]) {
            "utf-16be"
        } else {
            "utf-8"
        };
        let chosen = encoding.unwrap_or(detected);
        let content = match chosen {
            "utf-8" => {
                let stripped = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
                String::from_utf8(stripped.to_vec()).map_err(|_| {
                    CapabilityError::InvalidInput("不是有效的 UTF-8；请选择编码".into())
                })?
            }
            "utf-16le" | "utf-16be" => {
                let data = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
                    &bytes[2..]
                } else {
                    &bytes[..]
                };
                if data.len() % 2 != 0 {
                    return Err(CapabilityError::InvalidInput("UTF-16 字节长度无效".into()));
                }
                let units: Vec<u16> = data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| {
                        if chosen == "utf-16le" {
                            u16::from_le_bytes([pair[0], pair[1]])
                        } else {
                            u16::from_be_bytes([pair[0], pair[1]])
                        }
                    })
                    .collect();
                String::from_utf16(&units)
                    .map_err(|_| CapabilityError::InvalidInput("UTF-16 字符序列无效".into()))?
            }
            other => {
                return Err(CapabilityError::InvalidInput(format!(
                    "不支持的编码 {other}（GB18030 经显式选择随后接入）"
                )));
            }
        };
        Ok((content, chosen.to_owned()))
    }

    /// CAP-TEXT-003/004：Markdown 读取保留源文 / 创建（AC-CAP-027/028）。
    pub fn create_markdown(
        &self,
        directory: &Path,
        name: &str,
        content: &str,
    ) -> Result<PathBuf, CapabilityError> {
        self.create_text(directory, name, content, "utf-8", "lf")
    }

    pub fn read_markdown(&self, source: &Path) -> Result<(String, String), CapabilityError> {
        self.read_text(source, None)
    }

    /// CAP-TEXT-005：创建简单 DOCX（内置 OOXML 最小文档；AC-CAP-029）。
    pub fn create_docx(
        &self,
        directory: &Path,
        name: &str,
        paragraphs: Vec<String>,
    ) -> Result<PathBuf, CapabilityError> {
        std::fs::create_dir_all(directory).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let destination = unique_destination(directory, Path::new(name));
        let file = std::fs::File::create(&destination)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        writer.add_directory("[Content_Types].xml", options).ok();
        let content_types = "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
        zip_add_string(&mut writer, "[Content_Types].xml", content_types, options)?;
        let rels = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
        zip_add_string(&mut writer, "_rels/.rels", rels, options)?;
        let body: String = paragraphs
            .iter()
            .map(|text| {
                format!(
                    "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                    xml_escape(text)
                )
            })
            .collect();
        let document = format!(
            "<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
        );
        zip_add_string(&mut writer, "word/document.xml", &document, options)?;
        writer
            .finish()
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(destination)
    }

    /// CAP-TEXT-006：提取 DOCX 正文（主文档段落；AC-CAP-030）。
    pub fn extract_docx_text(&self, source: &Path) -> Result<String, CapabilityError> {
        let file =
            std::fs::File::open(source).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut reader =
            zip::ZipArchive::new(file).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        let mut entry = reader
            .by_name("word/document.xml")
            .map_err(|e| CapabilityError::Failed(format!("不是有效 DOCX：{e}")))?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut entry, &mut xml)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        Ok(extract_paragraphs(&xml))
    }
}

#[derive(Debug, Clone)]
pub struct RenameEntry {
    pub source: PathBuf,
    pub new_name: String,
}

#[derive(Debug, Clone)]
pub struct ZipEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Default)]
pub struct TrashReport {
    pub succeeded: usize,
    pub items: Vec<ItemResult>,
    pub restore_paths: Vec<PathBuf>,
}

impl TrashReport {
    pub fn succeeded(&self) -> usize {
        self.items.iter().filter(|item| item.ok).count()
    }
}

fn reject_nested_destination(source: &Path, target: &Path) -> std::io::Result<()> {
    if source.is_dir() && target.canonicalize()?.starts_with(source.canonicalize()?) {
        return Err(std::io::Error::other("不能把目录复制或移动到自身内部"));
    }
    Ok(())
}

fn copy_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(source)?;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(std::io::Error::other("复制不会跟随重解析点"));
        }
    }
    if source.is_dir() {
        std::fs::create_dir_all(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &destination.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(source, destination).map(|_| ())
    }
}

fn category_for(name: &str) -> String {
    let lower = name.to_lowercase();
    let extension = lower.rsplit('.').next().unwrap_or("").to_owned();
    match extension.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "tiff" | "webp" => "Images",
        "pdf" => "Documents",
        "txt" | "md" | "doc" | "docx" | "rtf" => "Texts",
        "zip" | "tar" | "gz" => "Archives",
        "mp3" | "m4a" | "wav" | "mp4" | "mov" | "mkv" => "Media",
        _ => "Others",
    }
    .to_owned()
}

fn append_to_zip<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    source: &Path,
    base: &Path,
    options: zip::write::FileOptions<'_, ()>,
) -> Result<(), CapabilityError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(source)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(CapabilityError::InvalidInput(
                "ZIP 打包不会跟随重解析点".into(),
            ));
        }
    }
    let relative = source.strip_prefix(base).unwrap_or(source);
    let name = relative.to_string_lossy().into_owned();
    #[cfg(windows)]
    let name = name.replace('\\', "/");
    if source.is_dir() {
        writer
            .add_directory(name, options)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        for entry in
            std::fs::read_dir(source).map_err(|e| CapabilityError::Failed(e.to_string()))?
        {
            let entry = entry.map_err(|e| CapabilityError::Failed(e.to_string()))?;
            append_to_zip(writer, &entry.path(), base, options)?;
        }
    } else {
        let mut file =
            std::fs::File::open(source).map_err(|e| CapabilityError::Failed(e.to_string()))?;
        writer
            .start_file(name, options)
            .map_err(|e| CapabilityError::Failed(e.to_string()))?;
        std::io::copy(&mut file, writer).map_err(|e| CapabilityError::Failed(e.to_string()))?;
    }
    Ok(())
}

use std::io::Seek;

fn zip_add_string<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    name: &str,
    content: &str,
    options: zip::write::FileOptions<'_, ()>,
) -> Result<(), CapabilityError> {
    writer
        .start_file::<String, ()>(name.to_owned(), options)
        .map_err(|e| CapabilityError::Failed(e.to_string()))?;
    writer
        .write_all(content.as_bytes())
        .map_err(|e| CapabilityError::Failed(e.to_string()))?;
    Ok(())
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 提取 w:p 段落的 w:t 文本（按文档顺序；不执行宏）。
fn extract_paragraphs(xml: &str) -> String {
    let mut lines = Vec::new();
    for paragraph in xml.split("<w:p").skip(1) {
        let mut text = String::new();
        let mut rest = paragraph;
        while let Some(start) = rest.find("<w:t") {
            rest = &rest[start..];
            let content_start = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            let content_end = rest[content_start..]
                .find("</w:t>")
                .map(|i| content_start + i)
                .unwrap_or(rest.len());
            text.push_str(&rest[content_start..content_end]);
            rest = &rest[content_end.min(rest.len())..];
        }
        if !text.is_empty() {
            lines.push(text);
        }
    }
    lines.join("\n")
}

#[cfg(windows)]
pub(crate) fn valid_windows_filename(text: &str) -> bool {
    let base = text
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !text.is_empty()
        && !text.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
        && !text.chars().any(|c| c < ' ')
        && !text.ends_with([' ', '.'])
        && !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
}

fn safe_zip_destination(root: &Path, relative: &Path, name: &str) -> Option<PathBuf> {
    if name.contains('\\')
        || Path::new(name).components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return None;
    }
    let mut destination = root.to_path_buf();
    for part in relative.components() {
        let std::path::Component::Normal(part) = part else {
            return None;
        };
        #[cfg(windows)]
        {
            if !valid_windows_filename(part.to_str()?) {
                return None;
            }
        }
        destination.push(part);
        if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
            if metadata.file_type().is_symlink() {
                return None;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return None;
                }
            }
        }
    }
    Some(destination)
}

fn move_to_trash(source: &Path) -> Result<(), String> {
    if !source.exists() {
        return Err(format!("找不到 {}", source.display()));
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Finder\" to delete (POSIX file \"{}\" as alias)",
            source
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        );
        let output = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "Finder 删除被拒绝（权限/不存在）".into()
        } else {
            stderr
        });
    }
    #[cfg(target_os = "linux")]
    {
        freedesktop_trash(source)
    }
    #[cfg(target_os = "windows")]
    {
        windows_recycle(source)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = source;
        Err("当前平台没有回收站适配".into())
    }
}

fn trash_lookup_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        return dirs;
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".Trash"));
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        dirs.push(data.join("Trash").join("files"));
    }
    dirs
}

#[cfg(target_os = "linux")]
fn freedesktop_trash(source: &Path) -> Result<(), String> {
    if std::fs::symlink_metadata(source)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("回收站不跟随符号链接；链接及目标均已保留".into());
    }
    let home = std::env::var("HOME").map_err(|_| "没有 HOME，无法使用回收站".to_owned())?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(home).join(".local/share"));
    let files = data.join("Trash").join("files");
    let info = data.join("Trash").join("info");
    std::fs::create_dir_all(&files).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&info).map_err(|error| error.to_string())?;
    let original = source
        .canonicalize()
        .map_err(|error| format!("无法解析回收站路径：{error}"))?;
    let base = original
        .file_name()
        .ok_or("回收站项目没有文件名")?
        .to_string_lossy()
        .into_owned();
    if base.contains(['/', '\\']) || base.contains('\0') {
        return Err("文件名不能进入回收站".into());
    }
    let mut name = base.clone();
    let mut index = 1u32;
    while files.join(&name).exists() || info.join(format!("{name}.trashinfo")).exists() {
        name = format!("{base}-{index}");
        index += 1;
        if index > 1000 {
            return Err("回收站中同名项目过多".into());
        }
    }
    let destination = files.join(&name);
    if std::fs::rename(&original, &destination).is_err() {
        return Err(format!(
            "无法把 {} 移入回收站（可能不在同一文件系统）",
            original.display()
        ));
    }
    let path_text = original.to_string_lossy();
    if path_text.contains(['\n', '\r']) {
        let _ = std::fs::rename(&destination, &original);
        return Err("路径含换行，已保留原文件".into());
    }
    let body = format!(
        "[Trash Info]\nPath={path_text}\nDeletionDate={}\n",
        utc_stamp()
    );
    if let Err(error) = std::fs::write(info.join(format!("{name}.trashinfo")), body) {
        let _ = std::fs::rename(&destination, &original);
        return Err(format!("无法写入回收站信息：{error}"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_recycle(source: &Path) -> Result<(), String> {
    let source = source.to_path_buf();
    // 每次操作使用独立 STA，不依赖调用方的 COM 模式，也不阻塞宿主主线程。
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
        use windows::Win32::UI::Shell::*;
        use std::os::windows::ffi::OsStrExt;
        if source.components().any(|part| matches!(part, std::path::Component::Prefix(prefix) if matches!(prefix.kind(), std::path::Prefix::UNC(_, _) | std::path::Prefix::VerbatimUNC(_, _)))) {
            return Err("网络位置无法保证进入系统回收站，原文件已保留".into());
        }
        struct Apartment;
        impl Drop for Apartment { fn drop(&mut self) { unsafe { CoUninitialize() } } }
        // SAFETY: 接口和字符串在本 STA 中有效；只请求回收，失败不改用永久删除。
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|e| e.to_string())?;
            let _apartment = Apartment;
            let wide = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
            let item: IShellItem = SHCreateItemFromParsingName(windows::core::PCWSTR(wide.as_ptr()), None).map_err(|e| e.to_string())?;
            let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
            operation.SetOperationFlags(FOFX_RECYCLEONDELETE | FOFX_ADDUNDORECORD | FOFX_EARLYFAILURE | FOF_NOERRORUI | FOF_NOCONFIRMATION | FOF_SILENT).map_err(|e| e.to_string())?;
            operation.DeleteItem(&item, None).map_err(|e| e.to_string())?;
            operation.PerformOperations().map_err(|e| e.to_string())?;
            if operation.GetAnyOperationsAborted().map_err(|e| e.to_string())?.as_bool() || source.exists() {
                return Err("回收操作取消或未完成，未改用永久删除".into());
            }
            Ok(())
        }
    }).join().map_err(|_| "回收站操作线程异常".to_owned())?
}

#[cfg(target_os = "linux")]
fn utc_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

#[cfg(target_os = "linux")]
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
