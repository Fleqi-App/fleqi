//! 纯文本与富文本文档提取；OOXML 只读主文档 XML，宏和外部实体不执行。
use crate::{capabilities::FileCapabilities, native_steps::run_command, pdf_operations};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::AtomicBool,
};
#[derive(Debug)]
pub struct Extracted {
    pub text: String,
    pub partial: bool,
    pub scope: String,
}
const MAX_TEXT_BYTES: u64 = 8 * 1024 * 1024;

pub fn extract(source: &Path, cancel: &AtomicBool) -> Result<Extracted, String> {
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    match extension.as_str() {
        "docx" | "docm" => docx(source),
        "doc" | "odt" | "rtf" | "rtfd" => {
            let mut command = std::process::Command::new("/usr/bin/textutil");
            command
                .args(["-convert", "txt", "-encoding", "UTF-8", "-stdout"])
                .arg(source);
            let text = run_command(command, cancel)?;
            let partial = text.len() >= MAX_TEXT_BYTES as usize;
            Ok(Extracted {
                text,
                partial,
                scope: "由系统文档转换器提取正文；不执行宏，不包含图片识别".into(),
            })
        }
        "pdf" => {
            let text = pdf_operations::text(source, "", cancel)?;
            Ok(Extracted {
                text,
                partial: false,
                scope: "PDF 内可提取正文；扫描页需要 OCR".into(),
            })
        }
        _ => read_range(source, "auto", 0, MAX_TEXT_BYTES),
    }
}

pub fn read_range(
    source: &Path,
    encoding: &str,
    offset: u64,
    limit: u64,
) -> Result<Extracted, String> {
    if limit == 0 || limit > MAX_TEXT_BYTES {
        return Err("每次读取范围为 1–8388608 字节".into());
    }
    let mut file = File::open(source).map_err(|e| e.to_string())?;
    let total = file.metadata().map_err(|e| e.to_string())?.len();
    if offset > total {
        return Err("起始位置超出文件长度".into());
    }
    let mut prefix = [0u8; 3];
    let prefix_len = file.read(&mut prefix).map_err(|e| e.to_string())?;
    let detected = if prefix[..prefix_len].starts_with(&[0xff, 0xfe]) {
        "utf-16le"
    } else if prefix[..prefix_len].starts_with(&[0xfe, 0xff]) {
        "utf-16be"
    } else {
        "utf-8"
    };
    let encoding = if encoding == "auto" {
        detected
    } else {
        encoding
    };
    if encoding.starts_with("utf-16") && !offset.is_multiple_of(2) {
        return Err("UTF-16 起始字节必须为偶数".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let end = offset + bytes.len() as u64;
    let partial = offset > 0 || end < total;
    let text = match encoding {
        "utf-8" => {
            let data = if offset == 0 {
                bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)
            } else {
                &bytes
            };
            let data = if offset > 0 {
                let skip = data
                    .iter()
                    .take_while(|byte| (**byte & 0xc0) == 0x80)
                    .count();
                &data[skip..]
            } else {
                data
            };
            match std::str::from_utf8(data) {
                Ok(text) => text.to_owned(),
                Err(error) if error.error_len().is_none() && end < total => {
                    std::str::from_utf8(&data[..error.valid_up_to()])
                        .map_err(|e| e.to_string())?
                        .to_owned()
                }
                Err(_) => return Err("文本不是有效 UTF-8，请选择正确编码".into()),
            }
        }
        "utf-16le" | "utf-16be" => {
            let data = if offset == 0
                && (bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]))
            {
                &bytes[2..]
            } else {
                &bytes
            };
            if data.len() % 2 != 0 && end == total {
                return Err("UTF-16 字节长度无效".into());
            }
            let mut units = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    if encoding == "utf-16le" {
                        u16::from_le_bytes(*pair)
                    } else {
                        u16::from_be_bytes(*pair)
                    }
                })
                .collect::<Vec<_>>();
            if end < total
                && units
                    .last()
                    .is_some_and(|unit| (0xd800..=0xdbff).contains(unit))
            {
                units.pop();
            }
            if offset > 0
                && units
                    .first()
                    .is_some_and(|unit| (0xdc00..=0xdfff).contains(unit))
            {
                units.remove(0);
            }
            String::from_utf16(&units).map_err(|_| "UTF-16 字符序列无效")?
        }
        _ => return Err("不支持的编码".into()),
    };
    Ok(Extracted {
        text,
        partial,
        scope: format!(
            "编码 {encoding}；字节范围 {offset}–{end} / {total}；切片边缘只保留完整字符"
        ),
    })
}

fn docx(source: &Path) -> Result<Extracted, String> {
    use quick_xml::{Reader, events::Event};
    let mut archive = zip::ZipArchive::new(File::open(source).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let entry = archive
        .by_name("word/document.xml")
        .map_err(|_| "文档缺少 word/document.xml")?;
    let partial = entry.size() > MAX_TEXT_BYTES;
    let mut reader = Reader::from_reader(std::io::BufReader::new(entry.take(MAX_TEXT_BYTES)));
    let mut buffer = Vec::new();
    let mut text = String::new();
    let mut in_text = false;
    let mut depth = 0usize;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(tag)) => {
                depth += 1;
                if tag.local_name().as_ref() == "t" {
                    in_text = true;
                }
            }
            Ok(Event::End(tag)) => {
                depth = depth.saturating_sub(1);
                match tag.local_name().as_ref() {
                    "t" => in_text = false,
                    "p" => text.push('\n'),
                    "tc" => text.push('\t'),
                    _ => {}
                }
            }
            Ok(Event::Empty(tag)) => match tag.local_name().as_ref() {
                "tab" => text.push('\t'),
                "br" => text.push('\n'),
                _ => {}
            },
            Ok(Event::Text(value)) if in_text => text.push_str(&value.xml10_content()),
            Ok(Event::CData(value)) if in_text => text.push_str(&value.xml10_content()),
            Ok(Event::GeneralRef(reference)) if in_text => {
                if let Some(character) = reference.resolve_char_ref().map_err(|e| e.to_string())? {
                    text.push(character);
                } else {
                    match reference.as_ref() {
                        "amp" => text.push('&'),
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "quot" => text.push('"'),
                        "apos" => text.push('\''),
                        _ => return Err("文档包含未定义实体".into()),
                    }
                }
            }
            Ok(Event::DocType(_)) => return Err("不读取带外部实体声明的文档".into()),
            Ok(Event::Eof) => {
                if depth > 0 && !partial {
                    return Err("文档正文 XML 不完整".into());
                }
                break;
            }
            Err(error) if partial => {
                let _ = error;
                break;
            }
            Err(error) => return Err(format!("正文 XML 无效：{error}")),
            _ => {}
        }
        buffer.clear();
    }
    Ok(Extracted {
        text: text.trim_end_matches(['\n', '\t']).to_owned(),
        partial,
        scope: "仅主文档段落与表格；不包含页眉、页脚、脚注和图片文字；不执行宏".into(),
    })
}

pub fn word_count(text: &str, unit: &str) -> Result<usize, String> {
    match unit {
        "words" => Ok(text.split_whitespace().count()),
        "characters" => Ok(text.chars().count()),
        "cjk" => Ok(text
            .chars()
            .filter(|c| matches!(*c as u32,0x3400..=0x4dbf|0x4e00..=0x9fff))
            .count()),
        _ => Err("统计单位必须为 words/characters/cjk".into()),
    }
}

pub fn write_extracted(
    source: &Path,
    directory: &Path,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let extracted = extract(source, cancel)?;
    let name = format!(
        "{}-extracted.txt",
        source.file_stem().unwrap_or_default().to_string_lossy()
    );
    let target = FileCapabilities::new()
        .create_text(directory, &name, &extracted.text, "utf-8", "lf")
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "已生成：{}\n范围：{}；{}\n",
        target.display(),
        extracted.scope,
        if extracted.partial {
            "仅部分正文"
        } else {
            "正文提取完成"
        }
    ))
}
