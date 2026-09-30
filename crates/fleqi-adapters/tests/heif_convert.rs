//! Apple HEIF/HEIC 转为 PNG、JPEG、WebP。样本由 libheif 无损编码，原件保留。

use fleqi_adapters::capabilities::FileCapabilities;
use fleqi_adapters::is_heif;
use image::{GenericImageView, ImageFormat, Rgb, RgbImage};
use std::path::Path;
use std::process::Command;

fn lossless_heic(directory: &Path, name: &str, image: &RgbImage) -> std::path::PathBuf {
    let png = directory.join(format!("{name}.png"));
    image.save_with_format(&png, ImageFormat::Png).unwrap();
    let heic = directory.join(format!("{name}.heic"));
    let output = Command::new("heif-enc")
        .args(["-L", "-p", "chroma=444", "-o"])
        .arg(&heic)
        .arg(&png)
        .output()
        .expect("heif-enc");
    assert!(
        output.status.success(),
        "heif-enc 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(&heic).unwrap();
    assert!(is_heif(&bytes), "生成的文件不是 HEIF");
    heic
}

#[test]
fn apple_heif_converts_to_png_jpeg_and_webp_without_removing_the_original() {
    let root = tempfile::tempdir().unwrap();
    let mut source_image = RgbImage::new(12, 8);
    for pixel in source_image.pixels_mut() {
        *pixel = Rgb([10, 180, 30]);
    }
    source_image.put_pixel(1, 2, Rgb([255, 0, 0]));
    let heic = lossless_heic(root.path(), "photo", &source_image);
    let original = std::fs::read(&heic).unwrap();
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
    assert_eq!(decoded.get_pixel(0, 0), &Rgb([10, 180, 30]));

    let jpeg = files
        .image_convert_with_background(&heic, root.path(), "jpg", 95, Some([255, 255, 255]))
        .expect("jpeg");
    let jpeg_image = image::open(&jpeg).unwrap();
    assert_eq!(jpeg_image.dimensions(), (12, 8));
    let red = jpeg_image.to_rgb8().get_pixel(1, 2).0;
    assert!(red[0] > 200 && red[1] < 40 && red[2] < 40, "{red:?}");

    let webp = files
        .image_convert(&heic, root.path(), "webp", 90)
        .expect("webp");
    assert_eq!(image::open(&webp).unwrap().dimensions(), (12, 8));

    assert_eq!(std::fs::read(&heic).unwrap(), original);
    assert!(png.extension().is_some_and(|ext| ext == "png"));
    assert!(jpeg.extension().is_some_and(|ext| ext == "jpg"));
    assert!(webp.extension().is_some_and(|ext| ext == "webp"));
}
