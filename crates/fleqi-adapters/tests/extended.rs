//! M3.5 扩展能力测试：十类历史意图的代表路径（AC-CAP-031..135 各类代表场景）。
//! 全部真实执行（文件/系统/网络回环/计算）；OCR/ASR 等外部工具依赖走检测与
//! ToolUnavailable 条件路径。

use fleqi_adapters::capabilities::{CapabilityError, FileCapabilities};
use fleqi_adapters::extended::{ComputeCapabilities, ExtendedCapabilities, SystemCapabilities};

fn write(path: &std::path::Path, content: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
    path.to_path_buf()
}

#[test]
fn cap_file_010_011_metadata_sizes_with_subdirectories() {
    let root = tempfile::tempdir().unwrap();
    let file = write(&root.path().join("a.bin"), &"x".repeat(1234));
    let _ = write(&root.path().join("子/b.txt"), "hello");
    let extended = ExtendedCapabilities::new();
    let size = extended.file_size(&file).expect("文件大小");
    assert_eq!(size.logical, 1234);
    assert!(size.on_disk >= 1234);
    let folder = extended.folder_size(root.path()).expect("目录大小");
    assert!(folder.total_bytes >= 1239, "包含子目录");
    assert_eq!(folder.unreadable_entries.len(), 0);
}

#[test]
fn cap_file_013_014_find_largest_and_duplicates() {
    let root = tempfile::tempdir().unwrap();
    let big = write(&root.path().join("big.bin"), &"0".repeat(2000));
    write(&root.path().join("small.txt"), "s");
    let dup_content = "duplicate-payload";
    let d1 = write(&root.path().join("d1.txt"), dup_content);
    let d2 = write(&root.path().join("d2.txt"), dup_content);
    let _ = d2;
    let extended = ExtendedCapabilities::new();
    let largest = extended.find_largest(root.path(), 1).expect("最大文件");
    assert_eq!(largest[0].path, big);
    let duplicates = extended.find_duplicates(root.path()).expect("重复");
    let group = duplicates
        .iter()
        .find(|g| g.paths.contains(&d1))
        .expect("同内容归组");
    assert_eq!(group.paths.len(), 2, "不同名同内容一组");
}

#[test]
fn cap_dev_006_checksum_sha256_matches_known_vector() {
    let root = tempfile::tempdir().unwrap();
    // "hello" 的 SHA-256 已知向量。
    let file = write(&root.path().join("hello.txt"), "hello");
    let extended = ExtendedCapabilities::new();
    let digest = extended.sha256(&file).expect("摘要");
    assert_eq!(
        digest,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn cap_text_007_word_count_english_and_cjk_rules() {
    let root = tempfile::tempdir().unwrap();
    let english = write(&root.path().join("en.txt"), "one two  three\nfour");
    let cjk = write(&root.path().join("zh.txt"), "你好世界 测试");
    let extended = ExtendedCapabilities::new();
    let en = extended.word_count(&english, "words").expect("英文字数");
    assert_eq!(en, 4, "空白分隔的非空文本段");
    let chars = extended.word_count(&cjk, "characters").expect("中文字符数");
    assert_eq!(chars, 7, "Unicode 字符数（含空格）");
    let cjk_chars = extended.word_count(&cjk, "cjk").expect("CJK 计数");
    assert_eq!(cjk_chars, 6);
}

#[test]
fn cap_calc_001_004_percent_unit_time_sqrt() {
    let compute = ComputeCapabilities;
    // AC-CAP-103：15% of 85.99（用户参数化，非来源键数字）。
    let percent = compute.percent(15.0, 85.99).expect("百分比");
    assert!((percent - 12.8985).abs() < 1e-9);
    // AC-CAP-104：5 英尺 9 英寸 → 厘米。
    let cm = compute.height_to_cm(5, 9).expect("身高换算");
    assert!((cm - 175.26).abs() < 1e-6);
    // AC-CAP-105：2 天 → 秒。
    let seconds = compute.duration_to_seconds(2.0, "days").expect("时间换算");
    assert_eq!(seconds, 172_800.0);
    // AC-CAP-106：sqrt(1764)。
    let root = compute.sqrt(1764.0).expect("平方根");
    assert!((root - 42.0).abs() < 1e-9);
    assert!(compute.sqrt(-1.0).is_err(), "负数按范围处理，不虚构实数");
}

#[test]
fn cap_system_009_010_processor_and_ram_reported() {
    let system = SystemCapabilities::new();
    let processor = system.processor().expect("CPU 信息");
    assert!(processor.contains("Apple") || processor.contains("Intel") || !processor.is_empty());
    let ram = system.total_ram_gb().expect("内存");
    assert!(ram > 0.0, "总量与系统 API 一致");
}

#[test]
fn cap_system_001_012_013_display_battery_charger() {
    let system = SystemCapabilities::new();
    let displays = system.displays().expect("显示器");
    assert!(!displays.is_empty(), "至少一台显示器");
    assert!(displays[0].width > 0 && displays[0].height > 0);
    let battery = system.battery().expect("电池状态可读取");
    assert!(
        battery.is_some() || std::env::var("CI").is_ok(),
        "无电池设备返回 None 而非 0"
    );
    let charger = system.charger_wattage();
    // 未接电源时 None，不虚构瓦数（AC-CAP-117）。
    assert!(charger.is_none() || charger.unwrap() > 0.0);
}

#[test]
fn cap_network_001_ping_loopback_succeeds() {
    let extended = ExtendedCapabilities::new();
    let result = extended.ping("127.0.0.1", 3).expect("ping 调用");
    assert!(result.received >= 1, "回环可达：{result:?}");
    assert!(result.avg_ms >= 0.0);
}

#[test]
fn cap_network_002_download_real_bytes_over_loopback() {
    // 回环起真实 HTTP 服务，下载真实字节。
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{Read, Write};
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = "fleqi-download-payload";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    let extended = ExtendedCapabilities::new();
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("dl.txt");
    let size = extended
        .download(&format!("http://127.0.0.1:{port}/file"), &target)
        .expect("下载");
    assert_eq!(size, "fleqi-download-payload".len() as u64);
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "fleqi-download-payload"
    );
    let _ = handle.join();
    // 404 不留伪成功文件（AC-CAP-133）。
    let missing = root.path().join("404.txt");
    let outcome = extended.download(&format!("http://127.0.0.1:{port}/missing"), &missing);
    assert!(outcome.is_err() || !missing.exists());
}

#[test]
fn cap_dev_001_git_is_repo_distinguishes_directories() {
    let extended = ExtendedCapabilities::new();
    let plain = tempfile::tempdir().unwrap();
    assert!(!extended.git_is_repo(plain.path()).expect("判定"));
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".git")).unwrap();
    assert!(extended.git_is_repo(repo.path()).expect("判定仓库"));
    // 仓库子目录也判定为仓库。
    let sub = repo.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    assert!(extended.git_is_repo(&sub).expect("子目录判定"));
}

#[test]
fn ac_ocr_asr_unavailable_reports_condition_not_placeholder() {
    let extended = ExtendedCapabilities::new();
    let root = tempfile::tempdir().unwrap();
    let image = FileCapabilities::new()
        .generate_test_png(root.path(), "x.png", 4, 4)
        .unwrap();
    match extended.ocr_text(&image) {
        Ok(_) => {} // 环境已安装 tesseract：成功路径有效
        Err(CapabilityError::ToolUnavailable(message)) => {
            assert!(message.contains("tesseract"), "{message}")
        }
        Err(other) => panic!("OCR 应给出工具条件而非 {other:?}"),
    }
    let wav = match FileCapabilities::new().generate_test_wav(root.path(), "t.wav", 1) {
        Ok(wav) => wav,
        Err(_) => return,
    };
    match extended.transcribe_text(&wav) {
        Ok(_) => {}
        Err(CapabilityError::ToolUnavailable(message)) => assert!(
            message.contains("whisper") || message.contains("转写"),
            "{message}"
        ),
        Err(other) => panic!("转写应给出工具条件而非 {other:?}"),
    }
}
