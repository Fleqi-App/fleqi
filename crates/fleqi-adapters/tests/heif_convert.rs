//! Apple HEIF/HEIC 转为 PNG、JPEG、WebP，验证像素、尺寸与原件保留。

use fleqi_adapters::capabilities::FileCapabilities;
use fleqi_adapters::is_heif;
use image::GenericImageView;
use std::sync::atomic::AtomicBool;

// 固定样本由 Ubuntu 官方 libheif 1.21.2（x265）生成：
// 12×8 RGB [10, 180, 30]，(1, 2) 为 [255, 0, 0]；
// heif-enc -L -p chroma=444 -o heif-lossless-12x8.heic source.png。
// 测试运行只需要解码器，不再依赖现场编码工具。
const HEIC: &[u8] = include_bytes!("fixtures/heif-lossless-12x8.heic");

#[test]
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux")),
    ignore = "需要 PATH 中的 heif-convert；安装后使用 --ignored 执行真实解码验证"
)]
fn apple_heif_converts_to_png_jpeg_and_webp_without_removing_the_original() {
    let root = tempfile::tempdir().unwrap();
    let heic = root.path().join("photo.heic");
    std::fs::write(&heic, HEIC).unwrap();
    assert!(is_heif(HEIC));
    let files = FileCapabilities;

    let png = files
        .image_convert(&heic, root.path(), "png", 90)
        .expect("png");
    let decoded = image::open(&png).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (12, 8));
    let red = decoded.get_pixel(1, 2).0;
    assert!(
        red[0] >= 250 && red[1] <= 2 && red[2] <= 2,
        "红色像素被损坏：{red:?}"
    );
    let green = decoded.get_pixel(0, 0).0;
    assert!(
        green
            .iter()
            .zip([10, 180, 30])
            .all(|(actual, expected)| actual.abs_diff(expected) <= 2),
        "绿色像素被损坏：{green:?}"
    );

    let jpeg = files
        .image_convert_with_background(
            &heic,
            root.path(),
            "jpg",
            95,
            Some([255, 255, 255]),
            &AtomicBool::new(false),
        )
        .expect("jpeg");
    let jpeg_image = image::open(&jpeg).unwrap();
    assert_eq!(jpeg_image.dimensions(), (12, 8));
    let red = jpeg_image.to_rgb8().get_pixel(1, 2).0;
    assert!(red[0] > 200 && red[1] < 40 && red[2] < 40, "{red:?}");

    let webp = files
        .image_convert(&heic, root.path(), "webp", 90)
        .expect("webp");
    assert_eq!(image::open(&webp).unwrap().to_rgb8(), decoded);

    assert_eq!(std::fs::read(&heic).unwrap(), HEIC);
    assert!(png.extension().is_some_and(|ext| ext == "png"));
    assert!(jpeg.extension().is_some_and(|ext| ext == "jpg"));
    assert!(webp.extension().is_some_and(|ext| ext == "webp"));
}
