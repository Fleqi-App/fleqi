//! 参数化本地能力；输入 PathRef 仅在 Rust 中解析，UI 显示路径不用于还原选区。
use crate::capabilities::{BatchReport, FileCapabilities, unique_destination};
use fleqi_application::{paths::PathRegistry, run_service::NativeStepPort};
use fleqi_domain::execution::ExecutionStep;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

type DirectoryStatus = dyn Fn() -> Result<String, String> + Send + Sync;

pub struct NativeSteps {
    directory_status: std::sync::OnceLock<Arc<DirectoryStatus>>,
    pub paths: Arc<PathRegistry>,
    summary: std::sync::OnceLock<Arc<dyn fleqi_application::summary_service::SummaryPort>>,
}
impl NativeSteps {
    pub fn new(paths: Arc<PathRegistry>) -> Self {
        Self {
            paths,
            directory_status: std::sync::OnceLock::new(),
            summary: std::sync::OnceLock::new(),
        }
    }
    pub fn set_directory_status(&self, callback: Arc<DirectoryStatus>) -> Result<(), String> {
        self.directory_status
            .set(callback)
            .map_err(|_| "目录状态端口已配置".into())
    }
    pub fn set_summary(
        &self,
        summary: Arc<dyn fleqi_application::summary_service::SummaryPort>,
    ) -> Result<(), String> {
        self.summary
            .set(summary)
            .map_err(|_| "摘要端口已配置".into())
    }
}
fn name(value: &str) -> Result<&str, String> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\', '\0']) {
        Err("文件名不能为空或包含路径分隔符".into())
    } else {
        Ok(value)
    }
}
fn result(path: PathBuf) -> String {
    format!("已生成：{}\n", path.display())
}
fn batch(report: BatchReport) -> Result<String, String> {
    if !report.failures.is_empty() {
        return Err(format!(
            "成功 {} 项；失败：{}",
            report.succeeded,
            report
                .failures
                .iter()
                .map(|item| format!(
                    "{}：{}",
                    item.source.display(),
                    item.message.as_deref().unwrap_or("未知错误")
                ))
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    Ok(format!("成功处理 {} 项\n", report.succeeded))
}
pub fn page_numbers(value: &str, total: u32) -> Result<Vec<u32>, String> {
    let mut pages = vec![];
    for item in value.split(',').map(str::trim) {
        let parts: Vec<_> = item.split('-').collect();
        let first: u32 = parts
            .first()
            .unwrap_or(&"")
            .parse()
            .map_err(|_| "请输入有效页码")?;
        let last: u32 = if parts.len() == 2 {
            parts[1].parse().map_err(|_| "请输入有效页范围")?
        } else if parts.len() == 1 {
            first
        } else {
            return Err("页范围格式错误".into());
        };
        if first == 0 || first > last || last > total {
            return Err(format!("页码超出 1–{total}"));
        }
        pages.extend(first..=last);
    }
    if pages.is_empty() {
        return Err("请选择页面".into());
    }
    Ok(pages)
}
impl NativeStepPort for NativeSteps {
    fn execute(
        &self,
        step: &ExecutionStep,
        cwd: &Path,
        cancel: &AtomicBool,
    ) -> Result<fleqi_application::run_service::NativeOutput, String> {
        if fleqi_application::capability_service::writes_output_files(&step.operation) {
            let mut parameters: BTreeMap<String, String> =
                serde_json::from_str(step.args.first().ok_or("缺少参数")?)
                    .map_err(|e| e.to_string())?;
            if parameters
                .get("_nameConflict")
                .is_some_and(|value| value == "overwrite")
            {
                let sources = step
                    .input_refs
                    .iter()
                    .map(|id| {
                        self.paths
                            .resolve(id)
                            .ok_or_else(|| "输入引用已失效".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                // Directory image batches select their own per-source output paths.
                // Retain their safe naming until those destinations are individually planned.
                if step.operation == "CAP-IMAGE-002" && sources.iter().any(|path| path.is_dir()) {
                    parameters.insert("_nameConflict".into(), "uniqueName".into());
                    let mut safe = step.clone();
                    safe.args =
                        vec![serde_json::to_string(&parameters).map_err(|e| e.to_string())?];
                    return self.execute(&safe, cwd, cancel);
                }
                let cleanup = fleqi_application::capability_service::supports_conversion_cleanup(
                    &step.operation,
                ) && parameters
                    .get("sourceHandling")
                    .is_some_and(|value| value == "trashAfterSuccess");
                let guard = if cleanup {
                    conversion_source_guard(sources.first().ok_or("缺少转换输入")?, &parameters)?
                } else {
                    None
                };
                parameters.insert("_nameConflict".into(), "uniqueName".into());
                parameters.insert("_besideSource".into(), "false".into());
                if cleanup {
                    parameters.insert("sourceHandling".into(), "keep".into());
                }
                let mut staged = step.clone();
                staged.args = vec![serde_json::to_string(&parameters).map_err(|e| e.to_string())?];
                let (mut report, outputs) =
                    crate::output_transaction::generate(cwd, &sources, cancel, |directory| {
                        self.execute(&staged, directory, cancel)
                    })?;
                if cleanup && !report.partial {
                    if outputs.len() != 1 {
                        return Err("转换输出数量异常，原文件已保留".into());
                    }
                    report.output =
                        finish_conversion(&sources[0], outputs[0].clone(), guard, cancel)?;
                }
                return Ok(report);
            }
        }
        if step.operation == "CAP-IMAGE-002"
            || (step.operation.starts_with("CAP-IMAGE-")
                && step.operation.as_str() >= "CAP-IMAGE-007"
                && step.operation.as_str() <= "CAP-IMAGE-017")
        {
            let parameters = serde_json::from_str(step.args.first().ok_or("缺少参数")?)
                .map_err(|e| e.to_string())?;
            let sources = step
                .input_refs
                .iter()
                .map(|id| {
                    self.paths
                        .resolve(id)
                        .ok_or_else(|| "输入已失效".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let operation = if step.operation == "CAP-IMAGE-002" {
                "image.resize"
            } else {
                &step.operation
            };
            return crate::image_operations::execute_report(
                operation,
                &sources,
                cwd,
                &parameters,
                cancel,
            );
        }
        if step.operation == "CAP-FILE-012" {
            return self.directory_status.get().ok_or("宿主目录状态不可用")?().map(Into::into);
        }
        if fleqi_application::capability_service::extended_descriptors()
            .iter()
            .any(|spec| spec.id == step.operation)
        {
            let parameters = serde_json::from_str(step.args.first().ok_or("缺少参数")?)
                .map_err(|e| e.to_string())?;
            let sources = step
                .input_refs
                .iter()
                .map(|id| {
                    self.paths
                        .resolve(id)
                        .ok_or_else(|| "输入已失效".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            return if step.operation.starts_with("CAP-SYSTEM-")
                || step.operation.starts_with("CAP-NETWORK-")
            {
                crate::system_operations::execute(
                    &step.operation,
                    &sources,
                    cwd,
                    &parameters,
                    cancel,
                )
            } else {
                crate::file_operations::execute(&step.operation, &sources, cwd, &parameters, cancel)
            };
        }
        if matches!(
            step.operation.as_str(),
            "CAP-TEXT-001"
                | "CAP-TEXT-003"
                | "CAP-TEXT-006"
                | "CAP-TEXT-007"
                | "CAP-TEXT-008"
                | "CAP-TEXT-009"
                | "CAP-TEXT-010"
                | "CAP-TEXT-011"
                | "CAP-PDF-008"
        ) {
            return self.execute_document(step, cwd, cancel);
        }
        self.execute_output(step, cwd, cancel).map(Into::into)
    }
}

impl NativeSteps {
    fn execute_document(
        &self,
        step: &ExecutionStep,
        cwd: &Path,
        cancel: &AtomicBool,
    ) -> Result<fleqi_application::run_service::NativeOutput, String> {
        let parameters: BTreeMap<String, String> =
            serde_json::from_str(step.args.first().ok_or("缺少参数")?)
                .map_err(|e| e.to_string())?;
        let sources = step
            .input_refs
            .iter()
            .map(|id| {
                self.paths
                    .resolve(id)
                    .ok_or_else(|| "输入已失效".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut results = vec![];
        let mut partial = false;
        let mut total = 0usize;
        for source in &sources {
            if cancel.load(Ordering::Acquire) {
                return Err("已取消".into());
            }
            let content = if matches!(step.operation.as_str(), "CAP-TEXT-001" | "CAP-TEXT-003") {
                let offset = parameters
                    .get("offset")
                    .map(String::as_str)
                    .unwrap_or("0")
                    .parse()
                    .map_err(|_| "起始位置无效")?;
                let limit = parameters
                    .get("maxBytes")
                    .map(String::as_str)
                    .unwrap_or("65536")
                    .parse()
                    .map_err(|_| "读取范围无效")?;
                crate::document_operations::read_range(
                    source,
                    parameters
                        .get("encoding")
                        .map(String::as_str)
                        .unwrap_or("auto"),
                    offset,
                    limit,
                )?
            } else if step.operation == "CAP-PDF-008" {
                let password = parameters
                    .get("password")
                    .filter(|s| !s.is_empty())
                    .map(|reference| fleqi_application::secrets::resolve(reference))
                    .transpose()?
                    .unwrap_or_default();
                crate::pdf_operations::text_for_summary(
                    source,
                    &password,
                    parameters.get("pages").map(String::as_str).unwrap_or(""),
                    parameters.get("ocr").is_none_or(|mode| mode != "never"),
                    parameters
                        .get("ocrLanguage")
                        .map(String::as_str)
                        .unwrap_or("chi_sim+eng"),
                    cancel,
                )?
            } else {
                crate::document_operations::extract(source, cancel)?
            };
            partial |= content.partial;
            if matches!(step.operation.as_str(), "CAP-TEXT-007" | "CAP-TEXT-008") {
                let unit = parameters
                    .get("unit")
                    .map(String::as_str)
                    .unwrap_or("words");
                let count = crate::document_operations::word_count(&content.text, unit)?;
                total = total.checked_add(count).ok_or("计数超出范围")?;
                results.push(format!(
                    "{}：{count} {unit}；{}{}",
                    source.display(),
                    content.scope,
                    if content.partial {
                        "（范围不完整）"
                    } else {
                        ""
                    }
                ));
            } else if matches!(
                step.operation.as_str(),
                "CAP-TEXT-009" | "CAP-TEXT-010" | "CAP-PDF-008"
            ) {
                if content.text.trim().is_empty() {
                    return Err(format!("没有可供摘要的正文；{}", content.scope));
                }
                let language = parameters
                    .get("language")
                    .map(String::as_str)
                    .unwrap_or("中文");
                let limit = parameters
                    .get("length")
                    .map(String::as_str)
                    .unwrap_or("300")
                    .parse::<usize>()
                    .map_err(|_| "摘要长度无效")?;
                if limit == 0 || limit > 10000 {
                    return Err("摘要长度范围 1–10000 字".into());
                }
                let body = format!(
                    "读取范围：{}；{}\n正文：\n{}",
                    content.scope,
                    if content.partial {
                        "输入已截取，不能推断未读取内容"
                    } else {
                        "已提取正文"
                    },
                    content.text
                );
                let summary = self.summary.get().ok_or("摘要服务未配置")?.summarize(
                    &body,
                    language,
                    limit,
                    parameters.get("model").map(String::as_str),
                    cancel,
                )?;
                results.push(format!("{}\n{}", source.display(), summary));
            } else {
                if parameters.get("output").is_some_and(|mode| mode == "file") {
                    let mut name = source.file_stem().unwrap_or_default().to_os_string();
                    name.push("-extracted.txt");
                    let target = unique_destination(cwd, Path::new(&name));
                    std::fs::write(&target, content.text.as_bytes())
                        .map_err(|error| error.to_string())?;
                    results.push(format!("已生成：{}", target.display()));
                }
                results.push(format!(
                    "{}\n读取范围：{}{}\n\n{}",
                    source.display(),
                    content.scope,
                    if content.partial {
                        "（仅部分内容）"
                    } else {
                        ""
                    },
                    content.text
                ));
            }
        }
        if matches!(step.operation.as_str(), "CAP-TEXT-007" | "CAP-TEXT-008") {
            results.push(format!("已读取范围合计：{total}"));
        }
        Ok(fleqi_application::run_service::NativeOutput {
            output: results.join("\n\n"),
            partial,
        })
    }

    fn execute_output(
        &self,
        step: &ExecutionStep,
        cwd: &Path,
        cancel: &AtomicBool,
    ) -> Result<String, String> {
        if cancel.load(Ordering::Acquire) {
            return Err("已取消".into());
        }
        let values: BTreeMap<String, String> =
            serde_json::from_str(step.args.first().ok_or("缺少能力参数")?)
                .map_err(|e| e.to_string())?;
        let value = |key: &str| values.get(key).map(String::as_str).unwrap_or("");
        let number = |key: &str| -> Result<u32, String> {
            value(key)
                .parse::<u32>()
                .map_err(|_| format!("{key} 必须为非负整数"))
        };
        let sources: Vec<PathBuf> = step
            .input_refs
            .iter()
            .map(|id| {
                self.paths
                    .resolve(id)
                    .ok_or_else(|| "输入引用已失效，请重新选择".to_string())
            })
            .collect::<Result<_, _>>()?;
        if sources.iter().any(|path| !path.exists()) {
            return Err("部分输入已不存在，请重新选择".into());
        }
        let first = || {
            sources
                .first()
                .map(PathBuf::as_path)
                .ok_or_else(|| "请先选择文件".to_string())
        };
        let files = FileCapabilities::new();
        let output_name = |fallback: &str| -> Result<String, String> {
            name(if value("name").is_empty() {
                fallback
            } else {
                value("name")
            })
            .map(str::to_owned)
        };
        let error = |e: crate::capabilities::CapabilityError| e.to_string();
        match step.operation.as_str() {
            "CAP-FILE-001" | "CAP-TEXT-002" => files
                .create_text(
                    cwd,
                    name(value("name"))?,
                    value("content"),
                    value("encoding"),
                    value("newline"),
                )
                .map(result)
                .map_err(error),
            "CAP-FILE-002" => files
                .create_folder(cwd, name(value("name"))?, false)
                .map(result)
                .map_err(error),
            "CAP-FILE-003" | "CAP-FILE-004" => {
                let target = cwd.join(value("destination"));
                if value("destination").is_empty() {
                    return Err("请选择目标目录".into());
                }
                for source in &sources {
                    let origin = source.canonicalize().map_err(|e| e.to_string())?;
                    let destination = if target.exists() {
                        target.canonicalize().map_err(|e| e.to_string())?
                    } else {
                        target.clone()
                    };
                    if source.is_dir() && destination.starts_with(&origin) {
                        return Err("目标目录不能位于源目录内部".into());
                    }
                }
                if step.operation == "CAP-FILE-003" {
                    batch(files.copy(sources, &target).map_err(error)?)
                } else {
                    batch(files.move_entries(sources, &target).map_err(error)?)
                }
            }
            "CAP-FILE-005" => {
                let preview = files.rename_preview(&sources, value("template"));
                for entry in &preview {
                    name(&entry.new_name)?;
                }
                batch(files.rename_apply(preview).map_err(error)?)
            }
            "CAP-FILE-006" => {
                let width = number("width")?;
                if width > 16 {
                    return Err("序号位数不得超过 16".into());
                }
                let start = number("start")?;
                let increment = number("step")?;
                if increment == 0 {
                    return Err("序号步长必须大于零".into());
                }
                let mut output = String::new();
                for (index, source) in sources.iter().enumerate() {
                    if cancel.load(Ordering::Acquire) {
                        return Err(format!("已取消\n{output}"));
                    }
                    let current = start
                        .checked_add(increment.checked_mul(index as u32).ok_or("序号溢出")?)
                        .ok_or("序号溢出")?;
                    let filename = source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .ok_or("非 UTF-8 名称不支持模板编号")?;
                    output.push_str(&batch(
                        files
                            .batch_number(
                                source.parent().ok_or("缺少父目录")?,
                                &[filename],
                                current,
                                increment,
                                width as usize,
                                value("position"),
                            )
                            .map_err(error)?,
                    )?);
                }
                Ok(output)
            }
            "CAP-FILE-007" => {
                let mut output = String::new();
                for source in &sources {
                    if cancel.load(Ordering::Acquire) {
                        return Err(format!("已取消\n{output}"));
                    }
                    let category = if value("groupBy") == "date" {
                        let modified = source
                            .metadata()
                            .and_then(|m| m.modified())
                            .map_err(|e| e.to_string())?;
                        let date: time::OffsetDateTime = modified.into();
                        format!("{:04}-{:02}", date.year(), u8::from(date.month()))
                    } else {
                        source
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("无扩展名")
                            .to_lowercase()
                    };
                    output.push_str(&batch(
                        files
                            .move_entries(vec![source.clone()], &cwd.join(category))
                            .map_err(error)?,
                    )?);
                }
                Ok(output)
            }
            "CAP-FILE-008" => {
                let report = files.trash(sources);
                if report.items.iter().any(|item| !item.ok) {
                    return Err(format!("移入回收站失败：{:?}", report.items));
                }
                Ok(format!(
                    "已移入回收站 {} 项，可通过 Finder 恢复\n",
                    report.items.len()
                ))
            }
            "CAP-ZIP-001" => {
                let target = unique_destination(cwd, Path::new(name(value("name"))?));
                files.zip_create(&sources, &target).map_err(error)?;
                Ok(result(target))
            }
            "CAP-ZIP-002" => Ok(files
                .zip_list(first()?)
                .map_err(error)?
                .iter()
                .map(|entry| format!("{}\t{} bytes\n", entry.name, entry.size))
                .collect()),
            "CAP-ZIP-003" => {
                let target = unique_destination(cwd, Path::new(name(value("name"))?));
                batch(files.zip_extract(first()?, &target).map_err(error)?)
            }
            "CAP-IMAGE-001" | "CAP-IMAGE-004" => {
                let quality = number("quality")?;
                if quality == 0 || quality > 100 {
                    return Err("质量范围为 1–100".into());
                }
                let background = if value("alphaPolicy") == "flatten" {
                    let raw = value("background").trim_start_matches('#');
                    if raw.len() != 6 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err("背景颜色需要 #RRGGBB 格式".into());
                    }
                    Some([
                        u8::from_str_radix(&raw[0..2], 16).map_err(|error| error.to_string())?,
                        u8::from_str_radix(&raw[2..4], 16).map_err(|error| error.to_string())?,
                        u8::from_str_radix(&raw[4..6], 16).map_err(|error| error.to_string())?,
                    ])
                } else {
                    None
                };
                let source = first()?;
                let guard = conversion_source_guard(source, &values)?;
                let target = files
                    .image_convert_with_background(
                        source,
                        cwd,
                        if step.operation == "CAP-IMAGE-004" {
                            "jpg"
                        } else {
                            value("format")
                        },
                        quality as u8,
                        background,
                    )
                    .map_err(error)?;
                image::open(&target).map_err(|e| format!("新图片校验失败，原文件已保留：{e}"))?;
                finish_conversion(source, target, guard, cancel)
            }
            "CAP-IMAGE-002" => {
                crate::image_operations::execute("image.resize", &sources, cwd, &values, cancel)
            }
            "CAP-IMAGE-003" => {
                crate::image_operations::execute("image.rotate", &sources, cwd, &values, cancel)
            }
            "CAP-IMAGE-005" => {
                use image::GenericImageView;
                let display = crate::image_operations::load(first()?, cancel)?;
                let stored = image::ImageReader::open(first()?)
                    .ok()
                    .and_then(|r| r.with_guessed_format().ok())
                    .and_then(|r| r.into_dimensions().ok());
                Ok(serde_json::json!({"storedDimensions":stored,"displayDimensions":display.dimensions(),"source":"image decoder and orientation metadata"}).to_string())
            }
            "CAP-IMAGE-006" => {
                validate_ocr_languages(value("language"), cancel)?;
                let mut command =
                    std::process::Command::new(crate::image_operations::tool("tesseract")?);
                command
                    .arg(first()?)
                    .arg("stdout")
                    .args(["-l", value("language")]);
                run_command(command, cancel)
            }
            "CAP-IMAGE-007" | "CAP-IMAGE-008" | "CAP-IMAGE-009" | "CAP-IMAGE-010"
            | "CAP-IMAGE-011" | "CAP-IMAGE-012" | "CAP-IMAGE-013" | "CAP-IMAGE-014"
            | "CAP-IMAGE-015" | "CAP-IMAGE-016" | "CAP-IMAGE-017" => {
                crate::image_operations::execute(&step.operation, &sources, cwd, &values, cancel)
            }
            "CAP-MEDIA-001" | "CAP-MEDIA-002" | "CAP-MEDIA-003" | "CAP-MEDIA-004" => {
                media(&step.operation, first()?, cwd, &values, cancel)
            }
            "CAP-PDF-001" => files
                .pdf_merge(&sources, cwd, &output_name("merged.pdf")?)
                .map(result)
                .map_err(error),
            "CAP-PDF-002" => {
                let group = number("pagesPerFile")?;
                if group == 0 {
                    return Err("每份页数必须大于零".into());
                }
                files
                    .pdf_split_every(first()?, cwd, group)
                    .map(|paths| paths.into_iter().map(result).collect())
                    .map_err(error)
            }
            "CAP-PDF-003" => {
                let source = first()?;
                let pages =
                    page_numbers(value("pages"), files.pdf_page_count(source).map_err(error)?)?;
                files
                    .pdf_extract_pages(source, &pages, cwd, &output_name("extracted.pdf")?)
                    .map(result)
                    .map_err(error)
            }
            "CAP-PDF-004" => {
                let source = first()?;
                let pages =
                    page_numbers(value("pages"), files.pdf_page_count(source).map_err(error)?)?;
                files
                    .pdf_rotate_pages(
                        source,
                        &pages,
                        number("angle")? as i32,
                        cwd,
                        &output_name("rotated.pdf")?,
                    )
                    .map(result)
                    .map_err(error)
            }
            "CAP-PDF-005" => files
                .pdf_compress(first()?, cwd, &output_name("compressed.pdf")?)
                .map(|(path, before, after)| {
                    format!("{}大小：{} → {} bytes\n", result(path), before, after)
                })
                .map_err(error),
            "CAP-TEXT-001" => files
                .read_text(first()?, Some(value("encoding")))
                .map(|(text, _)| text)
                .map_err(error),
            "CAP-TEXT-003" => files
                .read_markdown(first()?)
                .map(|(text, _)| text)
                .map_err(error),
            "CAP-TEXT-004" => files
                .create_markdown(cwd, name(value("name"))?, value("content"))
                .map(result)
                .map_err(error),
            "CAP-TEXT-005" => files
                .create_docx(
                    cwd,
                    name(value("name"))?,
                    value("content").lines().map(str::to_owned).collect(),
                )
                .map(result)
                .map_err(error),
            "CAP-TEXT-006" => files.extract_docx_text(first()?).map_err(error),
            "CAP-MEDIA-005" | "CAP-MEDIA-006" | "CAP-MEDIA-007" | "CAP-MEDIA-008"
            | "CAP-MEDIA-009" | "CAP-MEDIA-010" | "CAP-MEDIA-011" | "CAP-MEDIA-012"
            | "CAP-MEDIA-013" => {
                crate::media_operations::execute(&step.operation, &sources, cwd, &values, cancel)
            }
            "CAP-PDF-006" | "CAP-PDF-007" | "CAP-PDF-009" | "CAP-PDF-010" | "CAP-PDF-011"
            | "CAP-PDF-012" => {
                crate::pdf_operations::execute(&step.operation, &sources, cwd, &values, cancel)
            }
            _ => Err("未登记的原生能力".into()),
        }
    }
}

fn media(
    operation: &str,
    source: &Path,
    cwd: &Path,
    values: &BTreeMap<String, String>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let value = |key: &str| values.get(key).map(String::as_str).unwrap_or("");
    let executable = crate::image_operations::tool("ffmpeg")?;
    let format = if operation == "CAP-MEDIA-004"
        || (operation == "CAP-MEDIA-002" && value("format").is_empty())
    {
        "mp4"
    } else {
        value("format")
    };
    if !["mp4", "mov", "mkv", "mp3", "m4a", "wav"].contains(&format) {
        return Err("目标格式无效".into());
    }
    let mut filename = source.file_stem().ok_or("输入缺少名称")?.to_os_string();
    filename.push(format!("-converted.{format}"));
    let target = unique_destination(cwd, Path::new(&filename));
    let guard = conversion_source_guard(source, values)?;
    let mut command = std::process::Command::new(executable);
    command.args(["-nostdin", "-n", "-i"]).arg(source);
    if operation == "CAP-MEDIA-004" {
        let start: f64 = value("start").parse().map_err(|_| "起始时间无效")?;
        let duration: f64 = value("duration").parse().map_err(|_| "时长无效")?;
        if !start.is_finite() || start < 0.0 || !duration.is_finite() || duration <= 0.0 {
            return Err("时间范围无效".into());
        }
        command.args(["-ss", value("start"), "-t", value("duration")]);
        if value("mode") == "copy" {
            command.args(["-c", "copy"]);
        }
    } else if operation == "CAP-MEDIA-002" {
        let quality: u8 = value("quality").parse().map_err(|_| "CRF 无效")?;
        if quality > 51 {
            return Err("CRF 范围为 0–51".into());
        }
        command.args(["-c:v", "libx264", "-crf", value("quality"), "-c:a", "aac"]);
    } else {
        command.args(["-vn", "-map", "0:a:0"]);
        if format != "wav" {
            command.args(["-b:a", &format!("{}k", value("bitrate"))]);
        }
    }
    command.arg(&target).current_dir(cwd);
    match run_command(command, cancel) {
        Ok(_) if target.metadata().is_ok_and(|metadata| metadata.len() > 0) => {
            let verified = crate::media_operations::probe(&target, cancel)?;
            if !verified["streams"]
                .as_array()
                .is_some_and(|streams| !streams.is_empty())
            {
                return Err("新文件没有可读取的媒体轨道，原文件已保留".into());
            }
            let mut message = finish_conversion(source, target, guard, cancel)?;
            if operation == "CAP-MEDIA-004" {
                message.push_str(&format!("请求起点 {} 秒，时长 {} 秒；输出实际 start_time={}，duration={} 秒（ffprobe）。\n", value("start"), value("duration"), verified["format"]["start_time"], verified["format"]["duration"]));
                if value("mode") == "copy" {
                    message.push_str("无损复制不重新编码；裁切受关键帧与轨道时间基约束，实际边界可能偏离请求。上述时间是输出时间轴，不代表精确源帧位置。\n");
                }
            }
            Ok(message)
        }
        Ok(_) => Err("转换完成但没有有效输出".into()),
        Err(error) => {
            let _ = std::fs::remove_file(&target);
            Err(error)
        }
    }
}

type ConversionSourceGuard = Option<(u64, std::time::SystemTime)>;

fn conversion_source_guard(
    source: &Path,
    values: &BTreeMap<String, String>,
) -> Result<ConversionSourceGuard, String> {
    match values
        .get("sourceHandling")
        .map(String::as_str)
        .unwrap_or("keep")
    {
        "keep" => Ok(None),
        "trashAfterSuccess" => {
            let metadata = std::fs::symlink_metadata(source).map_err(|e| e.to_string())?;
            if !metadata.is_file() {
                return Err("自动清理只支持普通文件；请先选择实际文件".into());
            }
            Ok(Some((
                metadata.len(),
                metadata.modified().map_err(|e| e.to_string())?,
            )))
        }
        _ => Err("原文件处理选项无效".into()),
    }
}

fn finish_conversion(
    source: &Path,
    target: PathBuf,
    guard: ConversionSourceGuard,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let mut message = result(target.clone());
    if let Some((length, modified)) = guard {
        if cancel.load(Ordering::Acquire) {
            return Err(format!("{message}已取消，原文件已保留"));
        }
        let unchanged = std::fs::symlink_metadata(source).is_ok_and(|meta| {
            meta.is_file() && meta.len() == length && meta.modified().ok() == Some(modified)
        });
        if !unchanged || source.canonicalize().ok() == target.canonicalize().ok() {
            return Err(format!("{message}原文件在转换过程中发生变化，已保留原文件"));
        }
        let report = FileCapabilities::new().trash(vec![source.to_path_buf()]);
        if report.items.iter().any(|item| !item.ok) {
            return Err(format!(
                "{message}新文件已生成，原文件移入回收站失败：{:?}",
                report.items
            ));
        }
        message.push_str("原文件已移入回收站，可通过 Finder 恢复。\n");
    } else {
        message.push_str("已保留原文件。\n");
    }
    Ok(message)
}

/// 同时排空两个管道并保留有界输出；取消等待子进程退出，不留下 ffmpeg 进程。
pub fn run_command(command: std::process::Command, cancel: &AtomicBool) -> Result<String, String> {
    run_command_with_input(command, None, cancel)
}

pub fn run_command_with_input(
    mut command: std::process::Command,
    input: Option<&[u8]>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    use std::io::Read;
    command
        .stdin(if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("stdout 不可用")?;
    let stderr = child.stderr.take().ok_or("stderr 不可用")?;
    let read = |mut stream: Box<dyn Read + Send>| {
        let mut output = Vec::new();
        let mut chunk = [0u8; 8192];
        while let Ok(n) = stream.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let remaining = (8 * 1024 * 1024usize).saturating_sub(output.len());
            output.extend_from_slice(&chunk[..n.min(remaining)]);
        }
        String::from_utf8_lossy(&output).into_owned()
    };
    let out = std::thread::spawn(move || read(Box::new(stdout)));
    let err = std::thread::spawn(move || read(Box::new(stderr)));
    let writer = input.map(|bytes| {
        let bytes = bytes.to_vec();
        let mut stdin = child.stdin.take().expect("piped stdin");
        std::thread::spawn(move || std::io::Write::write_all(&mut stdin, &bytes))
    });
    let started = std::time::Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire)
            || started.elapsed() > std::time::Duration::from_secs(1800)
        {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            if let Some(writer) = writer {
                let _ = writer.join();
            }
            return Err("已取消或运行超时".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    };
    let output = out.join().map_err(|_| "读取输出失败")?;
    let error = err.join().map_err(|_| "读取错误输出失败")?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    if status.success() {
        Ok(output)
    } else {
        Err(format!("退出码 {:?}：{}", status.code(), error))
    }
}

/// Tesseract may exit successfully when only some requested languages exist.
/// Check each model explicitly so a multilingual request never silently degrades.
pub fn validate_ocr_languages(language: &str, cancel: &AtomicBool) -> Result<(), String> {
    let mut command = std::process::Command::new(crate::image_operations::tool("tesseract")?);
    command.arg("--list-langs");
    let available = run_command(command, cancel)?;
    let requested: Vec<_> = language.split('+').collect();
    if requested.is_empty()
        || requested.iter().any(|lang| {
            lang.is_empty()
                || !lang
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
    {
        return Err("OCR 语言名称无效".into());
    }
    let missing: Vec<_> = requested
        .into_iter()
        .filter(|lang| !available.lines().any(|line| line.trim() == *lang))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "缺少 OCR 语言模型：{}；请安装后重试",
            missing.join("、")
        ));
    }
    Ok(())
}
