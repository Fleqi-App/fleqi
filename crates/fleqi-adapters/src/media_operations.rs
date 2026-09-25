//! 媒体探测、几何处理与本地转写；数值来自 ffprobe，缺字段保留 unknown。
use crate::{capabilities::unique_destination, image_operations::tool, native_steps::run_command};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub fn probe(source: &Path, cancel: &AtomicBool) -> Result<Value, String> {
    let mut command = std::process::Command::new(tool("ffprobe")?);
    command
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(source);
    serde_json::from_str(&run_command(command, cancel)?)
        .map_err(|e| format!("媒体探测响应无效：{e}"))
}
fn numeric(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
        })
        .filter(|n| n.is_finite())
}
pub fn video_info(source: &Path, track: usize, cancel: &AtomicBool) -> Result<Value, String> {
    let data = probe(source, cancel)?;
    let stream = data["streams"]
        .as_array()
        .ok_or("没有媒体轨道")?
        .iter()
        .filter(|stream| stream["codec_type"] == "video")
        .nth(track)
        .ok_or("指定视频轨道不存在")?;
    let width = stream["width"].as_u64().ok_or("未提供视频宽度")?;
    let height = stream["height"].as_u64().ok_or("未提供视频高度")?;
    let rotation = stream["side_data_list"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| numeric(item.get("rotation"))))
        .or_else(|| numeric(stream["tags"].get("rotate")))
        .unwrap_or(0.0);
    let swap = (rotation.round() as i64).rem_euclid(180) != 0;
    Ok(
        json!({"streamIndex":stream["index"],"encodedWidth":width,"encodedHeight":height,"displayWidth":if swap{height}else{width},"displayHeight":if swap{width}else{height},"rotation":rotation,"codec":stream["codec_name"],"bitrate":numeric(stream.get("bit_rate")),"bitrateSource":"stream metadata","durationSeconds":numeric(stream.get("duration"))}),
    )
}
fn output(source: &Path, cwd: &Path, suffix: &str, extension: &str) -> PathBuf {
    let mut filename = source.file_stem().unwrap_or_default().to_os_string();
    filename.push(format!("-{suffix}.{extension}"));
    unique_destination(cwd, Path::new(&filename))
}

pub fn execute(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    parameters: &BTreeMap<String, String>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let source = sources.first().ok_or("请先选择媒体文件")?;
    let value = |key: &str, default: &str| {
        parameters
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.into())
    };
    let track = value("track", "0")
        .parse::<usize>()
        .map_err(|_| "轨道序号无效")?;
    match operation {
        "CAP-MEDIA-005" => {
            let data = probe(source, cancel)?;
            let seconds = numeric(data["format"].get("duration"));
            Ok(json!({"durationSeconds":seconds,"source":"container metadata","unknown":seconds.is_none()}).to_string())
        }
        "CAP-MEDIA-006" | "CAP-MEDIA-008" => Ok(video_info(source, track, cancel)?.to_string()),
        "CAP-MEDIA-007" => {
            let data = probe(source, cancel)?;
            let (rate, origin) = if value("scope", "container") == "stream" {
                (
                    video_info(source, track, cancel)?["bitrate"].as_f64(),
                    "stream metadata",
                )
            } else {
                (
                    numeric(data["format"].get("bit_rate")),
                    "container metadata",
                )
            };
            Ok(json!({"bitsPerSecond":rate,"source":origin,"estimated":false,"unknown":rate.is_none()}).to_string())
        }
        "CAP-MEDIA-009" | "CAP-MEDIA-010" | "CAP-MEDIA-011" => transcribe(
            source,
            cwd,
            if operation == "CAP-MEDIA-009" {
                "txt"
            } else if operation == "CAP-MEDIA-010" {
                "srt"
            } else {
                "vtt"
            },
            &value("language", "auto"),
            parameters.get("model").map(PathBuf::from),
            cancel,
        ),
        "CAP-MEDIA-012" | "CAP-MEDIA-013" => {
            let info = video_info(source, track, cancel)?;
            let filter = if operation == "CAP-MEDIA-012" {
                match value("angle", "90").as_str() {
                    "90" => "transpose=clock".into(),
                    "180" => "hflip,vflip".into(),
                    "270" => "transpose=cclock".into(),
                    _ => return Err("旋转角度必须是 90/180/270".into()),
                }
            } else {
                let rw = value("ratioWidth", "16")
                    .parse::<u32>()
                    .map_err(|_| "宽高比无效")?;
                let rh = value("ratioHeight", "10")
                    .parse::<u32>()
                    .map_err(|_| "宽高比无效")?;
                if rw == 0 || rh == 0 {
                    return Err("宽高比必须为正".into());
                }
                let width = info["displayWidth"].as_u64().ok_or("宽度未知")? as u32;
                let height = info["displayHeight"].as_u64().ok_or("高度未知")? as u32;
                let ratio = rw as f64 / rh as f64;
                let (cw, ch) = if width as f64 / height as f64 > ratio {
                    ((height as f64 * ratio) as u32 / 2 * 2, height / 2 * 2)
                } else {
                    (width / 2 * 2, (width as f64 / ratio) as u32 / 2 * 2)
                };
                if cw < 2 || ch < 2 {
                    return Err("裁切区域过小".into());
                }
                let (x, y) = match value("anchor", "center").as_str() {
                    "center" => ((width - cw) / 2, (height - ch) / 2),
                    "topLeft" => (0, 0),
                    "bottomRight" => (width - cw, height - ch),
                    _ => return Err("裁切锚点无效".into()),
                };
                format!("crop={cw}:{ch}:{x}:{y}")
            };
            let target = output(source, cwd, "processed", "mp4");
            let mut command = std::process::Command::new(tool("ffmpeg")?);
            command
                .args(["-nostdin", "-v", "error", "-n", "-i"])
                .arg(source)
                .args([
                    "-map",
                    &format!("0:v:{track}"),
                    "-map",
                    "0:a?",
                    "-vf",
                    &filter,
                    "-c:v",
                    "libx264",
                    "-c:a",
                    "aac",
                    "-metadata:s:v:0",
                    "rotate=0",
                ])
                .arg(&target);
            if let Err(error) = run_command(command, cancel) {
                let _ = std::fs::remove_file(&target);
                return Err(error);
            }
            let verified = video_info(&target, 0, cancel)?;
            Ok(format!("已生成：{}\n{}\n", target.display(), verified))
        }
        _ => Err("未登记的媒体操作".into()),
    }
}

pub fn find_model() -> Option<PathBuf> {
    let mut directories = vec![
        PathBuf::from("/opt/homebrew/share/whisper.cpp"),
        PathBuf::from("/usr/local/share/whisper.cpp"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        directories.push(PathBuf::from(&home).join(".fleqi/models"));
        directories.push(PathBuf::from(home).join(".cache/whisper"));
    }
    for directory in directories {
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("ggml-") && name.ends_with(".bin") {
                    return Some(entry.path());
                }
            }
        }
    }
    None
}

pub fn transcribe(
    source: &Path,
    cwd: &Path,
    format: &str,
    language: &str,
    model: Option<PathBuf>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if !["txt", "srt", "vtt"].contains(&format) {
        return Err("转写输出格式无效".into());
    }
    let model = model
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(find_model)
        .ok_or("缺少本地转写模型：请安装 ggml 模型并指定模型文件")?;
    if !model.is_file() {
        return Err("指定转写模型不存在".into());
    }
    let data = probe(source, cancel)?;
    if !data["streams"]
        .as_array()
        .is_some_and(|streams| streams.iter().any(|stream| stream["codec_type"] == "audio"))
    {
        return Err("媒体没有音轨，不能转写".into());
    }
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let wav = directory.path().join("input.wav");
    let prefix = directory.path().join("transcript");
    let mut convert = std::process::Command::new(tool("ffmpeg")?);
    convert
        .args(["-nostdin", "-v", "error", "-n", "-i"])
        .arg(source)
        .args([
            "-map",
            "0:a:0",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&wav);
    run_command(convert, cancel)?;
    let mut command = std::process::Command::new(tool("whisper-cli")?);
    command
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .args(["-l", language, "-nt", "-of"])
        .arg(&prefix)
        .arg(format!("-o{format}"));
    run_command(command, cancel)?;
    let generated = prefix.with_extension(format);
    let text = std::fs::read_to_string(&generated).map_err(|e| format!("转写未生成输出：{e}"))?;
    if format == "srt" && !text.trim().is_empty() && !text.contains(" --> ") {
        return Err("转写输出不是合法 SRT".into());
    }
    if format == "vtt" && !text.starts_with("WEBVTT") {
        return Err("转写输出不是合法 WebVTT".into());
    }
    let destination = output(source, cwd, "transcript", format);
    std::fs::copy(&generated, &destination).map_err(|e| e.to_string())?;
    Ok(format!(
        "已生成：{}\n{}",
        destination.display(),
        if text.trim().is_empty() {
            "没有识别到语音"
        } else {
            &text
        }
    ))
}
