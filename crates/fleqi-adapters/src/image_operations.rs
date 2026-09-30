//! 扩展图像处理：保留透明度与原件；临时转换仅在本次临时目录中进行。
use crate::{capabilities::unique_destination, native_steps::run_command};
use image::{DynamicImage, GenericImageView, ImageDecoder, Rgba, RgbaImage};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub fn open_image(source: &Path) -> Result<DynamicImage, String> {
    load(source, &AtomicBool::new(false))
}

pub fn load(source: &Path, cancel: &AtomicBool) -> Result<DynamicImage, String> {
    let header = read_header(source)?;
    if crate::heif::is_heif(&header) {
        return crate::heif::decode(source, cancel);
    }
    match decode_with_image_crate(source) {
        Ok(image) => Ok(image),
        Err(error) => {
            #[cfg(target_os = "macos")]
            {
                let _ = error;
                decode_with_sips(source, cancel)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(format!("输入无法解码：{error}"))
            }
        }
    }
}

fn read_header(source: &Path) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(source).map_err(|error| format!("无法读取图像：{error}"))?;
    let mut header = vec![0; 64];
    use std::io::Read;
    let count = file
        .read(&mut header)
        .map_err(|error| format!("无法读取图像：{error}"))?;
    header.truncate(count);
    Ok(header)
}

fn decode_with_image_crate(source: &Path) -> image::ImageResult<DynamicImage> {
    let reader = image::ImageReader::open(source)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut decoded = DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    Ok(decoded)
}

#[cfg(target_os = "macos")]
fn decode_with_sips(source: &Path, cancel: &AtomicBool) -> Result<DynamicImage, String> {
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let converted = temporary.path().join("decoded.png");
    let mut command = std::process::Command::new("/usr/bin/sips");
    command
        .args(["-s", "format", "png"])
        .arg(source)
        .arg("--out")
        .arg(&converted);
    run_command(command, cancel)?;
    image::open(converted).map_err(|error| format!("输入无法解码：{error}"))
}
fn color(value: &str) -> Result<Rgba<u8>, String> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 && value.len() != 8 {
        return Err("颜色必须为 #RRGGBB 或 #RRGGBBAA".into());
    }
    let mut rgba = [0, 0, 0, 255];
    for (i, component) in rgba.iter_mut().enumerate().take(value.len() / 2) {
        *component =
            u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| "颜色包含无效字符")?;
    }
    Ok(Rgba(rgba))
}
fn dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 100_000_000 {
        Err("输出尺寸必须为正，且不超过 1 亿像素".into())
    } else {
        Ok(())
    }
}
fn output(cwd: &Path, source: &Path, suffix: &str, extension: &str) -> PathBuf {
    let mut name = source.file_stem().unwrap_or_default().to_os_string();
    name.push(format!("-{suffix}.{extension}"));
    unique_destination(cwd, Path::new(&name))
}

pub fn execute(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    values: &BTreeMap<String, String>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    execute_report(operation, sources, cwd, values, cancel).map(|report| report.output)
}

pub fn execute_report(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    values: &BTreeMap<String, String>,
    cancel: &AtomicBool,
) -> Result<fleqi_application::run_service::NativeOutput, String> {
    if matches!(operation, "CAP-IMAGE-012" | "CAP-IMAGE-015") {
        return execute_one(operation, sources, cwd, values, cancel).map(Into::into);
    }
    let mut inputs = Vec::new();
    let mut pending = sources.to_vec();
    let recursive = values.get("recursive").is_some_and(|value| value == "true");
    let mut failures = Vec::new();
    while let Some(path) = pending.pop() {
        if cancel.load(Ordering::Acquire) {
            return Err("已取消图像批处理".into());
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let kind = entry.file_type().map_err(|e| e.to_string())?;
                if kind.is_dir() {
                    if recursive {
                        pending.push(entry.path());
                    }
                } else if kind.is_file()
                    && entry
                        .path()
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|ext| {
                            matches!(
                                ext.to_ascii_lowercase().as_str(),
                                "png"
                                    | "jpg"
                                    | "jpeg"
                                    | "webp"
                                    | "bmp"
                                    | "tif"
                                    | "tiff"
                                    | "gif"
                                    | "heic"
                                    | "heif"
                                    | "ico"
                            )
                        })
                {
                    inputs.push(entry.path());
                }
            }
        } else {
            inputs.push(path);
        }
        if inputs.len() + pending.len() > 10000 {
            return Err("图像范围超过 10000 项，请缩小范围".into());
        }
    }
    if let Some(formats) = values
        .get("formats")
        .filter(|value| !value.trim().is_empty())
    {
        let formats: Vec<_> = formats
            .split(',')
            .map(|value| value.trim().to_ascii_lowercase())
            .collect();
        inputs.retain(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| formats.contains(&ext.to_ascii_lowercase()))
        });
    }
    inputs.sort();
    inputs.dedup();
    if inputs.is_empty() {
        return Err("范围内没有可处理的图像".into());
    }
    let total = inputs.len();
    let mut outputs = Vec::new();
    for input in inputs {
        if cancel.load(Ordering::Acquire) {
            return Err(format!(
                "已取消；已完成 {} 项\n{}",
                outputs.len(),
                outputs.join("\n")
            ));
        }
        let destination = if values
            .get("_besideSource")
            .is_some_and(|value| value == "true")
        {
            input.parent().unwrap_or(cwd)
        } else {
            cwd
        };
        match execute_one(
            operation,
            std::slice::from_ref(&input),
            destination,
            values,
            cancel,
        ) {
            Ok(output) => outputs.push(output),
            Err(error) => failures.push(format!("{}：{error}", input.display())),
        }
    }
    if outputs.is_empty() {
        return Err(failures.join("\n"));
    }
    Ok(fleqi_application::run_service::NativeOutput {
        output: format!(
            "范围 {} 项；成功 {} 项；失败 {} 项\n{}\n{}",
            total,
            outputs.len(),
            failures.len(),
            outputs.join("\n"),
            failures.join("\n")
        ),
        partial: !failures.is_empty(),
    })
}

fn execute_one(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    values: &BTreeMap<String, String>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let first = sources.first().ok_or("请选择图像")?;
    let value = |key: &str, default: &str| {
        values
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let number = |key: &str, default: &str| -> Result<u32, String> {
        value(key, default)
            .parse()
            .map_err(|_| format!("{key} 需要整数"))
    };
    let mut image = load(first, cancel)?;
    let mut target = output(cwd, first, operation, "png");
    match operation {
        "image.rotate" => {
            image = match number("angle", "90")? {
                90 => image.rotate90(),
                180 => image.rotate180(),
                270 => image.rotate270(),
                _ => return Err("旋转角度必须为 90/180/270".into()),
            };
        }
        "image.resize" => {
            let mode = value("mode", "box");
            let width = number("width", "1920")?;
            let height = number("height", "1080")?;
            let upscale = value("upscale", "false") == "true";
            let factor = match mode.as_str() {
                "width" => width as f64 / image.width() as f64,
                "height" => height as f64 / image.height() as f64,
                "box" => {
                    (width as f64 / image.width() as f64).min(height as f64 / image.height() as f64)
                }
                _ => return Err("缩放方式无效".into()),
            };
            let factor = if upscale { factor } else { factor.min(1.0) };
            let (width, height) = (
                ((image.width() as f64 * factor).round() as u32).max(1),
                ((image.height() as f64 * factor).round() as u32).max(1),
            );
            dimensions(width, height)?;
            image = image.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
        }
        "CAP-IMAGE-007" => {} // 重编码生成新文件，不复制 EXIF/XMP；像素/alpha 保持。
        "CAP-IMAGE-008" => {
            let left = number("left", "0")?;
            let right = number("right", "0")?;
            let top = number("top", "0")?;
            let bottom = number("bottom", "0")?;
            let width = image
                .width()
                .checked_sub(left.checked_add(right).ok_or("裁切越界")?)
                .ok_or("裁切越界")?;
            let height = image
                .height()
                .checked_sub(top.checked_add(bottom).ok_or("裁切越界")?)
                .ok_or("裁切越界")?;
            dimensions(width, height)?;
            image = image.crop_imm(left, top, width, height);
        }
        "CAP-IMAGE-009" => {
            let background = color(&value("color", "#ffffff"))?;
            let tolerance = number("tolerance", "0")?;
            if tolerance > 255 {
                return Err("颜色容差范围为 0–255".into());
            }
            let mut rgba = image.to_rgba8();
            for pixel in rgba.pixels_mut() {
                if (0..3).all(|i| pixel[i].abs_diff(background[i]) as u32 <= tolerance) {
                    pixel[3] = 0;
                }
            }
            image = DynamicImage::ImageRgba8(rgba);
        }
        "CAP-IMAGE-011" => {
            target = output(cwd, first, "icon", "ico");
            let sizes = parse_sizes(&value("sizes", "16,32,48,64,128,256"), 256)?;
            let images: Vec<_> = sizes
                .into_iter()
                .map(|size| {
                    image
                        .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
                        .to_rgba8()
                })
                .collect();
            let frames = images
                .iter()
                .map(|buffer| {
                    image::codecs::ico::IcoFrame::as_png(
                        buffer.as_raw(),
                        buffer.width(),
                        buffer.height(),
                        image::ExtendedColorType::Rgba8,
                    )
                    .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            image::codecs::ico::IcoEncoder::new(
                std::fs::File::create(&target).map_err(|e| e.to_string())?,
            )
            .encode_images(&frames)
            .map_err(|e| e.to_string())?;
            return Ok(format!("已生成：{}\n", target.display()));
        }
        "CAP-IMAGE-010" => {
            let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
            let iconset = temporary.path().join("output.iconset");
            std::fs::create_dir(&iconset).map_err(|e| e.to_string())?;
            for size in [16, 32, 128, 256, 512] {
                for retina in [1, 2] {
                    let dimensions = size * retina;
                    let name = if retina == 2 {
                        format!("icon_{size}x{size}@2x.png")
                    } else {
                        format!("icon_{size}x{size}.png")
                    };
                    image
                        .resize_exact(
                            dimensions,
                            dimensions,
                            image::imageops::FilterType::Lanczos3,
                        )
                        .save(iconset.join(name))
                        .map_err(|e| e.to_string())?;
                }
            }
            target = output(cwd, first, "icon", "icns");
            let mut command = std::process::Command::new("/usr/bin/iconutil");
            command
                .args(["-c", "icns"])
                .arg(&iconset)
                .arg("-o")
                .arg(&target);
            run_command(command, cancel)?;
            return Ok(format!("已生成：{}\n", target.display()));
        }
        "CAP-IMAGE-012" => {
            target = output(cwd, first, "animation", "gif");
            let duration = number("durationMs", "100")?;
            if duration == 0 {
                return Err("帧时长必须大于零".into());
            }
            let loops = number("loops", "0")?;
            if loops > u16::MAX as u32 {
                return Err("循环次数超出范围".into());
            }
            let mut encoder = image::codecs::gif::GifEncoder::new(
                std::fs::File::create(&target).map_err(|e| e.to_string())?,
            );
            encoder
                .set_repeat(if loops == 0 {
                    image::codecs::gif::Repeat::Infinite
                } else {
                    image::codecs::gif::Repeat::Finite(loops as u16)
                })
                .map_err(|e| e.to_string())?;
            for source in sources {
                if cancel.load(Ordering::Acquire) {
                    drop(encoder);
                    let _ = std::fs::remove_file(&target);
                    return Err("已取消".into());
                }
                let frame = load(source, cancel)?.to_rgba8();
                if frame.dimensions() != image.dimensions() {
                    return Err("GIF 各帧尺寸必须一致".into());
                }
                encoder
                    .encode_frame(image::Frame::from_parts(
                        frame,
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(duration, 1),
                    ))
                    .map_err(|e| e.to_string())?;
            }
            return Ok(format!(
                "已生成：{}\n帧数 {}，帧时长 {} ms\n",
                target.display(),
                sources.len(),
                duration
            ));
        }
        "CAP-IMAGE-013" => {
            let sigma = value("radius", "2")
                .parse::<f32>()
                .map_err(|_| "模糊半径无效")?;
            if !sigma.is_finite() || !(0.0..=100.0).contains(&sigma) {
                return Err("模糊半径范围 0–100".into());
            }
            image = image.blur(sigma);
        }
        "CAP-IMAGE-014" => {
            let border = number("width", "10")?;
            let double = border.checked_mul(2).ok_or("边框越界")?;
            let width = image.width().checked_add(double).ok_or("宽度越界")?;
            let height = image.height().checked_add(double).ok_or("高度越界")?;
            dimensions(width, height)?;
            let mut canvas =
                RgbaImage::from_pixel(width, height, color(&value("color", "#000000"))?);
            image::imageops::overlay(&mut canvas, &image.to_rgba8(), border as i64, border as i64);
            image = DynamicImage::ImageRgba8(canvas);
        }
        "CAP-IMAGE-015" => {
            let columns = number("columns", "3")?;
            let rows = number("rows", "3")?;
            let gap = number("gap", "0")?;
            let cell = number("cellSize", "256")?;
            if columns == 0
                || rows == 0
                || sources.len() as u64 > u64::from(columns) * u64::from(rows)
            {
                return Err("网格数量与输入不符".into());
            }
            let width = columns
                .checked_mul(cell)
                .and_then(|v| gap.checked_mul(columns - 1).and_then(|g| v.checked_add(g)))
                .ok_or("网格过大")?;
            let height = rows
                .checked_mul(cell)
                .and_then(|v| gap.checked_mul(rows - 1).and_then(|g| v.checked_add(g)))
                .ok_or("网格过大")?;
            dimensions(width, height)?;
            let mut canvas =
                RgbaImage::from_pixel(width, height, color(&value("color", "#ffffff"))?);
            for (index, source) in sources.iter().enumerate() {
                if cancel.load(Ordering::Acquire) {
                    return Err("已取消".into());
                }
                let thumbnail = load(source, cancel)?.thumbnail(cell, cell).to_rgba8();
                let x = (index as u32 % columns) * (cell + gap) + (cell - thumbnail.width()) / 2;
                let y = (index as u32 / columns) * (cell + gap) + (cell - thumbnail.height()) / 2;
                image::imageops::overlay(&mut canvas, &thumbnail, x as i64, y as i64);
            }
            image = DynamicImage::ImageRgba8(canvas);
        }
        "CAP-IMAGE-016" => {
            let tint = color(&value("color", "#0000ff"))?;
            let strength = value("strength", "0.3")
                .parse::<f32>()
                .map_err(|_| "着色强度无效")?;
            if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
                return Err("着色强度范围 0–1".into());
            }
            let mut rgba = image.to_rgba8();
            for pixel in rgba.pixels_mut() {
                for i in 0..3 {
                    pixel[i] = (pixel[i] as f32 * (1.0 - strength) + tint[i] as f32 * strength)
                        .round() as u8;
                }
            }
            image = DynamicImage::ImageRgba8(rgba);
        }
        "CAP-IMAGE-017" => {
            let executable = tool("magick")?;
            let font = value("font", "/System/Library/Fonts/Hiragino Sans GB.ttc");
            if font.contains('/') && !Path::new(&font).is_file() {
                return Err("指定字体不存在，请选择已安装字体".into());
            }
            let mut command = std::process::Command::new(executable);
            command
                .arg(first)
                .args([
                    "-font",
                    &font,
                    "-pointsize",
                    &value("size", "32"),
                    "-fill",
                    &value("color", "#000000"),
                    "-gravity",
                    "NorthWest",
                    "-annotate",
                    &format!("+{}+{}", number("x", "0")?, number("y", "0")?),
                    &value("text", "").replace('%', "%%"),
                ])
                .arg(&target);
            run_command(command, cancel)?;
            return Ok(format!("已生成：{}\n", target.display()));
        }
        _ => return Err("未登记的图像处理".into()),
    }
    let format = if operation == "CAP-IMAGE-009" {
        value("format", "png")
    } else {
        "png".into()
    };
    let mut note = String::new();
    match format.as_str() {
        "png" => image.save_with_format(&target, image::ImageFormat::Png),
        "webp" => {
            target = output(cwd, first, operation, "webp");
            image.save_with_format(&target, image::ImageFormat::WebP)
        }
        "jpg" | "jpeg" => {
            if value("alphaPolicy", "reject") != "flatten" {
                return Err(
                    "JPEG 不支持透明度；请选择 PNG/WebP，或明确选择 alphaPolicy=flatten 和背景颜色"
                        .into(),
                );
            }
            let background = color(&value("background", "#ffffff"))?;
            let rgba = image.to_rgba8();
            let flattened = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                let pixel = rgba.get_pixel(x, y);
                let alpha = u32::from(pixel[3]);
                image::Rgb(std::array::from_fn(|i| {
                    ((u32::from(pixel[i]) * alpha + u32::from(background[i]) * (255 - alpha) + 127)
                        / 255) as u8
                }))
            });
            image = DynamicImage::ImageRgb8(flattened);
            target = output(cwd, first, operation, "jpg");
            note = format!(
                "JPEG 不支持透明度，已明确合成到背景 {}。\n",
                value("background", "#ffffff")
            );
            image.save_with_format(&target, image::ImageFormat::Jpeg)
        }
        _ => return Err("输出格式只支持 PNG/WebP/JPEG".into()),
    }
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "已生成：{}\n尺寸 {} × {}\n{note}",
        target.display(),
        image.width(),
        image.height()
    ))
}
fn parse_sizes(text: &str, max: u32) -> Result<Vec<u32>, String> {
    let sizes = text
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<u32>()
                .map_err(|_| "图层尺寸无效".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if sizes.is_empty() || sizes.iter().any(|n| *n == 0 || *n > max) {
        Err(format!("图层尺寸范围为 1–{max}"))
    } else {
        Ok(sizes)
    }
}
/// 宿主注入工具服务；捕获 Weak<ToolService>，避免全局解析器持有宿主生命周期。
pub type ToolResolver = dyn Fn(&str) -> Result<Option<PathBuf>, String> + Send + Sync;
static TOOL_RESOLVER: std::sync::RwLock<Option<std::sync::Arc<ToolResolver>>> =
    std::sync::RwLock::new(None);

pub fn set_tool_resolver(resolver: std::sync::Arc<ToolResolver>) -> Result<(), String> {
    *TOOL_RESOLVER.write().map_err(|_| "工具解析器锁不可用")? = Some(resolver);
    Ok(())
}

fn executable_path(path: &Path) -> Option<PathBuf> {
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    // 保留可执行符号链接的名称：部分工具会按 argv[0] 决定子命令。
    // 只将相对 PATH 条目转为绝对路径，避免后续 Command::current_dir 改变解析位置。
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(std::env::current_dir().ok()?.join(path))
    }
}

pub fn tool(name: &str) -> Result<PathBuf, String> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err("工具解析需要单个可执行文件名".into());
    }
    // 回调会运行目标版本探测；先释放锁，不将文件系统/子进程操作置于全局锁内。
    let resolver = TOOL_RESOLVER
        .read()
        .map_err(|_| "工具解析器锁不可用")?
        .clone();
    let mut resolver_error = None;
    if let Some(resolver) = resolver {
        match resolver(name) {
            Ok(Some(path)) => {
                if let Some(path) = executable_path(&path) {
                    return Ok(path);
                }
            }
            Ok(None) => (),
            Err(error) => resolver_error = Some(error),
        }
    }
    let mut roots = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    roots.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    for root in roots {
        if let Some(path) = executable_path(&root.join(name)) {
            return Ok(path);
        }
    }
    Err(match resolver_error {
        Some(error) => format!("缺少工具 {name}；工具服务解析失败：{error}；请在工具页检查状态"),
        None => format!("缺少工具 {name}，请先在工具页安装或配置"),
    })
}
