use fleqi_adapters::image_operations::{execute, load};
use image::{AnimationDecoder, GenericImageView, Rgba, RgbaImage};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn run(operation: &str, inputs: &[PathBuf], directory: &Path, pairs: &[(&str, &str)]) -> PathBuf {
    let values = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect::<BTreeMap<_, _>>();
    let result = execute(
        operation,
        inputs,
        directory,
        &values,
        &AtomicBool::new(false),
    )
    .unwrap();
    PathBuf::from(
        result
            .lines()
            .find_map(|line| line.strip_prefix("已生成："))
            .unwrap(),
    )
}
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.png");
    let mut image = RgbaImage::from_pixel(80, 40, Rgba([255, 0, 0, 255]));
    for x in 0..40 {
        for y in 0..40 {
            image.put_pixel(x, y, Rgba([0, 0, 255, 120]));
        }
    }
    image.save(&source).unwrap();
    (directory, source)
}
#[test]
fn transforms_preserve_alpha_dimensions_and_original() {
    let (dir, source) = fixture();
    let original = std::fs::read(&source).unwrap();
    let rotated = image::open(run(
        "image.rotate",
        std::slice::from_ref(&source),
        dir.path(),
        &[("angle", "90")],
    ))
    .unwrap();
    assert_eq!(rotated.dimensions(), (40, 80));
    assert!(rotated.to_rgba8().pixels().any(|pixel| pixel[3] == 120));
    let fixed = image::open(run(
        "image.resize",
        std::slice::from_ref(&source),
        dir.path(),
        &[("mode", "height"), ("height", "60"), ("upscale", "true")],
    ))
    .unwrap();
    assert_eq!(fixed.dimensions(), (120, 60));
    let thumbnail = image::open(run(
        "image.resize",
        std::slice::from_ref(&source),
        dir.path(),
        &[("mode", "box"), ("width", "20"), ("height", "20")],
    ))
    .unwrap();
    assert_eq!(thumbnail.dimensions(), (20, 10));
    let crop = image::open(run(
        "CAP-IMAGE-008",
        std::slice::from_ref(&source),
        dir.path(),
        &[("left", "3"), ("right", "7"), ("top", "2"), ("bottom", "8")],
    ))
    .unwrap();
    assert_eq!(crop.dimensions(), (70, 30));
    let transparent = image::open(run(
        "CAP-IMAGE-009",
        std::slice::from_ref(&source),
        dir.path(),
        &[("color", "#ff0000"), ("tolerance", "0")],
    ))
    .unwrap();
    assert_eq!(transparent.to_rgba8().get_pixel(70, 20)[3], 0);
    assert_eq!(transparent.to_rgba8().get_pixel(10, 20)[3], 120);
    assert_eq!(std::fs::read(source).unwrap(), original);
}
#[test]
fn blur_border_grid_and_tint_follow_parameters() {
    let (dir, source) = fixture();
    let border = image::open(run(
        "CAP-IMAGE-014",
        std::slice::from_ref(&source),
        dir.path(),
        &[("width", "5"), ("color", "#00ff00")],
    ))
    .unwrap();
    assert_eq!(border.dimensions(), (90, 50));
    assert_eq!(border.to_rgba8().get_pixel(0, 0), &Rgba([0, 255, 0, 255]));
    let blurred = image::open(run(
        "CAP-IMAGE-013",
        std::slice::from_ref(&source),
        dir.path(),
        &[("radius", "3")],
    ))
    .unwrap();
    assert_eq!(blurred.dimensions(), (80, 40));
    assert_ne!(
        blurred.to_rgba8().get_pixel(39, 20),
        image::open(&source).unwrap().to_rgba8().get_pixel(39, 20)
    );
    let tinted = image::open(run(
        "CAP-IMAGE-016",
        std::slice::from_ref(&source),
        dir.path(),
        &[("color", "#00ff00"), ("strength", "0.5")],
    ))
    .unwrap();
    assert_eq!(tinted.to_rgba8().get_pixel(10, 20)[3], 120);
    assert_eq!(
        tinted.to_rgba8().get_pixel(70, 20),
        &Rgba([128, 128, 0, 255])
    );
    let blue = dir.path().join("blue.png");
    RgbaImage::from_pixel(80, 40, Rgba([0, 0, 255, 255]))
        .save(&blue)
        .unwrap();
    let grid = image::open(run(
        "CAP-IMAGE-015",
        &[source, blue],
        dir.path(),
        &[
            ("columns", "3"),
            ("rows", "1"),
            ("cellSize", "40"),
            ("gap", "2"),
        ],
    ))
    .unwrap();
    assert_eq!(grid.dimensions(), (124, 40));
    assert_eq!(grid.to_rgba8().get_pixel(60, 20), &Rgba([0, 0, 255, 255]));
    assert_eq!(
        grid.to_rgba8().get_pixel(110, 20),
        &Rgba([255, 255, 255, 255])
    );
}
#[test]
fn icon_layers_and_animation_frames_are_real() {
    let (dir, source) = fixture();
    let ico = run(
        "CAP-IMAGE-011",
        std::slice::from_ref(&source),
        dir.path(),
        &[("sizes", "16,32,64")],
    );
    let bytes = std::fs::read(ico).unwrap();
    assert_eq!(&bytes[..6], &[0, 0, 1, 0, 3, 0]);
    assert_eq!([bytes[6], bytes[22], bytes[38]], [16, 32, 64]);
    let blue = dir.path().join("blue.png");
    RgbaImage::from_pixel(80, 40, Rgba([0, 0, 255, 255]))
        .save(&blue)
        .unwrap();
    let gif = run(
        "CAP-IMAGE-012",
        &[source, blue],
        dir.path(),
        &[("durationMs", "120"), ("loops", "2")],
    );
    let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
        std::fs::File::open(gif).unwrap(),
    ))
    .unwrap();
    let frames = decoder.into_frames().collect_frames().unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].delay().numer_denom_ms(), (120, 1));
    assert_ne!(
        frames[0].buffer().get_pixel(70, 20),
        frames[1].buffer().get_pixel(70, 20)
    );
}
#[test]
fn metadata_reencoding_and_supported_formats_decode_by_content() {
    let (dir, source) = fixture();
    let original = image::open(&source).unwrap();
    for format in [
        image::ImageFormat::Png,
        image::ImageFormat::Jpeg,
        image::ImageFormat::Tiff,
        image::ImageFormat::Gif,
        image::ImageFormat::WebP,
    ] {
        let path = dir.path().join(format!("sample.{format:?}"));
        if format == image::ImageFormat::Jpeg {
            original.to_rgb8().save_with_format(&path, format).unwrap();
        } else {
            original.save_with_format(&path, format).unwrap();
        }
        let decoded = load(&path, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.dimensions(), (80, 40));
    }
    let misleading = dir.path().join("actually-png.jpg");
    std::fs::copy(&source, &misleading).unwrap();
    assert_eq!(
        image::guess_format(&std::fs::read(&misleading).unwrap()).unwrap(),
        image::ImageFormat::Png
    );
    let stripped = run(
        "CAP-IMAGE-007",
        std::slice::from_ref(&source),
        dir.path(),
        &[],
    );
    assert_eq!(
        image::open(stripped).unwrap().to_rgba8(),
        original.to_rgba8()
    );
}
#[test]
#[cfg(target_os = "macos")]
fn icns_has_independent_encoded_layers() {
    let (dir, source) = fixture();
    let output = run("CAP-IMAGE-010", &[source], dir.path(), &[]);
    let bytes = std::fs::read(output).unwrap();
    assert_eq!(&bytes[..4], b"icns");
    assert_eq!(
        u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize,
        bytes.len()
    );
    let mut offset = 8;
    let mut count = 0;
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        assert!(length >= 8 && offset + length <= bytes.len());
        count += 1;
        offset += length;
    }
    assert!(count >= 5);
}
