//! Apple HEIF/HEIC 解码。
//!
//! macOS 使用系统 `sips`（ImageIO）。Linux 使用 libheif，并应用文件里的旋转与裁切。
//! 其它平台在 PATH 上有 `heif-convert` 时走同一输出；没有解码器时明确失败，不把文件当成已转换。

use image::DynamicImage;
#[cfg(target_os = "linux")]
use image::{Rgba, RgbaImage};
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
    let program = crate::tools::lookup_on_path("heif-convert").ok_or(
        "当前系统没有 HEIF 解码器。请安装 heif-convert，或在 macOS 上使用系统自带的 sips。",
    )?;
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let converted = temporary.path().join("decoded.png");
    let mut command = std::process::Command::new(&program);
    command.arg(source).arg(&converted);
    crate::native_steps::run_command(command, cancel)?;
    if !converted.is_file()
        && temporary.path().join("decoded-1.png").is_file()
        && temporary.path().join("decoded-2.png").is_file()
    {
        // heif-convert 会给 collection 的每个顶层图像编号；副本只暴露 pitm 声明的主图。
        // ponytail: collection 的主图会重复解码；大 collection 有性能需求时再前置筛选。
        let primary = temporary.path().join("primary.heic");
        copy_primary_image(source, &primary, cancel)?;
        let mut command = std::process::Command::new(program);
        command.arg(primary).arg(&converted);
        crate::native_steps::run_command(command, cancel)?;
    }
    image::open(converted).map_err(|error| format!("HEIF 无法解码：{error}"))
}

#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
fn box_size(header: &[u8], available: u64) -> Result<(usize, u64), String> {
    let size = u32::from_be_bytes(
        header
            .get(..4)
            .ok_or("HEIF box 头不完整")?
            .try_into()
            .unwrap(),
    );
    let (header_size, size) = match size {
        0 => (8, available),
        1 => (
            16,
            u64::from_be_bytes(
                header
                    .get(8..16)
                    .ok_or("HEIF 扩展 box 头不完整")?
                    .try_into()
                    .unwrap(),
            ),
        ),
        size => (8, u64::from(size)),
    };
    if header.len() < header_size || size < header_size as u64 || size > available {
        return Err("HEIF box 长度越界".into());
    }
    Ok((header_size, size))
}

#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
fn primary_flags(metadata: &[u8]) -> Result<Vec<(usize, u8)>, String> {
    if metadata.first() != Some(&0) || metadata.len() < 4 {
        return Err("HEIF meta 版本或长度无效".into());
    }
    let mut primary = None;
    let mut item_info = None;
    let mut offset = 4;
    while offset < metadata.len() {
        let data = &metadata[offset..];
        let (header, size) = box_size(data, data.len() as u64)?;
        let size = size as usize;
        let body = &data[header..size];
        match &data[4..8] {
            b"pitm" => {
                if primary.is_some() {
                    return Err("HEIF 包含重复 pitm".into());
                }
                primary = Some(match body.first() {
                    Some(0) if body.len() >= 6 => {
                        u32::from(u16::from_be_bytes(body[4..6].try_into().unwrap()))
                    }
                    Some(1) if body.len() >= 8 => {
                        u32::from_be_bytes(body[4..8].try_into().unwrap())
                    }
                    _ => return Err("HEIF pitm 版本或长度无效".into()),
                });
            }
            b"iinf" if item_info.is_some() => return Err("HEIF 包含重复 iinf".into()),
            b"iinf" => item_info = Some((offset + header, body)),
            _ => (),
        }
        offset += size;
    }
    let primary = primary.ok_or("HEIF 缺少 pitm 主图声明")?;
    let (base, items) = item_info.ok_or("HEIF 缺少 iinf")?;
    let (mut offset, count) = match items.first() {
        Some(0) if items.len() >= 6 => (
            6,
            u32::from(u16::from_be_bytes(items[4..6].try_into().unwrap())),
        ),
        Some(1) if items.len() >= 8 => (8, u32::from_be_bytes(items[4..8].try_into().unwrap())),
        _ => return Err("HEIF iinf 版本或长度无效".into()),
    };
    let mut flags = Vec::new();
    let mut seen_primary = false;
    for _ in 0..count {
        let data = items.get(offset..).ok_or("HEIF iinf 条目越界")?;
        let (header, size) = box_size(data, data.len() as u64)?;
        let body = &data[header..size as usize];
        if &data[4..8] != b"infe" {
            return Err("HEIF iinf 包含无效条目".into());
        }
        let id = match body.first() {
            Some(2) if body.len() >= 12 => {
                u32::from(u16::from_be_bytes(body[4..6].try_into().unwrap()))
            }
            Some(3) if body.len() >= 14 => u32::from_be_bytes(body[4..8].try_into().unwrap()),
            _ => return Err("HEIF infe 版本或长度无效".into()),
        };
        if id == primary {
            if seen_primary || body[3] & 1 != 0 {
                return Err("HEIF 主图重复或已隐藏".into());
            }
            seen_primary = true;
        } else {
            flags.push((base + offset + header + 3, body[3] | 1));
        }
        offset += size as usize;
    }
    if !seen_primary || offset != items.len() {
        return Err("HEIF 主图不存在或条目数量不符".into());
    }
    Ok(flags)
}

#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
fn copy_primary_image(source: &Path, target: &Path, cancel: &AtomicBool) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut input = std::fs::File::open(source).map_err(|error| error.to_string())?;
    // 新建私有文件，不复制原件的只读属性；数据、引用和所有 box 长度保持不变。
    let mut copy = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| error.to_string())?;
    let mut buffer = [0; 64 * 1024];
    loop {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err("已取消 HEIF 解码".into());
        }
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        copy.write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
    }
    // 从同一副本读取并修改，避免源文件并发变化后将旧 offset 应用于新数据。
    let length = copy.metadata().map_err(|error| error.to_string())?.len();
    let mut offset = 0;
    let (metadata_offset, flags) = loop {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err("已取消 HEIF 解码".into());
        }
        if length.saturating_sub(offset) < 8 {
            return Err("HEIF 缺少有效 meta".into());
        }
        copy.seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        let mut header = [0; 16];
        copy.read_exact(&mut header[..8])
            .map_err(|error| error.to_string())?;
        if header[..4] == [0, 0, 0, 1] {
            copy.read_exact(&mut header[8..])
                .map_err(|error| error.to_string())?;
        }
        let (header_size, size) = box_size(&header, length - offset)?;
        if &header[4..8] == b"meta" {
            let bytes = size - header_size as u64;
            // ponytail: 元数据上限 16 MiB；真实大 collection 需要时再扩大，像素数据不读入内存。
            if bytes > 16 * 1024 * 1024 {
                return Err("HEIF meta 超过 16 MiB，无法选择主图".into());
            }
            let mut metadata = vec![0; bytes as usize];
            copy.read_exact(&mut metadata)
                .map_err(|error| error.to_string())?;
            break (offset + header_size as u64, primary_flags(&metadata)?);
        }
        offset += size;
    };
    for (position, value) in flags {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err("已取消 HEIF 解码".into());
        }
        copy.seek(SeekFrom::Start(metadata_offset + position as u64))
            .and_then(|_| copy.write_all(&[value]))
            .map_err(|error| error.to_string())?;
    }
    if cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err("已取消 HEIF 解码".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_decode_does_not_require_a_source_or_decoder() {
        let error = decode(Path::new("missing.heic"), &AtomicBool::new(true)).unwrap_err();
        assert_eq!(error, "已取消 HEIF 解码");
    }

    #[test]
    fn primary_copy_preserves_readonly_source_and_hides_only_the_other_item() {
        let collection = include_bytes!("../tests/fixtures/heif-primary-second-rotated.heic");
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("readonly.heic");
        let target = directory.path().join("private.heic");
        std::fs::write(&source, collection).unwrap();
        let original_permissions = source.metadata().unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&source, readonly).unwrap();
        copy_primary_image(&source, &target, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), collection);
        assert!(source.metadata().unwrap().permissions().readonly());
        assert!(!target.metadata().unwrap().permissions().readonly());
        let copied = std::fs::read(target).unwrap();
        let changes: Vec<_> = collection
            .iter()
            .zip(&copied)
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(copied.len(), collection.len());
        assert_eq!(changes, vec![(&0, &1)]);
        std::fs::set_permissions(source, original_permissions).unwrap();
    }

    #[test]
    fn primary_metadata_checks_lengths_versions_and_wide_item_ids() {
        let make_box = |kind: &[u8; 4], body: &[u8], extended: bool| {
            let header = if extended { 16 } else { 8 };
            let mut bytes = if extended {
                1u32
            } else {
                (header + body.len()) as u32
            }
            .to_be_bytes()
            .to_vec();
            bytes.extend_from_slice(kind);
            if extended {
                bytes.extend_from_slice(&((header + body.len()) as u64).to_be_bytes());
            }
            bytes.extend_from_slice(body);
            bytes
        };
        let mut metadata = vec![0; 4];
        metadata.extend(make_box(b"pitm", &[1, 0, 0, 0, 0, 1, 0, 2], true));
        let mut items = vec![1, 0, 0, 0, 0, 0, 0, 2];
        for id in [65_537u32, 65_538] {
            let mut body = vec![3, 0, 0, 0];
            body.extend_from_slice(&id.to_be_bytes());
            body.extend_from_slice(b"\0\0hvc1\0");
            items.extend(make_box(b"infe", &body, true));
        }
        metadata.extend(make_box(b"iinf", &items, true));
        let flags = primary_flags(&metadata).unwrap();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].1, 1);
        assert_eq!(
            &metadata[flags[0].0 + 1..flags[0].0 + 5],
            &65_537u32.to_be_bytes()
        );
        assert!(primary_flags(&metadata[..metadata.len() - 1]).is_err());
        metadata[20] = 2; // pitm 只接受版本 0/1。
        assert!(primary_flags(&metadata).is_err());
        assert!(box_size(b"\0\0\0\x04free", 8).is_err());
        assert!(box_size(b"\0\0\0\x01free", 8).is_err());
        assert_eq!(box_size(b"\0\0\0\0free", 24).unwrap(), (8, 24));
    }

    #[test]
    #[cfg(windows)]
    fn windows_decoder_uses_exe_from_path() {
        const CHILD: &str = "FLEQI_HEIF_LOOKUP_CHILD";
        if std::env::var_os(CHILD).is_some() {
            // 空 exe 用于区分 PATH 命中与解码器缺失，不依赖外部解码工具。
            let error = decode(Path::new("missing.heic"), &AtomicBool::new(false)).unwrap_err();
            assert!(error.contains("os error 193"), "{error}");
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("heif-convert.exe"), []).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "heif::tests::windows_decoder_uses_exe_from_path"])
            .env(CHILD, "1")
            .env("PATH", directory.path())
            .env("PATHEXT", ".EXE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
