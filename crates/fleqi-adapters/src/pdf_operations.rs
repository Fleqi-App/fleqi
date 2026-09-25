//! PDF 内容/元数据/加密。口令只通过内存管道交给 qpdf，不放入 argv、任务或日志。
use crate::{
    capabilities::unique_destination,
    image_operations::tool,
    native_steps::{run_command, run_command_with_input},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn output(source: &Path, cwd: &Path, suffix: &str) -> PathBuf {
    let mut name = source.file_stem().unwrap_or_default().to_os_string();
    name.push(format!("-{suffix}.pdf"));
    unique_destination(cwd, Path::new(&name))
}
fn password(values: &BTreeMap<String, String>, key: &str) -> Result<String, String> {
    match values.get(key) {
        Some(reference) if !reference.is_empty() => fleqi_application::secrets::resolve(reference),
        _ => Ok(String::new()),
    }
}
fn qpdf(job: Value, secrets: &[&str], cancel: &AtomicBool) -> Result<(), String> {
    let mut command = std::process::Command::new(tool("qpdf")?);
    command.arg("--job-json-file=/dev/stdin");
    let bytes = serde_json::to_vec(&job).map_err(|e| e.to_string())?;
    run_command_with_input(command, Some(&bytes), cancel)
        .map(|_| ())
        .map_err(|mut error| {
            for secret in secrets {
                if !secret.is_empty() {
                    error = error.replace(secret, "[redacted]");
                }
            }
            error
        })
}
fn plain_document(source: &Path, pw: &str, cancel: &AtomicBool) -> Result<lopdf::Document, String> {
    let document = lopdf::Document::load(source).map_err(|e| e.to_string())?;
    if !document.is_encrypted() {
        return materialize_resources(document);
    }
    if pw.is_empty() {
        return Err("PDF 已加密，请提供正确口令".into());
    }
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = temporary.path().join("input.pdf");
    let output = temporary.path().join("plain.pdf");
    std::fs::copy(source, &input).map_err(|e| e.to_string())?;
    qpdf(
        json!({"inputFile":input,"outputFile":output,"password":pw,"decrypt":""}),
        &[pw],
        cancel,
    )?;
    materialize_resources(lopdf::Document::load(output).map_err(|e| e.to_string())?)
}
/// Materialize the nearest inherited Resources dictionary before lopdf text extraction.
/// PDF page trees may legally store it inline at a parent node.
pub(crate) fn materialize_resources(
    mut document: lopdf::Document,
) -> Result<lopdf::Document, String> {
    for (_, page) in document.get_pages() {
        let mut current = page;
        let mut visited = std::collections::HashSet::new();
        let resource = loop {
            if !visited.insert(current) || visited.len() > 256 {
                return Err("PDF 页树包含循环或过深".into());
            }
            let node = document
                .get_dictionary(current)
                .map_err(|e| e.to_string())?;
            if let Ok(resource) = node.get(b"Resources") {
                break Some(resource.clone());
            }
            match node.get(b"Parent").and_then(lopdf::Object::as_reference) {
                Ok(parent) => current = parent,
                Err(_) => break None,
            }
        };
        if let Some(resource) = resource {
            document
                .get_dictionary_mut(page)
                .map_err(|e| e.to_string())?
                .set("Resources", resource);
        }
    }
    Ok(document)
}

fn pdf_string(object: &lopdf::Object) -> Option<String> {
    let bytes = object.as_str().ok()?;
    if bytes.starts_with(&[0xfe, 0xff]) {
        let data = &bytes[2..];
        let units = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        String::from_utf16(&units).ok()
    } else {
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

pub fn execute(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    values: &BTreeMap<String, String>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let source = sources.first().ok_or("请选择 PDF")?;
    let pw = password(values, "password")?;
    match operation {
        "CAP-PDF-006" => {
            Ok(json!({"pages":plain_document(source,&pw,cancel)?.get_pages().len()}).to_string())
        }
        "CAP-PDF-007" => {
            let document = plain_document(source, &pw, cancel)?;
            let author = document
                .trailer
                .get(b"Info")
                .ok()
                .and_then(|info| document.dereference(info).ok().map(|(_, value)| value))
                .and_then(|info| info.as_dict().ok())
                .and_then(|info| info.get(b"Author").ok())
                .and_then(pdf_string);
            Ok(
                json!({"author":author,"notSet":author.is_none(),"source":"PDF Info.Author"})
                    .to_string(),
            )
        }
        "CAP-PDF-010" => {
            let mut document = plain_document(source, &pw, cancel)?;
            let scope = values.get("scope").map(String::as_str).unwrap_or("all");
            let mut removed = vec![];
            if matches!(scope, "all" | "info") && document.trailer.remove(b"Info").is_some() {
                removed.push("Info");
            }
            if matches!(scope, "all" | "xmp") {
                for object in document.objects.values_mut() {
                    if let Ok(dictionary) = object.as_dict_mut()
                        && dictionary.remove(b"Metadata").is_some()
                    {
                        removed.push("XMP");
                    }
                }
            }
            document.prune_objects();
            let target = output(source, cwd, "metadata-removed");
            document.save(&target).map_err(|e| e.to_string())?;
            Ok(format!(
                "已生成：{}\n移除字段：{}\n",
                target.display(),
                if removed.is_empty() {
                    "原文档未设置目标字段".into()
                } else {
                    removed.join(", ")
                }
            ))
        }
        "CAP-PDF-009" => {
            // pdfimages 支持嵌入图片的原格式或 PNG 转换，输出到独立目录。
            let directory = unique_destination(cwd, Path::new("pdf-images"));
            std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
            let document = plain_document(source, &pw, cancel)?;
            let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
            let plain = temporary.path().join("plain.pdf");
            let mut document = document;
            document.save(&plain).map_err(|e| e.to_string())?;
            let mut command = std::process::Command::new(tool("pdfimages")?);
            command.arg(if values.get("format").is_some_and(|v| v == "original") {
                "-all"
            } else {
                "-png"
            });
            if let Some(first) = values.get("firstPage") {
                command.args(["-f", first]);
            }
            if let Some(last) = values.get("lastPage")
                && !last.is_empty()
            {
                command.args(["-l", last]);
            }
            command.arg(plain).arg(directory.join("image"));
            run_command(command, cancel)?;
            let mut outputs = std::fs::read_dir(&directory)
                .map_err(|e| e.to_string())?
                .map(|item| item.map(|entry| entry.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            outputs.sort();
            if outputs.is_empty() {
                std::fs::remove_dir(&directory).map_err(|e| e.to_string())?;
                return Ok("指定范围没有嵌入图片\n".into());
            }
            Ok(format!(
                "已提取 {} 个图像文件：\n{}\n",
                outputs.len(),
                outputs
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
        }
        "CAP-PDF-011" | "CAP-PDF-012" => {
            let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
            let input = temporary.path().join("input.pdf");
            let generated = temporary.path().join("output.pdf");
            std::fs::copy(source, &input).map_err(|e| e.to_string())?;
            let owner = password(values, "ownerPassword")?;
            if pw.is_empty() {
                return Err("请提供打开口令".into());
            }
            let job = if operation == "CAP-PDF-011" {
                json!({"inputFile":input,"outputFile":generated,"password":pw,"decrypt":""})
            } else {
                if owner.is_empty() || owner == pw {
                    return Err("管理口令不能为空且须与打开口令不同".into());
                }
                json!({"inputFile":input,"outputFile":generated,"encrypt":{"userPassword":pw,"ownerPassword":owner,"256bit":{"print":values.get("printing").map(String::as_str).unwrap_or("full"),"extract":if values.get("allowExtract").is_some_and(|v|v=="false"){ "n" }else{"y"}}}})
            };
            qpdf(job, &[&pw, &owner], cancel)?;
            let target = output(
                source,
                cwd,
                if operation == "CAP-PDF-011" {
                    "decrypted"
                } else {
                    "encrypted"
                },
            );
            std::fs::copy(generated, &target).map_err(|e| e.to_string())?;
            Ok(format!("已生成：{}\n", target.display()))
        }
        _ => Err("未登记的 PDF 操作".into()),
    }
}

pub fn text(source: &Path, pw: &str, cancel: &AtomicBool) -> Result<String, String> {
    let document = plain_document(source, pw, cancel)?;
    let pages = document.get_pages().keys().copied().collect::<Vec<_>>();
    document.extract_text(&pages).map_err(|e| e.to_string())
}

pub fn text_for_summary(
    source: &Path,
    pw: &str,
    pages: &str,
    ocr: bool,
    language: &str,
    cancel: &AtomicBool,
) -> Result<crate::document_operations::Extracted, String> {
    let mut document = plain_document(source, pw, cancel)?;
    let total = document.get_pages().len() as u32;
    let selected = if pages.trim().is_empty() {
        (1..=total).collect::<Vec<_>>()
    } else {
        crate::native_steps::page_numbers(pages, total)?
    };
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let plain = temporary.path().join("input.pdf");
    let mut saved = false;
    let mut text = String::new();
    let mut unreadable = vec![];
    for page in &selected {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err("已取消".into());
        }
        let body = document.extract_text(&[*page]).unwrap_or_default();
        if !body.trim().is_empty() {
            text.push_str(&format!("\n[第 {page} 页]\n{body}"));
            continue;
        }
        if !ocr {
            unreadable.push(*page);
            continue;
        }
        if !saved {
            document.save(&plain).map_err(|e| e.to_string())?;
            saved = true;
        }
        let prefix = temporary.path().join(format!("page-{page}"));
        let mut render = std::process::Command::new(tool("pdftoppm")?);
        render
            .args([
                "-f",
                &page.to_string(),
                "-l",
                &page.to_string(),
                "-singlefile",
                "-scale-to",
                "2000",
                "-png",
            ])
            .arg(&plain)
            .arg(&prefix);
        if run_command(render, cancel).is_err() {
            unreadable.push(*page);
            continue;
        }
        crate::native_steps::validate_ocr_languages(language, cancel)?;
        let mut recognize = std::process::Command::new(tool("tesseract")?);
        recognize
            .arg(prefix.with_extension("png"))
            .args(["stdout", "-l", language]);
        match run_command(recognize, cancel) {
            Ok(body) if !body.trim().is_empty() => {
                text.push_str(&format!("\n[第 {page} 页，OCR]\n{body}"))
            }
            _ => unreadable.push(*page),
        }
    }
    Ok(crate::document_operations::Extracted {
        text,
        partial: !unreadable.is_empty(),
        scope: format!("PDF 共 {total} 页；本次页码 {selected:?}；未能提取的页 {unreadable:?}"),
    })
}
