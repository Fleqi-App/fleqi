//! AC-CAP-031..135 扩展矩阵（docs/capabilities.md §扩展能力）。
//! 真实场景优先：执行器存在且条件满足 → 真实通过；
//! 执行器尚不存在 → gap（诚实缺口，不计通过）；
//! 需要特殊系统状态/外部服务/凭据 → cond（条件反馈验收：真实失败/引导而非伪成功）。
//! 结果逐项落盘 `tests/.artifacts/ac-matrix/extended.json`。

use fleqi_adapters::capabilities::FileCapabilities;
use fleqi_adapters::extended::ComputeCapabilities;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;

#[derive(Clone, serde::Serialize)]
struct Row {
    ac: String,
    verdict: String, // pass | gap | cond | fail
    note: String,
}

static ROWS: OnceLock<Mutex<Vec<Row>>> = OnceLock::new();
thread_local! {static CASE_FAILURES: std::cell::Cell<usize> = const {std::cell::Cell::new(0)};}

fn rows() -> &'static Mutex<Vec<Row>> {
    ROWS.get_or_init(|| Mutex::new(Vec::new()))
}

fn record(ac: &str, verdict: &str, note: &str) {
    if verdict == "fail" {
        CASE_FAILURES.with(|count| count.set(count.get() + 1));
    }
    let mut rows = rows().lock().unwrap();
    assert!(!rows.iter().any(|row| row.ac == ac), "重复验收编号 {ac}");
    rows.push(Row {
        ac: ac.to_owned(),
        verdict: verdict.to_owned(),
        note: note.to_owned(),
    });
    rows.sort_by(|a, b| a.ac.cmp(&b.ac));
    let directory = evidence_dir();
    std::fs::create_dir_all(&directory).expect("创建证据目录");
    let temporary = directory.join("extended.tmp.json");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&*rows).unwrap()).expect("写证据");
    std::fs::rename(temporary, directory.join("extended.json")).expect("发布证据");
    println!("MATRIX {ac} {verdict} {note}");
}

macro_rules! scenario {
    ($ac:expr, $body:block) => { scenario!(@run $ac,"pass",&format!("真实固定样本及结果断言通过；capabilities_matrix_extended.rs:{}",line!()),$body) };
    ($ac:expr, $note:expr, $body:block) => { scenario!(@run $ac,"cond",$note,$body) };
    (@run $ac:expr, $verdict:expr, $note:expr, $body:block) => {{
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let inner = || $body;
            let result: Result<(), Box<dyn std::error::Error>> = inner();
            result.map_err(|error| error.to_string())
        }));
        match result {
            Ok(Ok(())) => record($ac, $verdict, $note),
            Ok(Err(message)) => record($ac, "fail", &message),
            Err(panic) => {
                let message = panic.downcast_ref::<String>().map(String::as_str)
                    .or_else(||panic.downcast_ref::<&str>().copied()).unwrap_or("断言失败");
                record($ac,"fail",message);
            }
        }
    }};
}
struct MatrixBatch;
impl Drop for MatrixBatch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(
                CASE_FAILURES.with(|count| count.replace(0)),
                0,
                "此批矩阵存在真实失败，详见 extended.json"
            );
        }
    }
}

fn cond(ac: &str, note: &str) {
    record(ac, "cond", note);
}

fn evidence_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut root = manifest;
    while let Some(parent) = root.parent() {
        if parent.join("pnpm-workspace.yaml").exists() {
            root = parent;
            break;
        }
        root = parent;
    }
    root.join("tests").join(".artifacts").join("ac-matrix")
}

struct WorkDir(PathBuf);
impl std::ops::Deref for WorkDir {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}
impl AsRef<Path> for WorkDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn workdir(name: &str) -> WorkDir {
    let dir = std::env::temp_dir().join(format!(
        "fleqi-ext-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建工作目录");
    WorkDir(dir)
}

fn parameters(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn native_result(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    pairs: &[(&str, &str)],
) -> Result<fleqi_application::run_service::NativeOutput, String> {
    use fleqi_application::{paths::PathRegistry, run_service::NativeStepPort};
    use fleqi_domain::{
        context::PathKind,
        execution::{ExecutionStep, StepKind},
    };
    let registry = std::sync::Arc::new(PathRegistry::new());
    let refs = sources
        .iter()
        .map(|p| registry.register(p, PathKind::File).id)
        .collect();
    let step = ExecutionStep {
        kind: StepKind::Native,
        operation: operation.into(),
        executable_ref: None,
        script: None,
        args: vec![serde_json::to_string(&parameters(pairs)).unwrap()],
        cwd_ref: None,
        env_refs: vec![],
        input_refs: refs,
        expected_outputs: vec![],
    };
    fleqi_adapters::native_steps::NativeSteps::new(registry).execute(
        &step,
        cwd,
        &std::sync::atomic::AtomicBool::new(false),
    )
}
fn native(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    pairs: &[(&str, &str)],
) -> Result<String, String> {
    native_result(operation, sources, cwd, pairs).map(|result| result.output)
}
fn file_op(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    args: &[(&str, &str)],
) -> Result<fleqi_application::run_service::NativeOutput, String> {
    fleqi_adapters::file_operations::execute(
        operation,
        sources,
        cwd,
        &parameters(args),
        &std::sync::atomic::AtomicBool::new(false),
    )
}
fn system_op(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    args: &[(&str, &str)],
) -> Result<String, String> {
    fleqi_adapters::system_operations::execute(
        operation,
        sources,
        cwd,
        &parameters(args),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .map(|r| r.output)
}
fn generated(result: &str) -> PathBuf {
    PathBuf::from(
        result
            .lines()
            .find_map(|l| l.strip_prefix("已生成："))
            .expect("生成路径"),
    )
}
fn command(name: &str, args: &[&str]) -> String {
    let out = std::process::Command::new(name)
        .args(args)
        .output()
        .expect("运行真实工具");
    assert!(
        out.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}
fn media(
    operation: &str,
    source: &Path,
    cwd: &Path,
    args: &[(&str, &str)],
) -> Result<String, String> {
    fleqi_adapters::media_operations::execute(
        operation,
        &[source.to_owned()],
        cwd,
        &parameters(args),
        &std::sync::atomic::AtomicBool::new(false),
    )
}
fn image_run(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    args: &[(&str, &str)],
) -> Result<PathBuf, String> {
    fleqi_adapters::image_operations::execute(
        operation,
        sources,
        cwd,
        &parameters(args),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .map(|s| generated(&s))
}
fn add_exif(path: &Path, orientation: u8) {
    // JPEG APP1 containing independent TIFF IFD Orientation and Artist entries.
    let mut payload = b"Exif\0\0II\x2a\0\x08\0\0\0\x02\0".to_vec();
    payload.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, orientation, 0, 0, 0]);
    payload.extend_from_slice(&[0x3b, 0x01, 2, 0, 3, 0, 0, 0, b'Q', b'A', 0, 0]);
    payload.extend_from_slice(&[0; 4]);
    let original = std::fs::read(path).unwrap();
    let mut result = vec![0xff, 0xd8, 0xff, 0xe1];
    result.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    result.extend_from_slice(&payload);
    result.extend_from_slice(&original[2..]);
    std::fs::write(path, result).unwrap();
}
fn image_fixture(dir: &Path) -> PathBuf {
    let source = dir.join("source.png");
    let mut img = image::RgbaImage::from_pixel(80, 40, image::Rgba([255, 0, 0, 255]));
    for x in 0..40 {
        for y in 0..40 {
            img.put_pixel(x, y, image::Rgba([0, 0, 255, 120]));
        }
    }
    img.save(&source).unwrap();
    source
}
#[test]
fn matrix_media_info_and_speech() {
    let _batch = MatrixBatch;
    let dir = workdir("media");
    let source = dir.join("multi.mp4");
    command(
        "ffmpeg",
        &[
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x100:rate=10",
            "-f",
            "lavfi",
            "-i",
            "color=blue:size=80x60:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "2",
            "-map",
            "0:v",
            "-map",
            "1:v",
            "-map",
            "2:a",
            "-c:v:0",
            "libx264",
            "-g",
            "1",
            "-c:v:1",
            "mpeg4",
            "-c:a",
            "aac",
            source.to_str().unwrap(),
        ],
    );
    let probe: serde_json::Value = serde_json::from_str(&command(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
            source.to_str().unwrap(),
        ],
    ))
    .unwrap();
    scenario!("AC-CAP-031", {
        let info: serde_json::Value =
            serde_json::from_str(&media("CAP-MEDIA-005", &source, &dir, &[])?)?;
        let expected = probe["format"]["duration"]
            .as_str()
            .unwrap()
            .parse::<f64>()?;
        assert_eq!(info["durationSeconds"].as_f64().unwrap(), expected);
        assert_eq!(info["unknown"], false);
        Ok(())
    });
    scenario!("AC-CAP-032", {
        let rotated = dir.join("rotation.mp4");
        command(
            "ffmpeg",
            &[
                "-v",
                "error",
                "-display_rotation:v:0",
                "90",
                "-i",
                source.to_str().unwrap(),
                "-map",
                "0",
                "-c",
                "copy",
                rotated.to_str().unwrap(),
            ],
        );
        let a: serde_json::Value =
            serde_json::from_str(&media("CAP-MEDIA-006", &rotated, &dir, &[("track", "0")])?)?;
        let b: serde_json::Value =
            serde_json::from_str(&media("CAP-MEDIA-006", &rotated, &dir, &[("track", "1")])?)?;
        assert_eq!(
            (a["encodedWidth"].as_u64(), a["encodedHeight"].as_u64()),
            (Some(160), Some(100))
        );
        assert_eq!(
            (a["displayWidth"].as_u64(), a["displayHeight"].as_u64()),
            (Some(100), Some(160))
        );
        assert_eq!(
            (b["encodedWidth"].as_u64(), b["encodedHeight"].as_u64()),
            (Some(80), Some(60))
        );
        Ok(())
    });
    scenario!("AC-CAP-033", {
        for (scope, expected) in [
            ("container", &probe["format"]["bit_rate"]),
            ("stream", &probe["streams"][0]["bit_rate"]),
        ] {
            let info: serde_json::Value =
                serde_json::from_str(&media("CAP-MEDIA-007", &source, &dir, &[("scope", scope)])?)?;
            assert_eq!(
                info["bitsPerSecond"].as_f64(),
                expected.as_str().map(|v| v.parse::<f64>().unwrap())
            );
            assert_eq!(info["estimated"], false);
        }
        Ok(())
    });
    scenario!("AC-CAP-034", {
        for (track, codec) in [("0", "h264"), ("1", "mpeg4")] {
            let info: serde_json::Value =
                serde_json::from_str(&media("CAP-MEDIA-008", &source, &dir, &[("track", track)])?)?;
            assert_eq!(info["codec"], codec);
        }
        assert!(media("CAP-MEDIA-008", &source, &dir, &[("track", "2")]).is_err());
        Ok(())
    });
    for (ac, op) in [
        ("AC-CAP-035", "CAP-MEDIA-009"),
        ("AC-CAP-036", "CAP-MEDIA-010"),
        ("AC-CAP-037", "CAP-MEDIA-011"),
    ] {
        let missing = dir.join("missing-model.bin");
        let failure =
            media(op, &source, &dir, &[("model", missing.to_str().unwrap())]).unwrap_err();
        assert!(failure.contains("模型不存在"));
        cond(
            ac,
            "真实执行器拒绝不存在的模型，未产生转写文件；已知语音准确度与字幕播放仍需已安装 ASR 模型验收",
        );
    }
    scenario!("AC-CAP-038", {
        let result = native(
            "CAP-MEDIA-004",
            std::slice::from_ref(&source),
            &dir,
            &[("start", "0"), ("duration", "1"), ("mode", "copy")],
        )?;
        let target = generated(&result);
        let hashes = |p: &Path| {
            command(
                "ffprobe",
                &[
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_packets",
                    "-show_data_hash",
                    "sha256",
                    "-show_entries",
                    "packet=data_hash",
                    "-of",
                    "csv=p=0",
                    p.to_str().unwrap(),
                ],
            )
        };
        let original = hashes(&source);
        let cut = hashes(&target);
        assert!(!cut.trim().is_empty());
        assert!(
            cut.lines()
                .all(|line| original.lines().any(|old| old == line)),
            "流复制包字节未重编码"
        );
        assert!(
            result.contains("关键帧") && result.contains("duration"),
            "{result}"
        );
        Ok(())
    });
    scenario!("AC-CAP-039", {
        let out = generated(&media("CAP-MEDIA-012", &source, &dir, &[("angle", "90")])?);
        let p = fleqi_adapters::media_operations::probe(
            &out,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        assert_eq!(p["streams"][0]["width"], 100);
        assert_eq!(p["streams"][0]["height"], 160);
        assert!(
            p["streams"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["codec_type"] == "audio")
        );
        let video = p["streams"][0]["duration"]
            .as_str()
            .unwrap()
            .parse::<f64>()?;
        let audio = p["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["codec_type"] == "audio")
            .unwrap()["duration"]
            .as_str()
            .unwrap()
            .parse::<f64>()?;
        assert!((video - audio).abs() < 0.1);
        Ok(())
    });
    scenario!("AC-CAP-040", {
        let out = generated(&media(
            "CAP-MEDIA-013",
            &source,
            &dir,
            &[
                ("ratioWidth", "1"),
                ("ratioHeight", "1"),
                ("anchor", "topLeft"),
            ],
        )?);
        let p = fleqi_adapters::media_operations::video_info(
            &out,
            0,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        assert_eq!(
            (p["encodedWidth"].as_u64(), p["encodedHeight"].as_u64()),
            (Some(100), Some(100))
        );
        assert!(media("CAP-MEDIA-013", &source, &dir, &[("ratioWidth", "0")]).is_err());
        Ok(())
    });
}
#[test]
fn matrix_image_extensions() {
    let _batch = MatrixBatch;
    use image::{AnimationDecoder, GenericImageView};
    let dir = workdir("images");
    let source = image_fixture(&dir);
    let sources = std::slice::from_ref(&source);
    let original = std::fs::read(&source).unwrap();
    scenario!("AC-CAP-041", {
        let oriented = dir.join("oriented.jpg");
        image::open(&source)?.to_rgb8().save(&oriented)?;
        add_exif(&oriented, 6);
        let info: serde_json::Value =
            serde_json::from_str(&native("CAP-IMAGE-005", &[oriented], &dir, &[])?)?;
        assert_eq!(info["storedDimensions"], serde_json::json!([80, 40]));
        assert_eq!(info["displayDimensions"], serde_json::json!([40, 80]));
        Ok(())
    });
    scenario!(
        "AC-CAP-042",
        "真实英文/数字及空白图已验证；中文语言模型缺失错误准确，多语言识别准确度待安装 chi_sim 后验收",
        {
            let text = dir.join("ocr.png");
            command(
                "magick",
                &[
                    "-size",
                    "640x160",
                    "xc:white",
                    "-font",
                    "/System/Library/Fonts/Hiragino Sans GB.ttc",
                    "-pointsize",
                    "48",
                    "-fill",
                    "black",
                    "-annotate",
                    "+20+80",
                    "HELLO 1234",
                    text.to_str().unwrap(),
                ],
            );
            let result = native(
                "CAP-IMAGE-006",
                std::slice::from_ref(&text),
                &dir,
                &[("language", "eng")],
            )?;
            assert!(
                result.contains("HELLO") && result.contains("1234"),
                "{result}"
            );
            let languages = command("tesseract", &["--list-langs"]);
            if !languages.lines().any(|line| line == "chi_sim") {
                let error = native(
                    "CAP-IMAGE-006",
                    &[text],
                    &dir,
                    &[("language", "eng+chi_sim")],
                )
                .unwrap_err();
                assert!(error.contains("chi_sim"), "{error}");
            }
            let blank = dir.join("blank.png");
            image::RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]))
                .save(&blank)?;
            assert!(
                native("CAP-IMAGE-006", &[blank], &dir, &[("language", "eng")])?
                    .trim()
                    .is_empty()
            );
            Ok(())
        }
    );
    scenario!("AC-CAP-043", {
        let image = image::open(image_run(
            "image.rotate",
            sources,
            &dir,
            &[("angle", "90")],
        )?)?
        .to_rgba8();
        assert_eq!(image.dimensions(), (40, 80));
        assert_eq!(*image.get_pixel(20, 10), image::Rgba([0, 0, 255, 120]));
        assert_eq!(*image.get_pixel(20, 70), image::Rgba([255, 0, 0, 255]));
        Ok(())
    });
    scenario!("AC-CAP-044", {
        let image = image::open(image_run(
            "image.resize",
            sources,
            &dir,
            &[("mode", "height"), ("height", "63"), ("upscale", "true")],
        )?)?;
        assert_eq!(image.dimensions(), (126, 63));
        Ok(())
    });
    scenario!("AC-CAP-045", {
        let portrait = image_run("image.rotate", sources, &dir, &[("angle", "90")])?;
        for (src, expected) in [(&source, (20, 10)), (&portrait, (10, 20))] {
            let img = image::open(image_run(
                "image.resize",
                std::slice::from_ref(src),
                &dir,
                &[("mode", "box"), ("width", "20"), ("height", "20")],
            )?)?;
            assert_eq!(img.dimensions(), expected);
        }
        Ok(())
    });
    scenario!("AC-CAP-046", {
        let tagged = dir.join("tagged.jpg");
        image::open(&source)?.to_rgb8().save(&tagged)?;
        add_exif(&tagged, 1);
        assert!(std::fs::read(&tagged)?.windows(6).any(|w| w == b"Exif\0\0"));
        let output = image_run("CAP-IMAGE-007", &[tagged], &dir, &[])?;
        assert!(!std::fs::read(&output)?.windows(6).any(|w| w == b"Exif\0\0"));
        assert_eq!(image::open(output)?.dimensions(), (80, 40));
        Ok(())
    });
    scenario!("AC-CAP-047", {
        let input_dir = dir.join("batch");
        std::fs::create_dir_all(input_dir.join("nested"))?;
        for extension in ["jpg", "jpeg", "png", "heic", "tiff", "gif", "webp"] {
            let input = input_dir.join(format!("sample.{extension}"));
            command(
                "magick",
                &[source.to_str().unwrap(), input.to_str().unwrap()],
            );
        }
        let nested = input_dir.join("nested/outside.png");
        std::fs::copy(&source, &nested)?;
        let result = native(
            "CAP-IMAGE-002",
            std::slice::from_ref(&input_dir),
            &dir,
            &[
                ("mode", "width"),
                ("width", "32"),
                ("formats", "jpg,jpeg,png,heic,tiff,gif,webp"),
                ("recursive", "false"),
            ],
        )?;
        let outputs: Vec<_> = result
            .lines()
            .filter_map(|line| line.strip_prefix("已生成："))
            .map(PathBuf::from)
            .collect();
        assert_eq!(outputs.len(), 7, "目录内7格式必须逐项处理：{result}");
        for output in outputs {
            assert_eq!(image::open(output)?.dimensions(), (32, 16));
        }
        assert_eq!(std::fs::read(&nested)?, original);
        assert_eq!(std::fs::read(&source)?, original);
        assert_eq!(std::fs::read_dir(input_dir.join("nested"))?.count(), 1);
        Ok(())
    });
    scenario!("AC-CAP-048", {
        let misleading = dir.join("fake.jpg");
        std::fs::copy(&source, &misleading)?;
        let result = native("CAP-FILE-009", &[misleading], &dir, &[])?;
        assert!(result.to_lowercase().contains("png"), "{result}");
        Ok(())
    });
    scenario!("AC-CAP-049", {
        let img = image::open(image_run(
            "CAP-IMAGE-008",
            sources,
            &dir,
            &[("left", "3"), ("right", "7"), ("top", "2"), ("bottom", "8")],
        )?)?;
        assert_eq!(img.dimensions(), (70, 30));
        assert!(image_run("CAP-IMAGE-008", sources, &dir, &[("left", "90")]).is_err());
        Ok(())
    });
    scenario!("AC-CAP-050", {
        let img = image::open(image_run(
            "CAP-IMAGE-009",
            sources,
            &dir,
            &[("color", "#ff0000"), ("tolerance", "0")],
        )?)?
        .to_rgba8();
        assert_eq!(img.get_pixel(70, 20)[3], 0);
        assert_eq!(img.get_pixel(10, 20)[3], 120);
        let error = image_run(
            "CAP-IMAGE-009",
            sources,
            &dir,
            &[("color", "#ff0000"), ("format", "jpg")],
        )
        .unwrap_err();
        assert!(
            error.contains("透明") || error.contains("alpha"),
            "JPG缺透明替代策略必须明确失败：{error}"
        );
        Ok(())
    });
    scenario!("AC-CAP-051", {
        let output = image_run("CAP-IMAGE-010", sources, &dir, &[])?;
        let extracted = dir.join("readback.iconset");
        command(
            "iconutil",
            &[
                "-c",
                "iconset",
                output.to_str().unwrap(),
                "-o",
                extracted.to_str().unwrap(),
            ],
        );
        for (name, size) in [
            ("icon_16x16.png", 16),
            ("icon_32x32.png", 32),
            ("icon_128x128.png", 128),
            ("icon_256x256.png", 256),
            ("icon_512x512.png", 512),
        ] {
            assert_eq!(
                image::open(extracted.join(name))?.dimensions(),
                (size, size)
            );
        }
        assert_eq!(std::fs::read(&source)?, original);
        Ok(())
    });
    scenario!("AC-CAP-052", {
        let out = image_run("CAP-IMAGE-011", sources, &dir, &[("sizes", "16,32,64")])?;
        let bytes = std::fs::read(&out)?;
        assert_eq!(&bytes[..6], &[0, 0, 1, 0, 3, 0]);
        assert_eq!([bytes[6], bytes[22], bytes[38]], [16, 32, 64]);
        let decoded = image::open(out)?.to_rgba8();
        assert!(decoded.pixels().any(|p| p[3] < 255));
        Ok(())
    });
    scenario!("AC-CAP-053", {
        let blue = dir.join("blue.png");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([0, 0, 255, 255])).save(&blue)?;
        let output = image_run(
            "CAP-IMAGE-012",
            &[source.clone(), blue],
            &dir,
            &[("durationMs", "120"), ("loops", "2")],
        )?;
        let bytes = std::fs::read(&output)?;
        assert!(
            bytes.windows(5).any(|w| w == [3, 1, 2, 0, 0]),
            "GIF NETSCAPE repeat count"
        );
        let frames = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
            std::fs::File::open(output)?,
        ))?
        .into_frames()
        .collect_frames()?;
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].delay().numer_denom_ms(), (120, 1));
        assert_eq!(
            *frames[0].buffer().get_pixel(70, 20),
            image::Rgba([255, 0, 0, 255])
        );
        assert_eq!(
            *frames[1].buffer().get_pixel(70, 20),
            image::Rgba([0, 0, 255, 255])
        );
        Ok(())
    });
    scenario!("AC-CAP-054", {
        let a = image::open(image_run(
            "CAP-IMAGE-013",
            sources,
            &dir,
            &[("radius", "1")],
        )?)?
        .to_rgba8();
        let b = image::open(image_run(
            "CAP-IMAGE-013",
            sources,
            &dir,
            &[("radius", "5")],
        )?)?
        .to_rgba8();
        assert_eq!(a.dimensions(), (80, 40));
        assert_ne!(a.get_pixel(38, 20), b.get_pixel(38, 20));
        assert_eq!(std::fs::read(&source)?, original);
        Ok(())
    });
    scenario!("AC-CAP-055", {
        let img = image::open(image_run(
            "CAP-IMAGE-014",
            sources,
            &dir,
            &[("width", "5"), ("color", "#00ff00")],
        )?)?
        .to_rgba8();
        assert_eq!(img.dimensions(), (90, 50));
        assert_eq!(*img.get_pixel(0, 0), image::Rgba([0, 255, 0, 255]));
        Ok(())
    });
    scenario!("AC-CAP-056", {
        let blue = dir.join("grid-blue.png");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([0, 0, 255, 255])).save(&blue)?;
        let img = image::open(image_run(
            "CAP-IMAGE-015",
            &[source.clone(), blue],
            &dir,
            &[
                ("columns", "3"),
                ("rows", "1"),
                ("cellSize", "40"),
                ("gap", "2"),
            ],
        )?)?
        .to_rgba8();
        assert_eq!(img.dimensions(), (124, 40));
        assert_eq!(*img.get_pixel(60, 20), image::Rgba([0, 0, 255, 255]));
        assert_eq!(*img.get_pixel(110, 20), image::Rgba([255, 255, 255, 255]));
        Ok(())
    });
    scenario!("AC-CAP-057", {
        let a = image::open(image_run(
            "CAP-IMAGE-016",
            sources,
            &dir,
            &[("color", "#00ff00"), ("strength", "0.5")],
        )?)?
        .to_rgba8();
        let b = image::open(image_run(
            "CAP-IMAGE-016",
            sources,
            &dir,
            &[("color", "#0000ff"), ("strength", "1")],
        )?)?
        .to_rgba8();
        assert_eq!(a.dimensions(), (80, 40));
        assert_eq!(a.get_pixel(10, 20)[3], 120);
        assert_eq!(*a.get_pixel(70, 20), image::Rgba([128, 128, 0, 255]));
        assert_ne!(a.get_pixel(70, 20), b.get_pixel(70, 20));
        Ok(())
    });
    scenario!("AC-CAP-058", {
        for text in ["中文", "English"] {
            let output = image_run(
                "CAP-IMAGE-017",
                sources,
                &dir,
                &[("text", text), ("size", "16"), ("x", "0"), ("y", "0")],
            )?;
            assert_ne!(
                image::open(output)?.to_rgba8(),
                image::open(&source)?.to_rgba8()
            );
        }
        let error = image_run(
            "CAP-IMAGE-017",
            sources,
            &dir,
            &[("text", "missing"), ("font", "/nonexistent/font.ttf")],
        )
        .unwrap_err();
        assert!(error.contains("字体不存在"));
        Ok(())
    });
}

fn rich_documents(directory: &Path) -> Vec<PathBuf> {
    let plain = directory.join("known.txt");
    std::fs::write(&plain, "alpha beta 中文测试\n").unwrap();
    let mut documents = vec![];
    for extension in ["doc", "docx", "odt", "rtf", "rtfd"] {
        let output = directory.join(format!("known.{extension}"));
        command(
            "/usr/bin/textutil",
            &[
                "-convert",
                extension,
                "-output",
                output.to_str().unwrap(),
                plain.to_str().unwrap(),
            ],
        );
        documents.push(output);
    }
    let macro_doc = directory.join("known.docm");
    std::fs::copy(directory.join("known.docx"), &macro_doc).unwrap();
    documents.push(macro_doc);
    documents
}
#[test]
fn matrix_text_and_docs() {
    let _batch = MatrixBatch;
    let dir = workdir("documents");
    let documents = rich_documents(&dir);
    scenario!("AC-CAP-059", {
        use fleqi_adapters::document_operations::word_count;
        assert_eq!(word_count("hello world fleqi", "words")?, 3);
        assert_eq!(word_count("fleqi 中文测试", "cjk")?, 4);
        assert_eq!(word_count("fleqi 中文测试", "characters")?, 10);
        assert!(word_count("x", "word").is_err());
        Ok(())
    });
    scenario!("AC-CAP-060", {
        for path in &documents {
            let extracted = fleqi_adapters::document_operations::extract(
                path,
                &std::sync::atomic::AtomicBool::new(false),
            )?;
            assert_eq!(
                extracted.text.trim(),
                "alpha beta 中文测试",
                "{}",
                path.display()
            );
            assert_eq!(
                fleqi_adapters::document_operations::word_count(&extracted.text, "words")?,
                3
            );
            let result = native(
                "CAP-TEXT-008",
                std::slice::from_ref(path),
                &dir,
                &[("unit", "cjk")],
            )?;
            assert!(result.contains("4 cjk"), "{result}");
        }
        Ok(())
    });
    let plain = dir.join("known.txt");
    let original = std::fs::read(&plain).unwrap();
    let error = native("CAP-TEXT-009", std::slice::from_ref(&plain), &dir, &[]).unwrap_err();
    assert!(error.contains("摘要服务未配置"), "{error}");
    assert_eq!(std::fs::read(&plain).unwrap(), original);
    cond(
        "AC-CAP-061",
        "真实读取固定正文后返回摘要服务未配置，原文未变；关键事实摘要质量需用户配置模型端点验收",
    );
    for path in &documents {
        let error = native("CAP-TEXT-010", std::slice::from_ref(path), &dir, &[]).unwrap_err();
        assert!(error.contains("摘要服务未配置"), "{error}");
    }
    let broken = dir.join("broken.docx");
    std::fs::write(&broken, b"not docx").unwrap();
    assert!(native("CAP-TEXT-010", &[broken], &dir, &[]).is_err());
    cond(
        "AC-CAP-062",
        "六类真实文档正文已进入摘要前置读取，缺摘要服务及坏文档明确失败；模型端点生成内容仍待配置后验收",
    );
    scenario!("AC-CAP-063", {
        for path in &documents {
            let result = native(
                "CAP-TEXT-011",
                std::slice::from_ref(path),
                &dir,
                &[("output", "file")],
            )?;
            assert_eq!(
                std::fs::read_to_string(generated(&result))?.trim(),
                "alpha beta 中文测试"
            );
            assert!(
                result.contains("图片") || result.contains("主文档"),
                "范围明示：{result}"
            );
        }
        Ok(())
    });
    // Opening an editor changes desktop state; validate missing-app feedback only.
    let error = native(
        "CAP-SYSTEM-001",
        &[plain],
        &dir,
        &[("editor", "FleqiMatrixDefinitelyMissingEditor")],
    );
    match error {
        Err(error) => {
            assert!(!error.contains("未登记"), "{error}");
            cond(
                "AC-CAP-064",
                &format!(
                    "不存在的编辑器返回真实错误：{error}；默认及已安装编辑器打开正确路径仍需桌面验收"
                ),
            );
        }
        Ok(_) => panic!("不存在的编辑器不能报告打开成功"),
    }
}

fn test_pdf(
    directory: &Path,
    name: &str,
    pages: u32,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = FileCapabilities::new().generate_test_pdf(directory, name, pages)?;
    let mut doc = lopdf::Document::load(&path)?;
    let font=doc.add_object(lopdf::dictionary!{"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica","Encoding"=>"WinAnsiEncoding"});
    let resources = doc.add_object(lopdf::dictionary! {"Font"=>lopdf::dictionary!{"F1"=>font}});
    for (number, id) in doc.get_pages() {
        let stream = doc.add_object(lopdf::Stream::new(
            lopdf::dictionary! {},
            format!("BT /F1 12 Tf 72 720 Td (Page {number}) Tj ET").into_bytes(),
        ));
        let page = doc.get_dictionary_mut(id)?;
        page.set("Resources", resources);
        page.set("Contents", stream);
    }
    doc.save(&path)?;
    Ok(path)
}
fn qpdf_query(
    path: &Path,
    password: &str,
    option: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut child = std::process::Command::new("qpdf")
        .arg("--password-file=-")
        .arg(option)
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{password}\n").as_bytes())?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "独立 qpdf 检查失败，凭据不进入日志"
    );
    Ok(String::from_utf8(output.stdout)?)
}
fn pdf(
    operation: &str,
    source: &Path,
    cwd: &Path,
    args: &[(&str, &str)],
) -> Result<String, String> {
    fleqi_adapters::pdf_operations::execute(
        operation,
        &[source.to_owned()],
        cwd,
        &parameters(args),
        &std::sync::atomic::AtomicBool::new(false),
    )
}
fn image_pdf(source: &Path) {
    use lopdf::{Object, Stream, dictionary};
    let mut doc = lopdf::Document::load(source).unwrap();
    let image=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>2,"Height"=>1,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8},vec![255,0,0,0,0,255]));
    let resources = doc.add_object(dictionary! {"XObject"=>dictionary!{"Im0"=>image}});
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 200 0 0 100 10 10 cm /Im0 Do Q".to_vec(),
    ));
    let page = *doc.get_pages().values().next().unwrap();
    let page = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    page.set("Resources", resources);
    page.set("Contents", Object::Reference(content));
    doc.save(source).unwrap();
}
struct Secrets(Vec<String>);
impl Secrets {
    fn new() -> Self {
        Self(vec![])
    }
    fn add(&mut self, value: &str) -> String {
        let key = fleqi_application::secrets::store(value.into());
        self.0.push(key.clone());
        key
    }
}
impl Drop for Secrets {
    fn drop(&mut self) {
        for key in &self.0 {
            fleqi_application::secrets::release(key);
        }
    }
}
#[test]
fn matrix_pdf_matrix() {
    let _batch = MatrixBatch;
    let file_caps = FileCapabilities::new();
    scenario!("AC-CAP-065", {
        let dir = workdir("065");
        let a = file_caps.create_text_file(&dir, "note.txt", "v1 中文", "utf-8")?;
        let b = file_caps.create_text_file(&dir, "note.txt", "v2", "utf-8")?;
        assert_ne!(a, b);
        assert_eq!(std::fs::read_to_string(a)?, "v1 中文");
        assert_eq!(std::fs::read_to_string(b)?, "v2");
        Ok(())
    });
    scenario!("AC-CAP-066", {
        let dir = workdir("066");
        for pages in [1, 4, 7] {
            let source = test_pdf(&dir, &format!("{pages}.pdf"), pages)?;
            let result: serde_json::Value =
                serde_json::from_str(&pdf("CAP-PDF-006", &source, &dir, &[])?)?;
            assert_eq!(result["pages"], pages);
        }
        let invalid = dir.join("invalid.pdf");
        std::fs::write(&invalid, b"not a pdf")?;
        assert!(pdf("CAP-PDF-006", &invalid, &dir, &[]).is_err());
        Ok(())
    });
    scenario!("AC-CAP-067", {
        let dir = workdir("067");
        let source = test_pdf(&dir, "source.pdf", 1)?;
        let empty: serde_json::Value =
            serde_json::from_str(&pdf("CAP-PDF-007", &source, &dir, &[])?)?;
        assert_eq!(empty["author"], serde_json::Value::Null);
        assert_eq!(empty["notSet"], true);
        let mut doc = lopdf::Document::load(&source)?;
        let info = doc.add_object(
            lopdf::dictionary! {"Author"=>lopdf::Object::string_literal("Known author")},
        );
        doc.trailer.set("Info", info);
        doc.save(&source)?;
        let author: serde_json::Value =
            serde_json::from_str(&pdf("CAP-PDF-007", &source, &dir, &[])?)?;
        assert_eq!(author["author"], "Known author");
        Ok(())
    });
    {
        let dir = workdir("068");
        let source = test_pdf(&dir, "text.pdf", 2).unwrap();
        let error = native(
            "CAP-PDF-008",
            std::slice::from_ref(&source),
            &dir,
            &[("pages", "2"), ("ocr", "never")],
        )
        .unwrap_err();
        assert!(error.contains("摘要服务未配置"), "{error}");
        let content = fleqi_adapters::pdf_operations::text_for_summary(
            &source,
            "",
            "2",
            false,
            "eng",
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        assert!(content.text.contains("Page 2") && !content.text.contains("Page 1"));
        let scanned = test_pdf(&dir, "scan.pdf", 1).unwrap();
        image_pdf(&scanned);
        let skipped = fleqi_adapters::pdf_operations::text_for_summary(
            &scanned,
            "",
            "",
            false,
            "eng",
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        assert!(skipped.partial && skipped.text.is_empty());
        assert!(skipped.scope.contains("[1]"));
        cond(
            "AC-CAP-068",
            "真实文本 PDF 页范围及扫描页关闭 OCR 的不完整范围已验证；缺摘要服务明确失败，扫描 OCR 质量与模型摘要仍待专门样本/端点验收",
        );
    }
    scenario!("AC-CAP-069", {
        let dir = workdir("069");
        let one = test_pdf(&dir, "one.pdf", 1)?;
        let two = test_pdf(&dir, "two.pdf", 2)?;
        let a = file_caps.pdf_merge(&[one.clone(), two.clone()], &dir, "a.pdf")?;
        let b = file_caps.pdf_merge(&[two, one], &dir, "b.pdf")?;
        let da = lopdf::Document::load(a)?;
        let db = lopdf::Document::load(b)?;
        assert_eq!(da.get_pages().len(), 3);
        assert_eq!(db.get_pages().len(), 3);
        assert!(
            da.extract_text(&[2])?.contains("Page 1"),
            "实际正文 {:?}",
            da.extract_text(&[2])?
        );
        assert!(db.extract_text(&[2])?.contains("Page 2"));
        Ok(())
    });
    scenario!("AC-CAP-070", {
        let dir = workdir("070");
        let source = test_pdf(&dir, "source.pdf", 3)?;
        let before = std::fs::metadata(&source)?.len();
        let (out, original, compressed) = file_caps.pdf_compress(&source, &dir, "optimized.pdf")?;
        assert_eq!(original, before);
        assert_eq!(compressed, std::fs::metadata(&out)?.len());
        assert_eq!(file_caps.pdf_page_count(&out)?, 3);
        assert_eq!(
            lopdf::Document::load(source)?.extract_text(&[1, 2, 3])?,
            lopdf::Document::load(out)?.extract_text(&[1, 2, 3])?
        );
        Ok(())
    });
    scenario!("AC-CAP-071", {
        let dir = workdir("071");
        let source = test_pdf(&dir, "source.pdf", 5)?;
        let parts = file_caps.pdf_split_every(&source, &dir, 1)?;
        assert_eq!(parts.len(), 5);
        for (index, part) in parts.iter().enumerate() {
            let doc = lopdf::Document::load(part)?;
            assert_eq!(doc.get_pages().len(), 1);
            assert!(
                doc.extract_text(&[1])?
                    .contains(&format!("Page {}", index + 1))
            );
        }
        Ok(())
    });
    scenario!("AC-CAP-072", {
        let dir = workdir("072");
        let source = test_pdf(&dir, "source.pdf", 1)?;
        assert!(pdf("CAP-PDF-009", &source, &dir, &[])?.contains("没有嵌入图片"));
        image_pdf(&source);
        let result = pdf(
            "CAP-PDF-009",
            &source,
            &dir,
            &[("format", "png"), ("firstPage", "1"), ("lastPage", "1")],
        )?;
        let paths: Vec<_> = result.lines().filter(|l| Path::new(l).is_file()).collect();
        assert_eq!(paths.len(), 1);
        let image = image::open(paths[0])?.to_rgb8();
        assert_eq!(image.dimensions(), (2, 1));
        assert_eq!(*image.get_pixel(0, 0), image::Rgb([255, 0, 0]));
        assert_eq!(*image.get_pixel(1, 0), image::Rgb([0, 0, 255]));
        Ok(())
    });
    scenario!("AC-CAP-073", {
        let dir = workdir("073");
        let source = test_pdf(&dir, "source.pdf", 2)?;
        let mut doc = lopdf::Document::load(&source)?;
        let info = doc
            .add_object(lopdf::dictionary! {"Author"=>lopdf::Object::string_literal("Remove me")});
        doc.trailer.set("Info", info);
        let text = doc.extract_text(&[1, 2])?;
        doc.save(&source)?;
        let out = generated(&pdf("CAP-PDF-010", &source, &dir, &[("scope", "all")])?);
        let cleaned = lopdf::Document::load(out)?;
        assert!(cleaned.trailer.get(b"Info").is_err());
        assert_eq!(cleaned.get_pages().len(), 2);
        assert_eq!(cleaned.extract_text(&[1, 2])?, text);
        assert!(lopdf::Document::load(source)?.trailer.get(b"Info").is_ok());
        Ok(())
    });
    scenario!("AC-CAP-074", {
        let dir = workdir("074");
        let source = test_pdf(&dir, "source.pdf", 6)?;
        let out = file_caps.pdf_extract_pages(&source, &[5, 2], &dir, "pick.pdf")?;
        let doc = lopdf::Document::load(out)?;
        assert_eq!(doc.get_pages().len(), 2);
        assert!(
            doc.extract_text(&[1])?.contains("Page 5"),
            "实际正文 {:?}",
            doc.extract_text(&[1])?
        );
        assert!(doc.extract_text(&[2])?.contains("Page 2"));
        Ok(())
    });
    scenario!("AC-CAP-075", {
        let dir = workdir("075");
        let source = test_pdf(&dir, "source.pdf", 4)?;
        let out = file_caps.pdf_rotate_pages(&source, &[1, 3], 180, &dir, "rot.pdf")?;
        let doc = lopdf::Document::load(out)?;
        for (index, id) in doc.get_pages() {
            let rotation = doc
                .get_dictionary(id)?
                .get(b"Rotate")
                .ok()
                .and_then(|v| v.as_i64().ok())
                .unwrap_or(0);
            assert_eq!(rotation, if index == 1 || index == 3 { 180 } else { 0 });
        }
        Ok(())
    });
    scenario!("AC-CAP-076", {
        let dir = workdir("076");
        let source = test_pdf(&dir, "plain.pdf", 2)?;
        let mut secrets = Secrets::new();
        let user = secrets.add("matrix user password");
        let owner = secrets.add("matrix owner password");
        let wrong = secrets.add("matrix wrong password");
        let encrypted = generated(&pdf(
            "CAP-PDF-012",
            &source,
            &dir,
            &[("password", &user), ("ownerPassword", &owner)],
        )?);
        let error = pdf("CAP-PDF-011", &encrypted, &dir, &[("password", &wrong)]).unwrap_err();
        assert!(!error.contains("matrix wrong password"));
        let output = pdf("CAP-PDF-011", &encrypted, &dir, &[("password", &user)])?;
        assert!(!output.contains("matrix user password"));
        let doc = lopdf::Document::load(generated(&output))?;
        assert!(!doc.is_encrypted());
        assert_eq!(doc.get_pages().len(), 2);
        assert!(doc.extract_text(&[2])?.contains("Page 2"));
        Ok(())
    });
    scenario!("AC-CAP-077", {
        let dir = workdir("077");
        let source = test_pdf(&dir, "plain.pdf", 1)?;
        let mut secrets = Secrets::new();
        let user = secrets.add("matrix open secret");
        let owner = secrets.add("matrix admin secret");
        let output = pdf(
            "CAP-PDF-012",
            &source,
            &dir,
            &[
                ("password", &user),
                ("ownerPassword", &owner),
                ("printing", "none"),
                ("allowExtract", "false"),
            ],
        )?;
        assert!(!output.contains("matrix open secret"));
        let encrypted = generated(&output);
        assert!(lopdf::Document::load(&encrypted)?.is_encrypted());
        let without = std::process::Command::new("qpdf")
            .args(["--show-npages"])
            .arg(&encrypted)
            .output()?;
        assert!(!without.status.success());
        assert_eq!(
            qpdf_query(&encrypted, "matrix open secret", "--show-npages")?.trim(),
            "1"
        );
        let permissions = qpdf_query(&encrypted, "matrix open secret", "--show-encryption")?;
        assert!(permissions.contains("extract for any purpose: not allowed"));
        assert!(permissions.contains("print low resolution: not allowed"));
        assert!(permissions.contains("print high resolution: not allowed"));
        assert!(pdf("CAP-PDF-006", &encrypted, &dir, &[]).is_err());
        Ok(())
    });
}

#[test]
fn matrix_zip_and_metadata() {
    let _batch = MatrixBatch;
    let caps = FileCapabilities::new();
    scenario!("AC-CAP-078", {
        use sha2::{Digest, Sha256};
        let dir = workdir("078");
        let mut sources = vec![];
        for name in ["空 格.txt", "中文#2.txt", "-lead.txt"] {
            let p = dir.join(name);
            std::fs::write(&p, format!("content-{name}"))?;
            sources.push(p);
        }
        let zip = dir.join("odd.zip");
        caps.zip_create(&sources, &zip)?;
        let out = dir.join("out");
        caps.zip_extract(&zip, &out)?;
        for p in sources {
            assert_eq!(
                Sha256::digest(std::fs::read(&p)?),
                Sha256::digest(std::fs::read(out.join(p.file_name().unwrap()))?)
            );
        }
        Ok(())
    });
    scenario!("AC-CAP-079", {
        use std::io::Write;
        let dir = workdir("079");
        let zip = dir.join("mixed.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip)?);
        let options: zip::write::SimpleFileOptions = zip::write::FileOptions::default();
        writer.start_file("good.txt", options)?;
        writer.write_all(b"good")?;
        writer.start_file("../escape.txt", options)?;
        writer.write_all(b"bad")?;
        writer.finish()?;
        let report = caps.zip_extract(&zip, &dir.join("out"))?;
        assert_eq!(report.succeeded, 1);
        assert!(!dir.join("escape.txt").exists());
        assert_eq!(std::fs::read(dir.join("out/good.txt"))?, b"good");
        let broken = dir.join("bad.zip");
        std::fs::write(&broken, b"PK invalid")?;
        assert!(caps.zip_extract(&broken, &dir.join("bad-out")).is_err());
        Ok(())
    });
    scenario!("AC-CAP-080", {
        let dir = workdir("080");
        let folder = dir.join("d");
        std::fs::create_dir_all(folder.join("nested"))?;
        std::fs::write(folder.join("f.txt"), b"x")?;
        std::fs::write(folder.join("nested/g.txt"), b"y")?;
        let zip = dir.join("a.zip");
        caps.zip_create(&[folder], &zip)?;
        let names: Vec<_> = caps.zip_list(&zip)?.into_iter().map(|e| e.name).collect();
        assert!(names.iter().any(|n| n.ends_with("/f.txt")));
        assert!(names.iter().any(|n| n.ends_with("/nested/g.txt")));
        assert!(names.iter().any(|n| n.ends_with("/nested/")));
        Ok(())
    });
    scenario!("AC-CAP-081", {
        let dir = workdir("081");
        let input = dir.join("repeat.bin");
        std::fs::write(&input, vec![0; 4096])?;
        let empty = dir.join("empty.txt");
        std::fs::write(&empty, b"")?;
        let zip = dir.join("a.zip");
        caps.zip_create(&[input, empty], &zip)?;
        let result = native(
            "CAP-ZIP-004",
            std::slice::from_ref(&zip),
            &dir,
            &[("scope", "entries"), ("precision", "2")],
        )?;
        let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
        let compressed = archive.by_name("repeat.bin")?.compressed_size();
        let expected = (1.0 - compressed as f64 / 4096.0) * 100.0;
        assert!(result.contains("4096"));
        assert!(result.contains(&format!("{expected:.2}")), "{result}");
        assert!(result.contains("分母为 0"), "{result}");
        Ok(())
    });
    scenario!("AC-CAP-082", {
        let dir = workdir("082");
        let input = dir.join("a.txt");
        std::fs::write(&input, b"unique bytes")?;
        let result = native(
            "CAP-ZIP-005",
            std::slice::from_ref(&input),
            &dir,
            &[
                ("destination", "collected"),
                ("name", "a.zip"),
                ("sourceIntent", "move"),
            ],
        )?;
        assert!(result.contains("阶段 1") && result.contains("阶段 2"));
        assert!(!input.exists());
        assert_eq!(std::fs::read(dir.join("collected/a.txt"))?, b"unique bytes");
        assert!(dir.join("collected/a.zip").exists());
        let tree = dir.join("tree");
        std::fs::create_dir(&tree)?;
        std::fs::write(tree.join("only.txt"), b"keep")?;
        std::os::unix::fs::symlink("only.txt", tree.join("link"))?;
        let failure = native(
            "CAP-ZIP-005",
            std::slice::from_ref(&tree),
            &dir,
            &[("destination", "failed-stage"), ("sourceIntent", "move")],
        )
        .unwrap_err();
        assert!(
            failure.contains("阶段 2") && failure.contains("压缩失败"),
            "{failure}"
        );
        assert_eq!(
            std::fs::read(dir.join("failed-stage/tree/only.txt"))?,
            b"keep"
        );
        assert!(!dir.join("failed-stage/archive.zip").exists());
        Ok(())
    });
    scenario!("AC-CAP-083", {
        use std::os::unix::fs::MetadataExt;
        let dir = workdir("083");
        let sparse = dir.join("sparse");
        std::fs::File::create(&sparse)?.set_len(16 * 1024 * 1024)?;
        let meta = std::fs::metadata(&sparse)?;
        assert!(meta.blocks() * 512 < meta.len());
        let result = native("CAP-FILE-010", &[sparse], &dir, &[("unit", "bytes")])?;
        assert!(
            result.contains(&format!("逻辑大小 {}", meta.len())),
            "{result}"
        );
        assert!(
            result.contains(&format!("占用空间 {}", meta.blocks() * 512)),
            "{result}"
        );
        Ok(())
    });
    scenario!("AC-CAP-084", {
        use std::os::unix::fs::PermissionsExt;
        let dir = workdir("084");
        std::fs::create_dir_all(dir.join("blocked"))?;
        std::fs::write(dir.join("a"), vec![0; 512])?;
        std::fs::write(dir.join("blocked/secret"), vec![0; 256])?;
        std::os::unix::fs::symlink(&*dir, dir.join("loop"))?;
        std::fs::set_permissions(dir.join("blocked"), std::fs::Permissions::from_mode(0o0))?;
        let result = native_result("CAP-FILE-011", &[], &dir, &[("recursive", "true")]);
        std::fs::set_permissions(dir.join("blocked"), std::fs::Permissions::from_mode(0o700))?;
        let result = result?;
        assert!(result.partial, "权限不足必须部分结果：{}", result.output);
        assert!(result.output.contains("blocked"));
        assert!(result.output.contains("512"));
        Ok(())
    });
    cond(
        "AC-CAP-085",
        "Finder 目标与 PTY 已应用 cwd 的同步/忙碌安全点依赖宿主观察端口；本矩阵不冒充原生联动验收，见 M2 原生合同证据",
    );
    scenario!("AC-CAP-086", {
        let dir = workdir("086");
        for (name, size) in [("small.bin", 100), ("large.bin", 9000), ("mid.bin", 2000)] {
            std::fs::write(dir.join(name), vec![0; size])?;
        }
        let result = native("CAP-FILE-013", &[], &dir, &[("limit", "2")])?;
        assert!(result.find("large.bin").unwrap() < result.find("mid.bin").unwrap());
        assert!(!result.contains("small.bin"));
        Ok(())
    });
    scenario!("AC-CAP-087", {
        let dir = workdir("087");
        std::fs::create_dir(dir.join("sub"))?;
        std::fs::write(dir.join("x.bin"), b"same")?;
        std::fs::write(dir.join("copy.bin"), b"same")?;
        std::fs::write(dir.join("sub/x.bin"), b"different")?;
        let result = native("CAP-FILE-014", &[], &dir, &[])?;
        assert!(result.contains("共 1 组"));
        assert!(result.contains("copy.bin"));
        assert!(!result.contains("sub/x.bin"));
        assert!(dir.join("x.bin").exists() && dir.join("copy.bin").exists());
        Ok(())
    });
    scenario!("AC-CAP-088", {
        let dir = workdir("088");
        let f1 = dir.join("t1.txt");
        let f2 = dir.join("t2.txt");
        std::fs::write(&f1, b"1")?;
        std::fs::write(&f2, b"2")?;
        let report = caps.trash(vec![f1.clone(), f2.clone()]);
        assert_eq!(report.succeeded, 2);
        assert_eq!(report.restore_paths.len(), 2);
        caps.restore_from_trash(&report.restore_paths[0], &f1)?;
        caps.restore_from_trash(&report.restore_paths[1], &f2)?;
        assert_eq!(std::fs::read(f1)?, b"1");
        assert_eq!(std::fs::read(f2)?, b"2");
        Ok(())
    });
    scenario!(
        "AC-CAP-089",
        "真实按类型整理及目录外保留已验证；自然语言到分类规则需已配置模型端点单独验收",
        {
            let dir = workdir("089");
            std::fs::write(dir.join("a.txt"), b"a")?;
            std::fs::create_dir(dir.join("untouched"))?;
            std::fs::write(dir.join("untouched/b.txt"), b"b")?;
            let plan = caps.organize_plan(&dir, "type", None)?;
            caps.organize_apply(&plan)?;
            assert!(!dir.join("a.txt").exists());
            assert_eq!(std::fs::read(dir.join("untouched/b.txt"))?, b"b");
            Ok(())
        }
    );
    scenario!("AC-CAP-090", {
        let dir = workdir("090");
        for name in ["m1.txt", "m2.txt"] {
            std::fs::write(dir.join(name), name)?;
        }
        let target = dir.join("参数 目标");
        std::fs::create_dir(&target)?;
        let result = caps.move_entries(vec![dir.join("m1.txt"), dir.join("m2.txt")], &target)?;
        assert_eq!(result.succeeded, 2);
        assert_eq!(std::fs::read_to_string(target.join("m2.txt"))?, "m2.txt");
        let cyclic = caps.move_entries(vec![target.clone()], &target);
        assert!(cyclic.is_err() || cyclic.unwrap().succeeded == 0);
        Ok(())
    });
}

#[test]
fn matrix_files_rename_and_attributes() {
    let _batch = MatrixBatch;
    scenario!("AC-CAP-091", {
        let dir = workdir("091");
        let disguised = dir.join("image.jpg");
        std::fs::copy(image_fixture(&dir), &disguised)?;
        let text = dir.join("text.bin");
        std::fs::write(&text, "known text")?;
        let binary = dir.join("binary.txt");
        std::fs::write(&binary, [0u8, 255, 0, 128])?;
        for (path, expected) in [
            (disguised, "image/png"),
            (text, "text/plain"),
            (binary, "application/octet-stream"),
        ] {
            let result = file_op("CAP-FILE-009", &[path], &dir, &[])?;
            assert!(!result.partial);
            assert!(result.output.contains(expected), "{}", result.output);
        }
        Ok(())
    });

    scenario!("AC-CAP-092", {
        let dir = workdir("092");
        let source = dir.join("download.txt");
        std::fs::write(&source, b"x")?;
        assert!(
            file_op("CAP-FILE-015", std::slice::from_ref(&source), &dir, &[])?
                .output
                .contains("未记录")
        );
        let plist = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><array><string>https://example.test/origin</string></array></plist>";
        command(
            "/usr/bin/xattr",
            &[
                "-w",
                "com.apple.metadata:kMDItemWhereFroms",
                plist,
                source.to_str().unwrap(),
            ],
        );
        let result = file_op("CAP-FILE-015", &[source], &dir, &[])?;
        assert!(!result.partial);
        assert!(result.output.contains("https://example.test/origin"));
        Ok(())
    });

    scenario!("AC-CAP-093", {
        let dir = workdir("093");
        for (name, timezone, expected) in [
            ("archive.tar.gz", "-01:00", "archive-1970-01-01.tar.gz"),
            ("README", "+08:00", "README-1970-01-02"),
        ] {
            let source = dir.join(name);
            std::fs::write(&source, b"preserve")?;
            std::fs::OpenOptions::new()
                .write(true)
                .open(&source)?
                .set_times(
                    std::fs::FileTimes::new().set_modified(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(86400),
                    ),
                )?;
            let result = file_op(
                "CAP-FILE-005",
                &[source],
                &dir,
                &[
                    ("mode", "date"),
                    ("dateSource", "modified"),
                    ("timezone", timezone),
                    ("extensionRule", "all"),
                ],
            )?;
            assert!(!result.partial);
            assert!(result.output.contains(expected));
            assert_eq!(std::fs::read(dir.join(expected))?, b"preserve");
        }
        Ok(())
    });
    scenario!("AC-CAP-094", {
        for (sort, width, expected) in [
            ("nameAsc", "2", ["alpha-02.txt", "beta-05.txt"]),
            ("nameDesc", "4", ["beta-0002.txt", "alpha-0005.txt"]),
        ] {
            let dir = workdir("094");
            let a = dir.join("alpha.txt");
            let b = dir.join("beta.txt");
            std::fs::write(&a, b"a")?;
            std::fs::write(&b, b"b")?;
            let result = file_op(
                "CAP-FILE-006",
                &[b, a],
                &dir,
                &[
                    ("sort", sort),
                    ("width", width),
                    ("position", "suffix"),
                    ("start", "2"),
                    ("step", "3"),
                ],
            )?;
            assert!(!result.partial);
            for name in expected {
                assert!(dir.join(name).exists(), "{name}");
            }
        }
        Ok(())
    });
    scenario!("AC-CAP-095", {
        let dir = workdir("095");
        let source = dir.join("document.tar.gz");
        std::fs::write(&source, b"contents")?;
        let result = file_op(
            "CAP-FILE-005",
            &[source],
            &dir,
            &[
                ("mode", "affix"),
                ("suffix", "_归档"),
                ("extensionRule", "all"),
                ("position", "beforeExtension"),
            ],
        )?;
        assert!(!result.partial);
        assert_eq!(
            std::fs::read(dir.join("document_归档.tar.gz"))?,
            b"contents"
        );
        Ok(())
    });
    scenario!("AC-CAP-096", {
        let dir = workdir("096");
        let source = dir.join("ABC.TXT");
        std::fs::write(&source, b"original")?;
        let result = file_op(
            "CAP-FILE-005",
            std::slice::from_ref(&source),
            &dir,
            &[
                ("mode", "case"),
                ("letterCase", "lower"),
                ("caseScope", "name"),
            ],
        )?;
        assert!(!result.partial);
        let names: Vec<_> = std::fs::read_dir(&dir)?
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("abc.txt")]);
        assert_eq!(std::fs::read(dir.join("abc.txt"))?, b"original");
        let protected = dir.join("protected.txt");
        std::fs::write(&protected, b"protected")?;
        let result = file_op(
            "CAP-FILE-005",
            &[dir.join("abc.txt")],
            &dir,
            &[("mode", "template"), ("template", "PROTECTED.txt")],
        )?;
        assert!(!result.partial);
        assert_eq!(std::fs::read(&protected)?, b"protected");
        assert!(result.output.contains("PROTECTED (1).txt"));
        assert_eq!(std::fs::read(dir.join("PROTECTED (1).txt"))?, b"original");
        Ok(())
    });

    scenario!(
        "AC-CAP-097",
        "创建/修改日期真实整理与缺拍摄日期/工具的明确反馈已验证；本机缺 exiftool，已知 EXIF 拍摄时间及时区样本仍待工具安装后验收",
        {
            let dir = workdir("097");
            for field in ["created", "modified"] {
                let source = dir.join(format!("{field}.txt"));
                std::fs::write(&source, b"x")?;
                let metadata = std::fs::metadata(&source)?;
                let date = time::OffsetDateTime::from(if field == "created" {
                    metadata.created()?
                } else {
                    metadata.modified()?
                });
                let expected = format!(
                    "{:04}-{:02}-{:02}",
                    date.year(),
                    u8::from(date.month()),
                    date.day()
                );
                let result = file_op(
                    "CAP-FILE-007",
                    std::slice::from_ref(&source),
                    &dir,
                    &[
                        ("groupBy", "date"),
                        ("dateSource", field),
                        ("timezone", "UTC"),
                        ("directoryFormat", "YYYY-MM-DD"),
                    ],
                )?;
                assert!(!result.partial);
                assert!(
                    dir.join(expected)
                        .join(source.file_name().unwrap())
                        .exists()
                );
            }
            let source = dir.join("no-photo.txt");
            std::fs::write(&source, b"no exif")?;
            let result = file_op(
                "CAP-FILE-007",
                std::slice::from_ref(&source),
                &dir,
                &[
                    ("groupBy", "date"),
                    ("dateSource", "taken"),
                    ("timezone", "UTC"),
                ],
            )?;
            assert!(result.partial);
            assert!(source.exists());
            assert!(result.output.contains("exiftool") || result.output.contains("拍摄日期"));
            Ok(())
        }
    );

    scenario!(
        "AC-CAP-098",
        "真实隐藏与只读权限样本引用系统属性，不确定原因明确标记；云端 dataless 文件需专门设备样本",
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = workdir("098");
            let source = dir.join(".hidden.txt");
            std::fs::write(&source, b"content")?;
            std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o444))?;
            let result = file_op("CAP-FILE-016", &[source], &dir, &[])?;
            assert!(!result.partial);
            assert!(result.output.contains("点文件=true"));
            assert!(result.output.contains("mode=444"));
            assert!(result.output.contains("无法仅据"));
            Ok(())
        }
    );
}

fn git(directory: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    command
        .args(["-C", directory.to_str().unwrap()])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1");
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8_lossy(&result.stdout).into_owned()
}
fn init_git(directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
    git(directory, &["init", "-q", "-b", "main"]);
    git(directory, &["config", "user.name", "Fleqi Matrix"]);
    git(
        directory,
        &["config", "user.email", "matrix@example.invalid"],
    );
    git(directory, &["config", "commit.gpgsign", "false"]);
    git(directory, &["config", "core.hooksPath", "/dev/null"]);
    std::fs::write(directory.join("seed.txt"), "seed\n").unwrap();
    git(directory, &["add", "--all"]);
    git(directory, &["commit", "-qm", "seed"]);
}
fn http_server(response: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request);
        socket.write_all(&response).unwrap();
    });
    (format!("http://{address}/fixture"), handle)
}
#[test]
fn matrix_search_and_calculation() {
    let _batch = MatrixBatch;
    scenario!(
        "AC-CAP-099",
        "真实匹配只限当前层、后缀大小写处理正确；Finder 桌面选择动作需自动化权限原生验收",
        {
            let dir = workdir("099");
            std::fs::create_dir(dir.join("sub"))?;
            for name in ["top.MP4", "other.txt", "sub/deep.mp4"] {
                std::fs::write(dir.join(name), b"sample")?;
            }
            let result = file_op(
                "CAP-FILE-017",
                &[],
                &dir,
                &[("extensions", "mp4"), ("action", "list")],
            )?;
            assert!(!result.partial);
            assert!(result.output.contains("top.MP4"));
            assert!(!result.output.contains("deep.mp4") && !result.output.contains("other.txt"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-100",
        "真实递归多层匹配完整且按目录分组；跨 Finder 窗口选择/定位仍需原生桌面验收",
        {
            let dir = workdir("100");
            std::fs::create_dir_all(dir.join("a/b"))?;
            for name in ["top.mp4", "a/inner.MP4", "a/b/deep.mp4", "a/other.txt"] {
                std::fs::write(dir.join(name), b"sample")?;
            }
            let result = file_op(
                "CAP-FILE-018",
                &[],
                &dir,
                &[
                    ("extensions", "mp4"),
                    ("recursive", "true"),
                    ("action", "list"),
                ],
            )?;
            assert!(!result.partial);
            for name in ["top.mp4", "inner.MP4", "deep.mp4"] {
                assert!(result.output.contains(name), "{}", result.output);
            }
            assert!(!result.output.contains("other.txt"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-101",
        "真实 PDF 用户关键词匹配及坏文件不完整反馈已验证；Finder选择动作与扫描 OCR 仍需设备/语言模型",
        {
            let dir = workdir("101");
            let source = test_pdf(&dir, "match.pdf", 2)?;
            let other = test_pdf(&dir, "other.pdf", 1)?;
            std::fs::write(dir.join("broken.pdf"), b"broken")?;
            let result = file_op(
                "CAP-FILE-019",
                &[],
                &dir,
                &[("keyword", "Page 2"), ("ocr", "never"), ("action", "list")],
            )?;
            assert!(result.partial);
            assert!(
                result
                    .output
                    .contains(source.file_name().unwrap().to_str().unwrap())
            );
            assert!(!result.output.contains(other.to_str().unwrap()));
            assert!(result.output.contains("broken.pdf"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-102",
        "真实指定范围跨目录 PDF 命中与坏文件不完整范围已验证；不把该临时范围当全盘扫描，Finder定位仍待原生验收",
        {
            let dir = workdir("102");
            std::fs::create_dir_all(dir.join("nested"))?;
            test_pdf(&dir, "root.pdf", 2)?;
            test_pdf(&dir.join("nested"), "deep.pdf", 2)?;
            std::fs::write(dir.join("nested/bad.pdf"), b"bad")?;
            let result = file_op(
                "CAP-FILE-020",
                &[],
                &dir,
                &[
                    ("keyword", "Page 2"),
                    ("recursive", "true"),
                    ("ocr", "never"),
                    ("action", "list"),
                ],
            )?;
            assert!(result.partial);
            assert!(
                result.output.contains("root.pdf")
                    && result.output.contains("deep.pdf")
                    && result.output.contains("bad.pdf")
            );
            assert!(
                result.output.contains("不完整") || result.output.contains("失败"),
                "{}",
                result.output
            );
            Ok(())
        }
    );
    scenario!("AC-CAP-103", {
        let compute = ComputeCapabilities;
        for (percent, base, expected) in [
            (15.0, 85.0, 12.75),
            (7.5, 12.5, 0.9375),
            (-20.0, 50.0, -10.0),
        ] {
            assert!((compute.percent(percent, base)? - expected).abs() < 1e-10);
        }
        Ok(())
    });
    scenario!("AC-CAP-104", {
        let compute = ComputeCapabilities;
        assert!((compute.height_to_cm(5, 7)? - 170.18).abs() < 1e-10);
        assert!((compute.height_to_cm(6, 0)? - 182.88).abs() < 1e-10);
        Ok(())
    });
    scenario!("AC-CAP-105", {
        let compute = ComputeCapabilities;
        for (value, unit, expected) in [
            (1.5, "hours", 5400.0),
            (2.0, "days", 172800.0),
            (42.0, "seconds", 42.0),
        ] {
            assert!((compute.duration_to_seconds(value, unit)? - expected).abs() < 1e-10);
        }
        assert!(compute.duration_to_seconds(1.0, "months").is_err());
        Ok(())
    });
    scenario!("AC-CAP-106", {
        let compute = ComputeCapabilities;
        assert!((compute.sqrt(144.0)? - 12.0).abs() < 1e-10);
        assert!((compute.sqrt(2.25)? - 1.5).abs() < 1e-10);
        assert!(compute.sqrt(-4.0).is_err());
        Ok(())
    });
}
#[test]
fn matrix_system_operations() {
    let _batch = MatrixBatch;
    let dir = workdir("system");
    scenario!(
        "AC-CAP-107",
        "已验证不存在终端应用的明确拒绝；实际特殊路径 cwd 需 Terminal/iTerm 和 Fleqi PTY 原生验收",
        {
            let error = system_op(
                "CAP-SYSTEM-002",
                &[],
                &dir,
                &[("terminal", "MissingTerminal")],
            )
            .unwrap_err();
            assert!(error.contains("terminal") || error.contains("终端"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-108",
        "真实查询并拒绝系统卷；可移除测试卷卸载与占用状态需专门设备验收",
        {
            let error = system_op("CAP-SYSTEM-003", &[], &dir, &[("volume", "/")]).unwrap_err();
            assert!(error.contains("拒绝弹出系统卷"), "{error}");
            Ok(())
        }
    );
    scenario!("AC-CAP-109", {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        for cancel_early in [false, true] {
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let directory = dir.to_path_buf();
            let seconds = if cancel_early { "30" } else { "2" };
            let worker = std::thread::spawn(move || {
                fleqi_adapters::system_operations::execute(
                    "CAP-SYSTEM-004",
                    &[],
                    &directory,
                    &parameters(&[("seconds", seconds), ("type", "idle")]),
                    &flag,
                )
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut child = None;
            while std::time::Instant::now() < deadline {
                let output = std::process::Command::new("/usr/bin/pgrep")
                    .args(["-P", &std::process::id().to_string(), "caffeinate"])
                    .output()?;
                if output.status.success() {
                    child = String::from_utf8(output.stdout)?
                        .lines()
                        .next()
                        .map(str::to_owned);
                    break;
                }
                std::thread::yield_now();
            }
            let child = child.ok_or("未观察到保持唤醒进程")?;
            let assertions = command("/usr/bin/pmset", &["-g", "assertions"]);
            assert!(
                assertions.contains(&format!("pid {child}(caffeinate)")),
                "未观察到真实电源断言"
            );
            if cancel_early {
                cancel.store(true, Ordering::Release);
            }
            let result = worker.join().unwrap();
            if cancel_early {
                assert!(result.is_err());
            } else {
                assert!(result?.output.contains("释放"));
            }
            let assertions = command("/usr/bin/pmset", &["-g", "assertions"]);
            assert!(
                !assertions.contains(&format!("pid {child}(caffeinate)")),
                "结束后仍有电源断言"
            );
        }
        Ok(())
    });
    scenario!(
        "AC-CAP-110",
        "无效外观参数在系统修改前被拒绝；系统真实明暗切换仍需受控设备验收",
        {
            let error =
                system_op("CAP-SYSTEM-005", &[], &dir, &[("appearance", "invalid")]).unwrap_err();
            assert!(error.contains("appearance"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-111",
        "无效 Finder 显隐参数在设置修改前被拒绝；Finder实际显隐仍需受控桌面验收",
        {
            let error =
                system_op("CAP-SYSTEM-006", &[], &dir, &[("visible", "invalid")]).unwrap_err();
            assert!(error.contains("visible"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-112",
        "真实文件和无效打印份数触发明确拒绝，无打印作业发出；测试打印队列真实受理仍待可用队列",
        {
            let source = dir.join("print.txt");
            std::fs::write(&source, "print sample")?;
            let error =
                system_op("CAP-SYSTEM-007", &[source], &dir, &[("copies", "0")]).unwrap_err();
            assert!(error.contains("copies"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-113",
        "负延迟在发送休眠请求前被拒绝；实际休眠/恢复及持久化记录需专门受控设备验收",
        {
            let error =
                system_op("CAP-SYSTEM-008", &[], &dir, &[("delaySeconds", "-1")]).unwrap_err();
            assert!(error.contains("delaySeconds"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-114",
        "本机 CPU 名称/架构与独立 sysctl 一致；另一芯片架构仍需设备验收",
        {
            let output = system_op("CAP-SYSTEM-009", &[], &dir, &[("fields", "all")])?;
            let name = command("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]);
            let architecture = command("/usr/sbin/sysctl", &["-n", "hw.machine"]);
            assert!(output.contains(name.trim()) && output.contains(architecture.trim()));
            Ok(())
        }
    );
    scenario!("AC-CAP-115", {
        let output: serde_json::Value = serde_json::from_str(&system_op(
            "CAP-SYSTEM-010",
            &[],
            &dir,
            &[("unit", "bytes"), ("scope", "total")],
        )?)?;
        let expected = command("/usr/sbin/sysctl", &["-n", "hw.memsize"])
            .trim()
            .parse::<f64>()?;
        assert_eq!(output["total"].as_f64(), Some(expected));
        assert!(output.get("Pages active").is_none());
        Ok(())
    });
    scenario!(
        "AC-CAP-116",
        "已将当前每个显示器的物理/逻辑字段与独立 system_profiler 比对；未连接的多屏配置需设备验收",
        {
            let output: serde_json::Value = serde_json::from_str(&system_op(
                "CAP-SYSTEM-011",
                &[],
                &dir,
                &[("scope", "both")],
            )?)?;
            let expected: serde_json::Value = serde_json::from_str(&command(
                "/usr/sbin/system_profiler",
                &["SPDisplaysDataType", "-json"],
            ))?;
            let screens: Vec<_> = expected["SPDisplaysDataType"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|gpu| gpu["spdisplays_ndrvs"].as_array().into_iter().flatten())
                .collect();
            assert_eq!(output["displays"].as_array().unwrap().len(), screens.len());
            for display in output["displays"].as_array().unwrap() {
                let actual = screens
                    .iter()
                    .find(|s| s["_spdisplays_displayID"] == display["id"])
                    .unwrap();
                assert_eq!(display["physicalPixels"], actual["_spdisplays_pixels"]);
                assert_eq!(
                    display["logicalResolution"],
                    actual["_spdisplays_resolution"]
                );
            }
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-117",
        "已查询本机额定及实时口径并核验未知反馈；插拔不同适配器状态需硬件验收",
        {
            let rated = system_op("CAP-SYSTEM-012", &[], &dir, &[("scope", "rated")])?;
            let measured = system_op("CAP-SYSTEM-012", &[], &dir, &[("scope", "measured")])?;
            assert!(
                rated.contains("额定")
                    || rated.contains("未知")
                    || rated.contains("未提供")
                    || rated.contains("没有提供")
            );
            assert!(
                measured.contains("实时")
                    || measured.contains("未知")
                    || measured.contains("未提供")
            );
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-118",
        "当前电池状态与独立 pmset 原始信息一致；充电/已满/无电池多设备状态仍需硬件验收",
        {
            let output = system_op("CAP-SYSTEM-013", &[], &dir, &[])?;
            let actual = command("/usr/bin/pmset", &["-g", "batt"]);
            let line = actual.lines().find(|l| l.contains("InternalBattery"));
            if let Some(line) = line {
                assert!(output.contains(line.trim()));
            } else {
                assert!(output.contains("未报告内置电池"));
            }
            Ok(())
        }
    );
    scenario!("AC-CAP-119", {
        use std::os::macos::fs::MetadataExt;
        let source = dir.join("quoted ' 中文.txt");
        std::fs::write(&source, b"keep")?;
        for value in ["true", "false"] {
            let output = system_op(
                "CAP-SYSTEM-014",
                std::slice::from_ref(&source),
                &dir,
                &[("hidden", value)],
            )?;
            assert_eq!(
                std::fs::metadata(&source)?.st_flags() & libc::UF_HIDDEN != 0,
                value == "true"
            );
            assert!(output.contains("Finder"));
            assert_eq!(std::fs::read(&source)?, b"keep");
        }
        Ok(())
    });
    cond(
        "AC-CAP-120",
        "实际 PTY cwd 查询及忙碌程序输入隔离依赖宿主观察端口；本矩阵不注入 pwd 冒充原生验收，见终端原生合同证据",
    );
}
#[test]
fn matrix_git_tools_network() {
    let _batch = MatrixBatch;
    scenario!("AC-CAP-121", {
        let dir = workdir("121");
        let repo = dir.join("repo");
        init_git(&repo);
        std::fs::create_dir(repo.join("sub"))?;
        let worktree = dir.join("worktree");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "matrix-worktree",
                worktree.to_str().unwrap(),
            ],
        );
        for path in [&repo, &repo.join("sub"), &worktree] {
            assert!(
                file_op("CAP-DEV-001", &[], path, &[])?
                    .output
                    .contains("是 Git 工作区")
            );
        }
        let plain = dir.join("plain");
        std::fs::create_dir(&plain)?;
        assert!(
            file_op("CAP-DEV-001", &[], &plain, &[])?
                .output
                .contains("不是 Git 仓库")
        );
        assert!(file_op("CAP-DEV-001", &[], &dir.join("missing"), &[]).is_err());
        Ok(())
    });
    scenario!("AC-CAP-122", {
        let dir = workdir("122");
        let repo = dir.join("repo");
        init_git(&repo);
        let remote = dir.join("remote.git");
        git(
            &repo,
            &[
                "clone",
                "-q",
                "--bare",
                repo.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        );
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        let peer = dir.join("peer");
        git(
            &repo,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                peer.to_str().unwrap(),
            ],
        );
        git(&peer, &["config", "user.name", "Matrix"]);
        git(&peer, &["config", "user.email", "matrix@example.invalid"]);
        git(&peer, &["config", "commit.gpgsign", "false"]);
        std::fs::write(peer.join("new.txt"), "from remote")?;
        git(&peer, &["add", "."]);
        git(&peer, &["commit", "-qm", "remote new"]);
        git(&peer, &["push", "-q", "origin", "main"]);
        file_op(
            "CAP-DEV-002",
            &[],
            &repo,
            &[
                ("remote", "origin"),
                ("branch", "main"),
                ("strategy", "ff-only"),
            ],
        )?;
        assert_eq!(
            std::fs::read_to_string(repo.join("new.txt"))?,
            "from remote"
        );
        std::fs::write(peer.join("seed.txt"), "remote\n")?;
        git(&peer, &["commit", "-qam", "remote conflict"]);
        git(&peer, &["push", "-q", "origin", "main"]);
        std::fs::write(repo.join("seed.txt"), "local\n")?;
        git(&repo, &["commit", "-qam", "local conflict"]);
        let failure = file_op(
            "CAP-DEV-002",
            &[],
            &repo,
            &[
                ("remote", "origin"),
                ("branch", "main"),
                ("strategy", "merge"),
            ],
        )
        .err()
        .ok_or("预期冲突")?;
        assert!(failure.contains("未自动重置"));
        assert!(!git(&repo, &["diff", "--name-only", "--diff-filter=U"]).is_empty());
        let content = std::fs::read_to_string(repo.join("seed.txt"))?;
        assert!(content.contains("local") && content.contains("remote"));
        Ok(())
    });
    scenario!("AC-CAP-123", {
        let dir = workdir("123");
        init_git(&dir);
        file_op(
            "CAP-DEV-003",
            &[],
            &dir,
            &[("branch", "feature-matrix"), ("create", "true")],
        )?;
        assert_eq!(
            git(&dir, &["branch", "--show-current"]).trim(),
            "feature-matrix"
        );
        std::fs::write(dir.join("seed.txt"), "feature\n")?;
        git(&dir, &["commit", "-qam", "feature"]);
        file_op(
            "CAP-DEV-003",
            &[],
            &dir,
            &[("branch", "main"), ("create", "false")],
        )?;
        std::fs::write(dir.join("seed.txt"), "uncommitted\n")?;
        assert!(
            file_op(
                "CAP-DEV-003",
                &[],
                &dir,
                &[("branch", "feature-matrix"), ("create", "false")]
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("seed.txt"))?,
            "uncommitted\n"
        );
        assert_eq!(git(&dir, &["branch", "--show-current"]).trim(), "main");
        Ok(())
    });
    scenario!("AC-CAP-124", {
        let dir = workdir("124");
        let repo = dir.join("repo");
        init_git(&repo);
        let remote = dir.join("remote.git");
        git(
            &repo,
            &[
                "clone",
                "-q",
                "--bare",
                repo.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        );
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        std::fs::create_dir(repo.join("sub"))?;
        std::fs::remove_file(repo.join("seed.txt"))?;
        std::fs::write(repo.join("root.txt"), "new")?;
        std::fs::write(repo.join("sub/inside.txt"), "inside")?;
        let result = file_op(
            "CAP-DEV-004",
            &[],
            &repo.join("sub"),
            &[
                ("scope", "all"),
                ("message", "all requested"),
                ("remote", "origin"),
                ("branch", "main"),
            ],
        )?;
        assert!(result.output.contains("推送成功"));
        assert_eq!(
            git(&repo, &["rev-parse", "HEAD"]),
            git(&remote, &["rev-parse", "refs/heads/main"])
        );
        let names = git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
        assert!(
            names.contains("root.txt")
                && names.contains("sub/inside.txt")
                && !names.contains("seed.txt")
        );
        std::fs::write(repo.join("retained.txt"), "retained commit")?;
        let failure = file_op(
            "CAP-DEV-004",
            &[],
            &repo,
            &[
                ("scope", "all"),
                ("message", "must retain"),
                ("remote", "missing-remote"),
                ("branch", "main"),
            ],
        )
        .err()
        .ok_or("推送应失败")?;
        assert!(failure.contains("提交成功") && failure.contains("推送失败"));
        assert_eq!(
            git(&repo, &["log", "-1", "--format=%s"]).trim(),
            "must retain"
        );
        Ok(())
    });
    scenario!("AC-CAP-125", {
        let dir = workdir("125");
        std::fs::create_dir_all(dir.join("node_modules"))?;
        std::fs::write(dir.join("app.js"), "one\ntwo\n")?;
        std::fs::write(dir.join("other.ts"), "one\ntwo\nthree\n")?;
        std::fs::write(dir.join("node_modules/dep.js"), "ignored\n")?;
        let js = file_op("CAP-DEV-005", &[], &dir, &[])?;
        assert!(js.output.contains("app.js") && !js.output.contains("dep.js"));
        assert!(js.output.contains("node_modules"));
        let ts = file_op("CAP-DEV-005", &[], &dir, &[("extensions", "ts")])?;
        assert!(ts.output.contains("other.ts") && !ts.output.contains("app.js"));
        assert!(ts.output.contains("3"));
        Ok(())
    });
    scenario!("AC-CAP-126", {
        let dir = workdir("126");
        for (name, bytes) in [
            ("vector", b"abc".to_vec()),
            ("large", vec![9u8; 5 * 1024 * 1024]),
        ] {
            let file = dir.join(name);
            std::fs::write(&file, bytes)?;
            let result = file_op("CAP-DEV-006", std::slice::from_ref(&file), &dir, &[])?;
            let independent = command("/usr/bin/shasum", &["-a", "256", file.to_str().unwrap()]);
            assert!(
                result
                    .output
                    .contains(independent.split_whitespace().next().unwrap())
            );
        }
        Ok(())
    });
    scenario!("AC-CAP-127", {
        let dir = workdir("127");
        let source = dir.join("owned.txt");
        std::fs::write(&source, b"same")?;
        command(
            "/usr/bin/xattr",
            &[
                "-w",
                "com.apple.quarantine",
                "0081;12345678;FleqiMatrix;",
                source.to_str().unwrap(),
            ],
        );
        command(
            "/usr/bin/xattr",
            &[
                "-w",
                "test.fleqi.keep",
                "retained",
                source.to_str().unwrap(),
            ],
        );
        let result = file_op("CAP-DEV-007", std::slice::from_ref(&source), &dir, &[])?;
        assert!(!result.partial);
        let attributes = command("/usr/bin/xattr", &[source.to_str().unwrap()]);
        assert!(
            !attributes.contains("com.apple.quarantine") && attributes.contains("test.fleqi.keep")
        );
        let second = file_op("CAP-DEV-007", &[source], &dir, &[])?;
        assert!(second.output.contains("不存在") || second.output.contains("没有"));
        Ok(())
    });
    scenario!(
        "AC-CAP-128",
        "本机已安装 Homebrew 路径/版本与独立命令一致且检测未安装任何包；未安装/异常路径环境仍需隔离设备验收",
        {
            let dir = workdir("128");
            let result = file_op("CAP-TOOLS-001", &[], &dir, &[])?;
            let independent = command("brew", &["--version"]);
            assert!(result.output.contains(independent.trim()));
            assert!(result.output.contains("路径"));
            Ok(())
        }
    );
    scenario!("AC-CAP-129", {
        let dir = workdir("129");
        let all = file_op("CAP-TOOLS-002", &[], &dir, &[("scope", "all")])?;
        let list = command("brew", &["list", "--formula", "--versions"]);
        for line in list.lines() {
            let name = line.split_whitespace().next().unwrap();
            assert!(
                all.output.contains(&format!("formula {name}\t")),
                "缺包{name}"
            );
        }
        let leaves = file_op("CAP-TOOLS-002", &[], &dir, &[("scope", "leaves")])?;
        let expected = command("brew", &["leaves"]);
        for name in expected.lines() {
            assert!(
                leaves.output.contains(&format!("formula {name}\t")),
                "缺叶子包{name}"
            );
        }
        Ok(())
    });
    scenario!(
        "AC-CAP-130",
        "非法包名在执行安装前被拒绝，预取消有效；真实 Ghostscript/其它包安装及完整性校验需受控安装环境",
        {
            let dir = workdir("130");
            assert!(file_op("CAP-TOOLS-003", &[], &dir, &[("package", "--invalid")]).is_err());
            let result = fleqi_adapters::file_operations::execute(
                "CAP-TOOLS-003",
                &[],
                &dir,
                &parameters(&[("package", "ghostscript")]),
                &std::sync::atomic::AtomicBool::new(true),
            );
            assert!(result.err().unwrap().contains("取消"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-131",
        "真实 Homebrew 清单确认目标未安装并拒绝卸载；未触碰用户包，指定包卸载和共享依赖保留需受控安装环境",
        {
            let dir = workdir("131");
            let error = file_op(
                "CAP-TOOLS-004",
                &[],
                &dir,
                &[("package", "fleqi-matrix-uninstalled-unique-package")],
            )
            .err()
            .ok_or("未安装包不能卸载成功")?;
            assert!(
                error.contains("未管理已安装包") || error.contains("未安装"),
                "{error}"
            );
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-132",
        "真实回环 ICMP 统计与参数校验通过；不可达网络路径需受控网络场景，未把命令拒绝冒充丢包验收",
        {
            let dir = workdir("132");
            let result = system_op(
                "CAP-NETWORK-001",
                &[],
                &dir,
                &[
                    ("host", "127.0.0.1"),
                    ("count", "1"),
                    ("timeoutSeconds", "1"),
                ],
            )?;
            assert!(result.contains("ICMP") && result.contains("1 packets transmitted"));
            assert!(result.contains("0.0% packet loss") || result.contains("0% packet loss"));
            assert!(system_op("CAP-NETWORK-001", &[], &dir, &[("host", "--invalid")]).is_err());
            Ok(())
        }
    );
    scenario!("AC-CAP-133", {
        let dir = workdir("133");
        let target = dir.join("payload.bin");
        std::fs::write(&target, b"old")?;
        let (last, server) = http_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone".to_vec(),
        );
        let (first,redirect)=http_server(format!("HTTP/1.1 302 Found\r\nLocation: {last}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes());
        let result = system_op(
            "CAP-NETWORK-002",
            &[],
            &dir,
            &[("url", &first), ("destination", "payload.bin")],
        );
        server.join().unwrap();
        redirect.join().unwrap();
        let result: serde_json::Value = serde_json::from_str(&result?)?;
        assert_eq!(std::fs::read(result["file"].as_str().unwrap())?, b"done");
        assert_eq!(result["resolvedSource"], last);
        assert_eq!(result["bytes"], 4);
        assert_eq!(std::fs::read(&target)?, b"old");
        let before = std::fs::read_dir(&dir)?.count();
        for response in [
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial".to_vec(),
        ] {
            let (url, server) = http_server(response);
            let result = system_op(
                "CAP-NETWORK-002",
                &[],
                &dir,
                &[
                    ("url", &url),
                    ("destination", "payload.bin"),
                    ("collision", "overwrite"),
                ],
            );
            server.join().unwrap();
            assert!(result.is_err());
            assert_eq!(std::fs::read(&target)?, b"old");
            assert_eq!(std::fs::read_dir(&dir)?.count(), before);
        }
        Ok(())
    });
    scenario!(
        "AC-CAP-134",
        "缺地点和非法单位被真实天气执行器拒绝；多地点来源结果、歧义澄清与服务失败需联网天气源验收",
        {
            let dir = workdir("134");
            let error = system_op("CAP-NETWORK-003", &[], &dir, &[]).unwrap_err();
            assert!(error.contains("地点"));
            let error = system_op(
                "CAP-NETWORK-003",
                &[],
                &dir,
                &[("location", "35,139"), ("unit", "invalid")],
            )
            .unwrap_err();
            assert!(error.contains("unit"));
            Ok(())
        }
    );
    scenario!(
        "AC-CAP-135",
        "无效收件人在消息发送前明确失败；未发送消息，受控收件人真实收到文本/附件仍需登录账号及授权",
        {
            let dir = workdir("135");
            let error = system_op(
                "CAP-SYSTEM-015",
                &[],
                &dir,
                &[
                    ("recipient", "invalid recipient"),
                    ("text", "must not send"),
                ],
            )
            .unwrap_err();
            assert!(
                error.contains("收件") || error.contains("recipient"),
                "{error}"
            );
            Ok(())
        }
    );
}
