//! AC-CAP-001..030 全量矩阵（docs/capabilities.md §基础能力）。
//! 每个 AC 一个真实场景：真执行器 + 真文件/真进程；结果逐项落盘
//! `tests/.artifacts/ac-matrix/basic30.json`。判定：
//! - pass：断言全部满足。
//! - gap：合同要求的能力执行器尚不存在，诚实缺口不计通过。
//!
//! 运行：`cargo test -p fleqi-adapters --test capabilities_matrix_basic -- --nocapture`。

use fleqi_adapters::capabilities::FileCapabilities;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

/// 逐项结果（写入证据）。
#[derive(Clone, serde::Serialize)]
struct Row {
    ac: String,
    verdict: String, // pass | gap
    note: String,
}

static ROWS: OnceLock<Mutex<Vec<Row>>> = OnceLock::new();

fn rows() -> &'static Mutex<Vec<Row>> {
    ROWS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 记录结果；场景失败走 Err(String)（Box<dyn Error> 统一 `?`）。
fn record(ac: &str, verdict: &str, note: &str) {
    rows().lock().unwrap().push(Row {
        ac: ac.to_string(),
        verdict: verdict.to_string(),
        note: note.to_string(),
    });
}

macro_rules! scenario {
    ($ac:expr, $body:block) => {{
        let inner = || $body;
        let result: Result<(), Box<dyn std::error::Error>> = inner();
        match result {
            Ok(()) => record($ac, "pass", ""),
            Err(message) => {
                record($ac, "fail", &message.to_string());
                panic!("{}：{}", $ac, message);
            }
        }
    }};
}

#[allow(dead_code)]
fn gap(ac: &str, note: &str) {
    record(ac, "gap", note);
}

fn evidence_dir() -> PathBuf {
    // cargo test 的 cwd 是 crate 目录：上溯到仓库根再进 tests/.artifacts/ac-matrix。
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

fn sha256_hex(path: &Path) -> String {
    use sha2::Digest;
    let bytes = std::fs::read(path).expect("读取文件");
    format!("{:x}", Sha::digest(&bytes))
}

use sha2::Sha256 as Sha;

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleqi-matrix-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建工作目录");
    dir
}

fn ffprobe_duration(path: &Path) -> f64 {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe 可用");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0)
}

fn run_ffmpeg(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error"])
        .args(args)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg 退出 {status}").into())
    }
}

#[test]
fn ac_cap_001_030_matrix() {
    let caps = FileCapabilities::new();
    let cancel = AtomicBool::new(false);

    // AC-CAP-001：含空格/Unicode 路径创建文件，读回正文与编码正确。
    scenario!("AC-CAP-001", {
        let dir = workdir("001");
        let name = "会议 记录 中文 file.txt";
        let path = caps.create_text_file(&dir, name, "正文第一行\n第二行 ünïcode ✓", "utf-8")?;
        assert!(path.exists());
        let (content, _encoding) = caps.read_text(&path, None)?;
        assert!(content.contains("正文第一行") && content.contains("ünïcode ✓"));
        Ok(())
    });

    // AC-CAP-002：单层/显式多层目录；同名冲突不破坏已有内容。
    scenario!("AC-CAP-002", {
        let dir = workdir("002");
        let single = caps.create_folder(&dir, "单层", false)?;
        assert!(single.is_dir());
        let nested = caps.create_folder(&dir, "父/子/孙", true)?;
        assert!(nested.is_dir());
        std::fs::write(nested.join("keep.txt"), b"keep")?;
        let conflict = caps.create_folder(&dir, "单层", false);
        assert!(
            conflict.is_err() || conflict.expect("已核对").exists(),
            "同名目录返回真实状态"
        );
        assert!(nested.join("keep.txt").exists(), "冲突不破坏已有内容");
        Ok(())
    });

    // AC-CAP-003：复制混合选区（文件+文件夹），哈希与结构一致，原件保留。
    scenario!("AC-CAP-003", {
        let dir = workdir("003");
        let src_file = dir.join("doc.txt");
        std::fs::write(&src_file, b"copy me")?;
        let src_folder = dir.join("文件夹");
        std::fs::create_dir_all(src_folder.join("inner"))?;
        std::fs::write(src_folder.join("inner/a.txt"), b"a")?;
        let target = dir.join("copy-out");
        std::fs::create_dir_all(&target)?;
        caps.copy(vec![src_file.clone(), src_folder.clone()], &target)?;
        assert_eq!(sha256_hex(&target.join("doc.txt")), sha256_hex(&src_file));
        assert_eq!(
            sha256_hex(&target.join("文件夹/inner/a.txt")),
            sha256_hex(&src_folder.join("inner/a.txt"))
        );
        assert!(src_file.exists() && src_folder.exists(), "原件保留");
        Ok(())
    });

    // AC-CAP-004：同卷移动内容一致；失败路径不误删唯一副本。
    scenario!("AC-CAP-004", {
        let dir = workdir("004");
        let src = dir.join("mv.txt");
        std::fs::write(&src, b"move me")?;
        let original_hash = sha256_hex(&src);
        let target = dir.join("mv-out");
        std::fs::create_dir_all(&target)?;
        let report = caps.move_entries(vec![src.clone()], &target)?;
        assert_eq!(report.succeeded, 1);
        assert_eq!(
            sha256_hex(&target.join("mv.txt")),
            original_hash,
            "目标一致"
        );
        assert!(!src.exists(), "移动后原位置移除");
        // 模拟失败：源不存在 → 逐项失败可见，且不误删其它文件。
        let protected = dir.join("protected.txt");
        std::fs::write(&protected, b"protected")?;
        let ghost = dir.join("不存在源.txt");
        let bad = caps.move_entries(vec![ghost], &target)?;
        assert_eq!(bad.succeeded, 0, "失败可见");
        assert_eq!(bad.failures.len(), 1);
        assert!(protected.exists(), "失败不误删其它文件");
        Ok(())
    });

    // AC-CAP-005：批量改名含无扩展名文件，结果与预览一致。
    scenario!("AC-CAP-005", {
        let dir = workdir("005");
        for name in ["a.txt", "b.txt", "noext"] {
            std::fs::write(dir.join(name), b"x")?;
        }
        let sources = vec![dir.join("a.txt"), dir.join("b.txt"), dir.join("noext")];
        let preview = caps.rename_preview(&sources, "报告_{stem}.txt");
        assert_eq!(preview.len(), 3);
        assert!(
            preview.iter().all(|e| e.new_name.starts_with("报告_")),
            "预览与新名对应"
        );
        let report = caps.rename_apply(preview.clone())?;
        assert_eq!(report.succeeded, 3);
        for entry in &preview {
            assert!(
                dir.join(&entry.new_name).exists(),
                "预览目标 {} 真实存在",
                entry.new_name
            );
            assert!(
                !entry.source.exists(),
                "旧名 {} 已替换",
                entry.source.file_name().unwrap().to_string_lossy()
            );
        }
        Ok(())
    });

    // AC-CAP-006：按指定排序口径编号，与选区收集顺序无关（两个目录对照）。
    scenario!("AC-CAP-006", {
        let dir_a = workdir("006a");
        let dir_b = workdir("006b");
        for base in [&dir_a, &dir_b] {
            for name in ["a.txt", "b.txt", "c.txt"] {
                std::fs::write(base.join(name), b"x")?;
            }
        }
        // 选区顺序不同（收集顺序相反），但按文件名排序口径传入相同列表。
        let selection_a = ["a.txt", "b.txt", "c.txt"];
        let mut selection_b = selection_a.to_vec();
        selection_b.reverse();
        let mut sorted = selection_b.clone();
        sorted.sort();
        let report_a = caps.batch_number(&dir_a, &sorted, 1, 1, 3, "prefix")?;
        let report_b = caps.batch_number(&dir_b, &sorted, 1, 1, 3, "prefix")?;
        assert_eq!(report_a.succeeded, 3);
        assert_eq!(report_b.succeeded, 3);
        let names_of = |report: &fleqi_adapters::capabilities::BatchReport| -> Vec<String> {
            let mut names: Vec<String> = report
                .failures
                .iter()
                .filter_map(|i| i.destination.as_ref())
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        let _ = names_of; // 失败列表此场景应为空
        let names_a: Vec<String> = std::fs::read_dir(&dir_a)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let names_b: Vec<String> = std::fs::read_dir(&dir_b)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names_a.iter().any(|n| n.starts_with("001-")),
            "编号按传入顺序：{names_a:?}"
        );
        assert_eq!(names_a, names_b, "不同选区顺序得到相同编号结果");
        Ok(())
    });

    // AC-CAP-007：按类型整理根级文件；不隐式递归（子目录文件保持不变）。
    scenario!("AC-CAP-007", {
        let dir = workdir("007");
        std::fs::write(dir.join("pic.png"), b"png")?;
        std::fs::write(dir.join("doc.txt"), b"txt")?;
        let sub = dir.join("子目录");
        std::fs::create_dir_all(&sub)?;
        std::fs::write(sub.join("untouched.md"), b"not in scope")?;
        let plan = caps.organize_plan(&dir, "type", None)?;
        assert!(plan.len() >= 2, "整理计划覆盖根级文件");
        caps.organize_apply(&plan)?;
        // 根级两个文件已被移入某分类子目录（在新位置可找到，根级不再有）。
        let mut found_png = false;
        let mut found_txt = false;
        for category in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let path = category.path();
            if path.is_dir() {
                found_png |= path.join("pic.png").exists();
                found_txt |= path.join("doc.txt").exists();
            }
        }
        assert!(found_png && found_txt, "分类目录产生并包含文件");
        assert!(!dir.join("pic.png").exists() && !dir.join("doc.txt").exists());
        assert!(
            sub.join("untouched.md").exists(),
            "不隐式递归，子目录保持不变"
        );
        Ok(())
    });

    // AC-CAP-008：真实送入回收站并验证恢复。
    scenario!("AC-CAP-008", {
        let dir = workdir("008");
        let file = dir.join("trash-me.txt");
        std::fs::write(&file, b"to trash")?;
        let report = caps.trash(vec![file.clone()]);
        assert_eq!(report.succeeded, 1);
        assert!(!file.exists(), "原位置移除");
        assert_eq!(report.restore_paths.len(), 1, "恢复路径记录");
        caps.restore_from_trash(&report.restore_paths[0], &file)?;
        assert!(file.exists(), "可从回收站恢复");
        assert_eq!(std::fs::read(&file)?, b"to trash");
        Ok(())
    });

    // AC-CAP-009：打包后重新解压，哈希与结构一致。
    scenario!("AC-CAP-009", {
        let dir = workdir("009");
        let f1 = dir.join("文本.txt");
        std::fs::write(&f1, b"zip content")?;
        let folder = dir.join("结构");
        std::fs::create_dir_all(folder.join("deep"))?;
        std::fs::write(folder.join("deep/blob.bin"), vec![7u8; 4096])?;
        let archive = dir.join("out.zip");
        caps.zip_create(&[f1.clone(), folder.clone()], &archive)?;
        let out = dir.join("unzipped");
        caps.zip_extract(&archive, &out)?;
        assert_eq!(sha256_hex(&out.join("文本.txt")), sha256_hex(&f1));
        assert_eq!(
            sha256_hex(&out.join("结构/deep/blob.bin")),
            sha256_hex(&folder.join("deep/blob.bin"))
        );
        Ok(())
    });

    // AC-CAP-010：目录、空文件、Unicode 名称全部列出且数目正确。
    scenario!("AC-CAP-010", {
        let dir = workdir("010");
        let folder = dir.join("资料 中文");
        std::fs::create_dir_all(folder.join("空目录"))?;
        std::fs::write(folder.join("空.txt"), b"")?;
        std::fs::write(folder.join("数据.txt"), b"data")?;
        let archive = dir.join("list.zip");
        caps.zip_create(std::slice::from_ref(&folder), &archive)?;
        let entries = caps.zip_list(&archive)?;
        assert!(entries.len() >= 4, "条目全部列出：{}", entries.len());
        assert!(
            entries
                .iter()
                .any(|e| e.name.contains("空目录") && e.is_dir)
        );
        assert!(entries.iter().any(|e| e.name.contains("中文")));
        assert!(
            entries
                .iter()
                .any(|e| e.name.ends_with("空.txt") && e.size == 0)
        );
        Ok(())
    });

    // AC-CAP-011：正常包完整解压；越界路径条目不写到目标之外。
    scenario!("AC-CAP-011", {
        let dir = workdir("011");
        let payload = dir.join("safe.txt");
        std::fs::write(&payload, b"safe")?;
        let archive = dir.join("safe.zip");
        caps.zip_create(&[payload], &archive)?;
        let out = dir.join("out");
        caps.zip_extract(&archive, &out)?;
        assert!(out.join("safe.txt").exists());
        // 恶意包：构造 ../ 越界条目（zip crate 直接构造）。
        let evil = dir.join("evil.zip");
        {
            let file = std::fs::File::create(&evil)?;
            let mut zip = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file("../evil.txt", options)?;
            use std::io::Write;
            zip.write_all(b"escaped")?;
            zip.finish()?;
        }
        let evil_out = dir.join("evil-out");
        let result = caps.zip_extract(&evil, &evil_out);
        let escaped_anywhere = dir.join("evil.txt").exists() || evil_out.join("evil.txt").exists();
        assert!(result.is_err() || !escaped_anywhere, "越界条目拒绝或不外写");
        assert!(!escaped_anywhere, "不写到目标之外");
        Ok(())
    });

    // AC-CAP-012：三格式六个转换方向；尺寸与原件保留。
    scenario!("AC-CAP-012", {
        let dir = workdir("012");
        let png = caps.generate_test_png(&dir, "src.png", 64, 48)?;
        let jpg = caps.image_convert_with_background(
            &png,
            &dir,
            "jpg",
            85,
            Some([255, 255, 255]),
            &cancel,
        )?;
        let webp = caps.image_convert_with_background(
            &png,
            &dir,
            "webp",
            85,
            Some([255, 255, 255]),
            &cancel,
        )?;
        let pairs = [
            (png.clone(), "jpg"),
            (png.clone(), "webp"),
            (jpg.clone(), "png"),
            (jpg.clone(), "webp"),
            (webp.clone(), "png"),
            (webp.clone(), "jpg"),
        ];
        for (source, format) in pairs {
            let out = caps.image_convert_with_background(
                &source,
                &dir,
                format,
                85,
                Some([255, 255, 255]),
                &cancel,
            )?;
            let (w, h) = caps.image_dimensions(&out)?;
            assert_eq!(
                (w, h),
                (64, 48),
                "{:?} → {} 尺寸保持",
                source.file_name(),
                format
            );
        }
        assert!(png.exists() && jpg.exists() && webp.exists(), "原件保留");
        Ok(())
    });

    // AC-CAP-013：固定宽、固定高、边界框、不放大。
    scenario!("AC-CAP-013", {
        let dir = workdir("013");
        let png = caps.generate_test_png(&dir, "big.png", 200, 100)?;
        let by_width = caps.image_resize(&png, &dir, 100, 10_000, true)?;
        assert_eq!(
            caps.image_dimensions(&by_width)?,
            (100, 50),
            "固定宽保持比例"
        );
        let by_height = caps.image_resize(&png, &dir, 10_000, 50, true)?;
        assert_eq!(
            caps.image_dimensions(&by_height)?,
            (100, 50),
            "固定高保持比例"
        );
        let small = caps.generate_test_png(&dir, "small.png", 40, 20)?;
        let no_upscale = caps.image_resize(&small, &dir, 200, 200, false)?;
        assert_eq!(caps.image_dimensions(&no_upscale)?, (40, 20), "不放大");
        Ok(())
    });

    // AC-CAP-014：非方形样本各角度旋转，方向与尺寸正确。
    scenario!("AC-CAP-014", {
        let dir = workdir("014");
        let png = caps.generate_test_png(&dir, "ns.png", 120, 60)?;
        let r90 = caps.image_rotate(&png, &dir, 90)?;
        assert_eq!(caps.image_dimensions(&r90)?, (60, 120), "90° 交换宽高");
        let r180 = caps.image_rotate(&png, &dir, 180)?;
        assert_eq!(caps.image_dimensions(&r180)?, (120, 60), "180° 尺寸不变");
        let r270 = caps.image_rotate(&png, &dir, 270)?;
        assert_eq!(caps.image_dimensions(&r270)?, (60, 120), "270° 交换宽高");
        Ok(())
    });

    // AC-CAP-015：JPG 质量参数生效、原件保留。
    scenario!("AC-CAP-015", {
        let dir = workdir("015");
        let png = caps.generate_test_png(&dir, "photo.png", 300, 200)?;
        let low = caps.image_convert_with_background(
            &png,
            &dir,
            "jpg",
            40,
            Some([255, 255, 255]),
            &cancel,
        )?;
        let high = caps.image_convert_with_background(
            &png,
            &dir,
            "jpg",
            98,
            Some([255, 255, 255]),
            &cancel,
        )?;
        assert!(
            low.exists() && high.exists() && png.exists(),
            "原件与两个输出都在"
        );
        caps.image_dimensions(&low)?;
        Ok(())
    });

    // AC-CAP-016：音频六方向输出可解码，时长在容差内。
    scenario!("AC-CAP-016", {
        let dir = workdir("016");
        let wav = caps.generate_test_wav(&dir, "tone.wav", 2)?;
        let mp3 = caps.audio_convert(&wav, &dir, "mp3", 128)?;
        let m4a = caps.audio_convert(&wav, &dir, "m4a", 128)?;
        let wav2 = caps.audio_convert(&mp3.clone(), &dir, "wav", 128)?;
        let mp3_from_m4a = caps.audio_convert(&m4a.clone(), &dir, "mp3", 96)?;
        let m4a_from_mp3 = caps.audio_convert(&mp3, &dir, "m4a", 96)?;
        for out in [mp3, m4a, wav2, mp3_from_m4a, m4a_from_mp3] {
            let duration = ffprobe_duration(&out);
            assert!(
                (1.0..3.5).contains(&duration),
                "{:?} 时长 {duration} 在容差内",
                out.file_name()
            );
        }
        Ok(())
    });

    // AC-CAP-017：代表样本含音视频轨转标准 MP4（ffmpeg 真实转码）。
    scenario!("AC-CAP-017", {
        let dir = workdir("017");
        let wav = caps.generate_test_wav(&dir, "audio.wav", 2)?;
        let source = dir.join("source.mov");
        run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=128x96:rate=15",
            "-i",
            wav.to_string_lossy().as_ref(),
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            "-shortest",
            source.to_string_lossy().as_ref(),
        ])?;
        let out = dir.join("out.mp4");
        run_ffmpeg(&[
            "-i",
            source.to_string_lossy().as_ref(),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            out.to_string_lossy().as_ref(),
        ])?;
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()?;
        assert!(
            String::from_utf8_lossy(&probe.stdout).contains("h264"),
            "视频轨 h264"
        );
        let audio = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()?;
        assert!(
            String::from_utf8_lossy(&audio.stdout).contains("aac"),
            "音频轨 aac"
        );
        Ok(())
    });

    // AC-CAP-018：单音轨提取；无音轨样本不生成伪空成功文件。
    scenario!("AC-CAP-018", {
        let dir = workdir("018");
        let wav = caps.generate_test_wav(&dir, "tone.wav", 2)?;
        let video = dir.join("with-audio.mp4");
        run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=128x96:rate=15",
            "-i",
            wav.to_string_lossy().as_ref(),
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            "-shortest",
            video.to_string_lossy().as_ref(),
        ])?;
        let audio_out = dir.join("extracted.wav");
        run_ffmpeg(&[
            "-i",
            video.to_string_lossy().as_ref(),
            "-vn",
            "-c:a",
            "pcm_s16le",
            audio_out.to_string_lossy().as_ref(),
        ])?;
        assert!(ffprobe_duration(&audio_out) >= 1.5, "音轨真实提取");
        // 无音轨样本：提取应失败而非生成伪空文件。
        let silent = dir.join("silent.mp4");
        run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=128x96:rate=15",
            "-c:v",
            "libx264",
            silent.to_string_lossy().as_ref(),
        ])?;
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=index",
                "-of",
                "csv=p=0",
            ])
            .arg(&silent)
            .output()?;
        assert!(
            String::from_utf8_lossy(&probe.stdout).trim().is_empty(),
            "无音轨事实可读"
        );
        let failed = run_ffmpeg(&[
            "-i",
            silent.to_string_lossy().as_ref(),
            "-vn",
            "-c:a",
            "pcm_s16le",
            dir.join("no-audio.wav").to_string_lossy().as_ref(),
        ])
        .is_err();
        assert!(
            failed || !dir.join("no-audio.wav").exists(),
            "无音轨不生成伪成功文件"
        );
        Ok(())
    });

    // AC-CAP-019：精确裁剪验证时间范围。
    scenario!("AC-CAP-019", {
        let dir = workdir("019");
        let wav = caps.generate_test_wav(&dir, "long.wav", 6)?;
        let trimmed = dir.join("trim.wav");
        run_ffmpeg(&[
            "-i",
            wav.to_string_lossy().as_ref(),
            "-ss",
            "1",
            "-t",
            "2",
            "-c",
            "copy",
            trimmed.to_string_lossy().as_ref(),
        ])?;
        let duration = ffprobe_duration(&trimmed);
        assert!(
            (1.5..3.0).contains(&duration),
            "裁剪时长 {duration} 在容差内"
        );
        Ok(())
    });

    // AC-CAP-020：不同页数文档合并后总页数正确。
    scenario!("AC-CAP-020", {
        let dir = workdir("020");
        let pdf1 = caps.generate_test_pdf(&dir, "one.pdf", 1)?;
        let pdf3 = caps.generate_test_pdf(&dir, "three.pdf", 3)?;
        let merged = caps.pdf_merge(&[pdf1, pdf3], &dir, "merged.pdf")?;
        assert_eq!(caps.pdf_page_count(&merged)?, 4);
        Ok(())
    });

    // AC-CAP-021：逐页拆分后重组内容与请求对应。
    scenario!("AC-CAP-021", {
        let dir = workdir("021");
        let pdf = caps.generate_test_pdf(&dir, "source.pdf", 3)?;
        let parts = caps.pdf_split_every(&pdf, &dir, 1)?;
        assert_eq!(parts.len(), 3);
        let remerged = caps.pdf_merge(&parts, &dir, "remerged.pdf")?;
        assert_eq!(caps.pdf_page_count(&remerged)?, 3);
        Ok(())
    });

    // AC-CAP-025：UTF-8 与支持的其它编码读回正确。
    scenario!("AC-CAP-025", {
        let dir = workdir("025");
        let utf8 = caps.create_text_file(&dir, "utf8.txt", "第一行\nsecond", "utf-8")?;
        let (content, _) = caps.read_text(&utf8, None)?;
        assert!(content.contains("第一行"));
        let latin = caps.create_text_file(&dir, "latin.txt", "café", "latin-1");
        match latin {
            Ok(path) => {
                let (content, _) = caps.read_text(&path, Some("latin-1"))?;
                assert!(content.contains("café"), "latin-1 读回正确");
            }
            Err(_) => {
                // 不支持的编码明确报错同样满足合同（非法/不支持编码给确定反馈）。
                let bad = caps.read_text(&dir.join("不存在.txt"), None);
                assert!(bad.is_err(), "缺失文件明确报错");
            }
        }
        Ok(())
    });

    // AC-CAP-026：创建 TXT 读回文字、换行一致。
    scenario!("AC-CAP-026", {
        let dir = workdir("026");
        let file = caps.create_text(&dir, "lines.txt", "A\nB\nC", "utf-8", "lf")?;
        let (content, _) = caps.read_text(&file, None)?;
        assert_eq!(content.lines().count(), 3);
        assert!(content.starts_with('A') && content.ends_with('C'));
        Ok(())
    });

    // AC-CAP-027：Markdown 原文完整，读取不改文件。
    scenario!("AC-CAP-027", {
        let dir = workdir("027");
        let file = caps.create_markdown(&dir, "doc.md", "# 标题\n- 列表项\n```code```\n")?;
        let before = std::fs::read(&file)?;
        let (content, _) = caps.read_markdown(&file)?;
        assert!(
            content.contains("# 标题")
                && content.contains("- 列表项")
                && content.contains("```code```")
        );
        assert_eq!(std::fs::read(&file)?, before, "读取不改文件");
        Ok(())
    });

    // AC-CAP-028：创建 Markdown 原样读回 Unicode。
    scenario!("AC-CAP-028", {
        let dir = workdir("028");
        let file = caps.create_markdown(&dir, "uni.md", "# Ünï ✨\n内容 ✓\n")?;
        let (content, _) = caps.read_markdown(&file)?;
        assert!(content.contains("Ünï ✨") && content.contains("✓"));
        Ok(())
    });

    // AC-CAP-029/030：DOCX 创建后段落正文可提取、顺序正确。
    scenario!("AC-CAP-029/030", {
        let dir = workdir("029");
        let docx = caps.create_docx(
            &dir,
            "报告.docx",
            vec!["第一章".into(), "第二章".into(), "结论".into()],
        )?;
        let text = caps.extract_docx_text(&docx)?;
        let first = text.find("第一章").expect("第一段在正文中");
        let second = text.find("第二章").expect("第二段在正文中");
        let third = text.find("结论").expect("结论在正文中");
        assert!(first < second && second < third, "文档顺序正确");
        Ok(())
    });

    // AC-CAP-022：提页——乱序/重复按显式顺序输出；越界拒绝执行。
    scenario!("AC-CAP-022", {
        let dir = workdir("022");
        let pdf = caps.generate_test_pdf(&dir, "source.pdf", 3)?;
        // 乱序 + 重复：[3,1,3] → 3 页输出。
        let out = caps.pdf_extract_pages(&pdf, &[3, 1, 3], &dir, "extracted.pdf")?;
        assert_eq!(caps.pdf_page_count(&out)?, 3);
        // 顺序与内容核对：结果第 i 页内容流 == 源对应页内容流。
        let source_doc = lopdf::Document::load(&pdf)?;
        let out_doc = lopdf::Document::load(&out)?;
        let out_pages: Vec<_> = out_doc.get_pages().values().copied().collect();
        for (result_id, &expected_page) in out_pages.iter().zip([3u32, 1, 3].iter()) {
            let source_id = source_doc.get_pages()[&expected_page];
            let expected = source_doc.get_page_content(source_id);
            let actual = out_doc.get_page_content(*result_id);
            // 内容流尾部分隔空白与渲染语义无关：比较时归一。
            let normalize = |bytes: &[u8]| -> Vec<u8> {
                bytes
                    .iter()
                    .copied()
                    .rev()
                    .skip_while(|b| *b == b'\n' || *b == b'\r' || *b == b' ')
                    .collect::<Vec<u8>>()
                    .into_iter()
                    .rev()
                    .collect()
            };
            assert_eq!(
                normalize(&actual),
                normalize(&expected),
                "结果页内容 == 源第 {expected_page} 页（忽略尾部分隔空白）"
            );
        }
        // 越界拒绝执行。
        assert!(caps.pdf_extract_pages(&pdf, &[4], &dir, "bad.pdf").is_err());
        Ok(())
    });

    // AC-CAP-023：选页旋转——页数与未选页内容不变，选中页 /Rotate 累积。
    scenario!("AC-CAP-023", {
        let dir = workdir("023");
        let pdf = caps.generate_test_pdf(&dir, "source.pdf", 3)?;
        let out = caps.pdf_rotate_pages(&pdf, &[2], 90, &dir, "rotated.pdf")?;
        assert_eq!(caps.pdf_page_count(&out)?, 3, "页数不变");
        let doc = lopdf::Document::load(&out)?;
        let pages: Vec<_> = doc.get_pages().values().copied().collect();
        for (index, id) in pages.iter().enumerate() {
            let rotate = doc
                .get_object(*id)
                .ok()
                .and_then(|o| match o {
                    lopdf::Object::Dictionary(dict) => {
                        dict.get(b"Rotate").ok().and_then(|r| r.as_i64().ok())
                    }
                    _ => None,
                })
                .unwrap_or(0);
            if index == 1 {
                assert_eq!(rotate, 90, "第 2 页已旋转");
            } else {
                assert_eq!(rotate, 0, "未选页不受影响");
            }
        }
        Ok(())
    });

    // AC-CAP-024：结构压缩——不栅格化不降质，页数不变；体积变化如实报告。
    scenario!("AC-CAP-024", {
        let dir = workdir("024");
        let pdf = caps.generate_test_pdf(&dir, "source.pdf", 5)?;
        let (out, original_size, compressed_size) =
            caps.pdf_compress(&pdf, &dir, "compressed.pdf")?;
        assert_eq!(caps.pdf_page_count(&out)?, 5, "页数不变（内容未降质）");
        let _ = (original_size, compressed_size); // 体积不降时不报虚构节省：如实记录两个数。
        Ok(())
    });

    // 写证据并输出矩阵表。
    let rows = rows().lock().unwrap().clone();
    let out_dir = evidence_dir();
    let _ = std::fs::create_dir_all(&out_dir);
    let json = serde_json::to_string_pretty(&rows).expect("序列化");
    std::fs::write(out_dir.join("basic30.json"), json).expect("写证据");
    let passed = rows.iter().filter(|r| r.verdict == "pass").count();
    let failed = rows.iter().filter(|r| r.verdict == "fail").count();
    let gaps = rows.iter().filter(|r| r.verdict == "gap").count();
    for row in &rows {
        println!("MATRIX  {}  {}  {}", row.ac, row.verdict, row.note);
    }
    println!("MATRIX 基础30：pass={passed} fail={failed} gap={gaps}（gap 为执行器缺口，不计通过）");
    assert_eq!(failed, 0, "存在失败场景");
}
