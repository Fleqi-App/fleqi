//! Apple HEIF/HEIC 解码。
//!
//! macOS 使用系统 `sips`（ImageIO）。Linux 使用 libheif，并应用文件里的旋转与裁切。
//! 其它平台在 PATH 上有 `heif-convert` 时走同一输出；没有解码器时明确失败，不把文件当成已转换。

use image::{DynamicImage, Rgba, RgbaImage};
use std::path::Path;
use std::sync::atomic::AtomicBool;

const HEIF_BRANDS: &[&[u8]] = &[
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"mif1", b"msf1",
];

/// ISO-BMFF `ftyp` 品牌属于 Apple HEIF/HEIC，或兼容的 HEIF 静态图像。
pub fn is_heif(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    bytes[8..]
        .chunks(4)
        .take(8)
        .any(|brand| HEIF_BRANDS.contains(&brand))
}

pub fn decode(source: &Path, cancel: &AtomicBool) -> Result<DynamicImage, String> {
    if cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err("已取消 HEIF 解码".into());
    }
    #[cfg(target_os = "macos")]
    {
        decode_with_sips(source, cancel)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = cancel;
        decode_with_libheif(source)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        decode_with_heif_convert(source, cancel)
    }
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
    crate::native_steps::run_command(command, cancel)?;
    image::open(converted).map_err(|error| format!("HEIF 无法解码：{error}"))
}

#[cfg(target_os = "linux")]
fn decode_with_libheif(source: &Path) -> Result<DynamicImage, String> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    let bytes = std::fs::read(source).map_err(|error| format!("无法读取 HEIF：{error}"))?;
    let lib = LibHeif::new_checked().map_err(|error| format!("HEIF 解码器不可用：{error}"))?;
    let context =
        HeifContext::read_from_bytes(&bytes).map_err(|error| format!("无法解析 HEIF：{error}"))?;
    let handle = context
        .primary_image_handle()
        .map_err(|error| format!("HEIF 没有主图像：{error}"))?;
    let width = handle.width();
    let height = handle.height();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 100_000_000 {
        return Err("HEIF 尺寸无效或超过 1 亿像素".into());
    }
    let decoded = lib
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .map_err(|error| format!("HEIF 解码失败：{error}"))?;
    let planes = decoded.planes();
    let plane = planes.interleaved.ok_or("HEIF 解码结果没有像素平面")?;
    if plane.bits_per_pixel != 8 {
        return Err(format!(
            "暂不支持 {} 位 HEIF，只转换 8 位图像",
            plane.bits_per_pixel
        ));
    }
    let row_bytes = usize::try_from(plane.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or("HEIF 尺寸无效")?;
    if plane.stride < row_bytes {
        return Err("HEIF 像素行距无效".into());
    }
    let mut rgba = RgbaImage::new(plane.width, plane.height);
    for y in 0..plane.height {
        let start = usize::try_from(y)
            .unwrap_or(usize::MAX)
            .saturating_mul(plane.stride);
        let end = start.checked_add(row_bytes).ok_or("HEIF 像素越界")?;
        let row = plane.data.get(start..end).ok_or("HEIF 像素不完整")?;
        for x in 0..plane.width as usize {
            let index = x * 4;
            rgba.put_pixel(
                x as u32,
                y,
                Rgba([row[index], row[index + 1], row[index + 2], row[index + 3]]),
            );
        }
    }
    Ok(DynamicImage::ImageRgba8(rgba))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn decode_with_heif_convert(source: &Path, cancel: &AtomicBool) -> Result<DynamicImage, String> {
    let program = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("heif-convert"))
        .find(|path| path.is_file())
        .ok_or(
            "当前系统没有 HEIF 解码器。请安装 heif-convert，或在 macOS 上使用系统自带的 sips。",
        )?;
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let converted = temporary.path().join("decoded.png");
    let mut command = std::process::Command::new(program);
    command.arg(source).arg(&converted);
    crate::native_steps::run_command(command, cancel)?;
    image::open(converted).map_err(|error| format!("HEIF 无法解码：{error}"))
}
