#![cfg(target_os = "macos")]
use fleqi_adapters::{image_operations::tool, native_steps::NativeSteps};
use fleqi_application::{paths::PathRegistry, run_service::NativeStepPort};
use fleqi_domain::{
    context::PathKind,
    execution::{ExecutionStep, StepKind},
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

fn convert(
    source: &Path,
    operation: &str,
    format: &str,
    handling: &str,
    cancel: bool,
) -> Result<String, String> {
    let paths = Arc::new(PathRegistry::new());
    let input = paths.register(source, PathKind::File);
    let parameters = BTreeMap::from([
        ("format", format),
        ("quality", "23"),
        ("sourceHandling", handling),
        ("alphaPolicy", "reject"),
        ("background", "#ffffff"),
    ]);
    let step = ExecutionStep {
        script_runtime: None,
        kind: StepKind::Native,
        operation: operation.into(),
        executable_ref: None,
        script: None,
        args: vec![serde_json::to_string(&parameters).unwrap()],
        cwd_ref: None,
        env_refs: vec![],
        input_refs: vec![input.id],
        expected_outputs: vec![],
    };
    NativeSteps::new(paths)
        .execute(&step, source.parent().unwrap(), &AtomicBool::new(cancel))
        .map(|result| result.output)
}

#[test]
fn mp4_to_mov_preserves_or_trashes_only_after_verified_output() {
    let root = tempfile::tempdir().unwrap();
    let filename = format!(
        "fleqi-conversion-{}.mp4",
        root.path().file_name().unwrap().to_string_lossy()
    );
    let source = root.path().join(filename);
    let status = std::process::Command::new(tool("ffmpeg").unwrap())
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=32x32:d=0.2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success());
    let original = std::fs::read(&source).unwrap();
    let kept = convert(&source, "CAP-MEDIA-002", "mov", "keep", false).unwrap();
    assert!(kept.contains("已保留原文件"));
    assert_eq!(std::fs::read(&source).unwrap(), original);
    let removed = convert(&source, "CAP-MEDIA-002", "mov", "trashAfterSuccess", false).unwrap();
    assert!(removed.contains("原文件已移入回收站"));
    assert!(!source.exists());
    let output = root.path().join(format!(
        "{}-converted (1).mov",
        source.file_stem().unwrap().to_string_lossy()
    ));
    let probe = fleqi_adapters::media_operations::probe(&output, &AtomicBool::new(false)).unwrap();
    assert_eq!(probe["streams"][0]["codec_name"], "h264");
    let trashed = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".Trash")
        .join(source.file_name().unwrap());
    assert_eq!(std::fs::read(&trashed).unwrap(), original);
    // Restore only the exact disposable fixture, then TempDir cleans it up.
    std::fs::rename(&trashed, &source).unwrap();
}

#[test]
fn failed_and_cancelled_conversions_keep_originals() {
    let root = tempfile::tempdir().unwrap();
    let corrupt = root.path().join("invalid.mp4");
    std::fs::write(&corrupt, b"not video").unwrap();
    assert!(convert(&corrupt, "CAP-MEDIA-002", "mov", "trashAfterSuccess", false).is_err());
    assert_eq!(std::fs::read(&corrupt).unwrap(), b"not video");
    let image = root.path().join("source.png");
    image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 255]))
        .save(&image)
        .unwrap();
    assert!(convert(&image, "CAP-IMAGE-001", "jpg", "trashAfterSuccess", true).is_err());
    assert!(image.exists());
    assert!(convert(&image, "CAP-IMAGE-001", "jpg", "invalid-policy", false).is_err());
    assert!(image.exists());
}
