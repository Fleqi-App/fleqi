use fleqi_adapters::{capabilities::FileCapabilities, file_operations};
use fleqi_application::run_service::NativeOutput;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};
fn run(
    id: &str,
    paths: &[PathBuf],
    cwd: &Path,
    params: &[(&str, &str)],
) -> Result<NativeOutput, String> {
    file_operations::execute(
        id,
        paths,
        cwd,
        &params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        &AtomicBool::new(false),
    )
}
fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}
fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn init(root: &Path) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Fleqi Test"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "core.hooksPath", "/dev/null"]);
}
#[test]
fn content_magic_ignores_a_false_extension_and_recognizes_binary() {
    let dir = tempfile::tempdir().unwrap();
    let text = write(dir.path(), "misleading.png", b"plain text\n");
    let binary = write(dir.path(), "binary.txt", [0u8; 128]);
    let result = run("CAP-FILE-009", &[text, binary], dir.path(), &[]).unwrap();
    assert!(!result.partial);
    assert!(result.output.contains("text/plain"));
    assert!(result.output.contains("不匹配"));
    assert!(result.output.contains("application/octet-stream"));
}
#[test]
fn deep_scans_sort_group_and_report_missing_roots_without_following_links() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.bin", b"same");
    write(dir.path(), "sub/b.bin", b"same");
    write(dir.path(), "sub/deep/large.bin", [7u8; 100]);
    let external = tempfile::tempdir().unwrap();
    write(external.path(), "outside.mp4", b"outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(external.path(), dir.path().join("link")).unwrap();
    let largest = run("CAP-FILE-013", &[], dir.path(), &[("limit", "1")]).unwrap();
    assert!(largest.output.contains("large.bin"));
    assert!(!largest.output.contains("outside.mp4"));
    let duplicate = run("CAP-FILE-014", &[], dir.path(), &[]).unwrap();
    assert!(duplicate.output.contains("重复组共 1 组"));
    assert!(duplicate.output.contains("a.bin"));
    assert!(duplicate.output.contains("b.bin"));
    let size = run(
        "CAP-FILE-011",
        &[dir.path().to_owned(), dir.path().join("missing")],
        dir.path(),
        &[],
    )
    .unwrap();
    assert!(size.partial);
    assert!(size.output.contains("108 bytes"));
    assert!(size.output.contains("missing"));
}
#[test]
fn sparse_file_reports_logical_and_allocated_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse");
    fs::File::create(&path)
        .unwrap()
        .set_len(1024 * 1024 * 16)
        .unwrap();
    let result = run("CAP-FILE-010", &[path], dir.path(), &[]).unwrap();
    assert!(result.output.contains("16777216 bytes"));
    assert!(result.output.contains("占用空间"));
}
#[test]
fn shallow_and_recursive_extension_search_have_distinct_scopes() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "first.MP4", b"a");
    write(dir.path(), "sub/deep.mp4", b"b");
    write(dir.path(), ".hidden.mp4", b"c");
    let shallow = run("CAP-FILE-017", &[], dir.path(), &[("action", "list")]).unwrap();
    assert!(shallow.output.contains("first.MP4"));
    assert!(!shallow.output.contains("deep.mp4"));
    assert!(!shallow.output.contains(".hidden.mp4"));
    let deep = run("CAP-FILE-018", &[], dir.path(), &[("action", "list")]).unwrap();
    assert!(deep.output.contains("deep.mp4"));
    assert!(deep.output.contains("命中 2 个"));
}
#[test]
fn pdf_content_search_uses_body_and_reports_broken_files() {
    let dir = tempfile::tempdir().unwrap();
    let file = FileCapabilities::new()
        .generate_test_pdf(dir.path(), "fixture.pdf", 1)
        .unwrap();
    let mut pdf = lopdf::Document::load(&file).unwrap();
    let page = *pdf.get_pages().values().next().unwrap();
    let content = lopdf::content::Content {
        operations: vec![
            lopdf::content::Operation::new("BT", vec![]),
            lopdf::content::Operation::new(
                "Tf",
                vec![lopdf::Object::Name(b"F1".to_vec()), 12.into()],
            ),
            lopdf::content::Operation::new(
                "Tj",
                vec![lopdf::Object::string_literal(
                    "Apple orchard unique fixture",
                )],
            ),
            lopdf::content::Operation::new("ET", vec![]),
        ],
    };
    let stream = pdf.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        content.encode().unwrap(),
    ));
    pdf.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    pdf.save(&file).unwrap();
    assert!(
        fleqi_adapters::pdf_operations::text(&file, "", &AtomicBool::new(false))
            .unwrap()
            .contains("Apple")
    );
    write(dir.path(), "sub/broken.pdf", b"bad PDF");
    let result = run(
        "CAP-FILE-020",
        &[],
        dir.path(),
        &[("keyword", "apple"), ("action", "list"), ("ocr", "never")],
    )
    .unwrap();
    assert!(result.partial);
    assert!(result.output.contains("fixture.pdf"));
    assert!(result.output.contains("broken.pdf"));
    assert!(result.output.contains("命中 1 个"));
    let sensitive = run(
        "CAP-FILE-019",
        &[file],
        dir.path(),
        &[
            ("keyword", "apple"),
            ("caseSensitive", "true"),
            ("action", "list"),
            ("ocr", "never"),
        ],
    )
    .unwrap();
    assert!(sensitive.output.contains("命中 0 个"));
}
#[test]
fn code_count_excludes_dependencies_and_accepts_different_extensions() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.js", b"a\n\nb");
    write(dir.path(), "src/b.JSX", b"one\n");
    write(dir.path(), "node_modules/x.js", b"exclude\n");
    write(dir.path(), ".git/x.js", b"exclude\n");
    write(dir.path(), "src/code.rs", b"one\ntwo\n");
    let js = run("CAP-DEV-005", &[], dir.path(), &[]).unwrap();
    assert!(js.output.contains("共 2 文件，4 行"));
    let rust = run("CAP-DEV-005", &[], dir.path(), &[("extensions", "rs")]).unwrap();
    assert!(rust.output.contains("共 1 文件，2 行"));
    let nonblank = run("CAP-DEV-005", &[], dir.path(), &[("countKind", "nonBlank")]).unwrap();
    assert!(nonblank.output.contains("共 2 文件，3 行"));
}
#[test]
fn checksum_reads_beyond_process_output_limits_and_matches_known_vector() {
    let dir = tempfile::tempdir().unwrap();
    let hello = write(dir.path(), "hello", b"hello");
    let result = run("CAP-DEV-006", &[hello], dir.path(), &[]).unwrap();
    assert!(
        result
            .output
            .starts_with("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );
    let large = write(dir.path(), "large", vec![0x61; 9 * 1024 * 1024]);
    let output = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(&large)
        .output()
        .unwrap();
    let expected = String::from_utf8(output.stdout).unwrap();
    let result = run("CAP-DEV-006", &[large], dir.path(), &[]).unwrap();
    assert_eq!(result.output.trim(), expected.trim());
}
#[test]
fn git_switch_commit_push_pull_and_push_failure_preserve_real_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("work space ' repo");
    let remote = dir.path().join("remote.git");
    let clone = dir.path().join("clone");
    init(&repo);
    git(dir.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    write(&repo, "first.txt", b"first");
    let result = run(
        "CAP-DEV-004",
        &[],
        &repo,
        &[
            ("scope", "all"),
            ("message", "first fixture"),
            ("branch", "main"),
        ],
    )
    .unwrap();
    assert!(result.output.contains("阶段 3：推送成功"));
    git(
        dir.path(),
        &[
            "clone",
            "--branch",
            "main",
            remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    git(&clone, &["config", "user.name", "Fleqi Test"]);
    git(&clone, &["config", "user.email", "fixture@example.invalid"]);
    git(&clone, &["config", "commit.gpgsign", "false"]);
    git(&clone, &["config", "core.hooksPath", "/dev/null"]);
    write(&clone, "new.txt", b"new");
    git(&clone, &["add", "."]);
    git(&clone, &["commit", "-m", "remote fixture"]);
    git(&clone, &["push"]);
    let pulled = run("CAP-DEV-002", &[], &repo, &[("branch", "main")]).unwrap();
    assert!(pulled.output.contains("拉取完成"));
    assert_eq!(fs::read(repo.join("new.txt")).unwrap(), b"new");
    run(
        "CAP-DEV-003",
        &[],
        &repo,
        &[("branch", "feature"), ("create", "true")],
    )
    .unwrap();
    assert_eq!(git(&repo, &["branch", "--show-current"]).trim(), "feature");
    write(&repo, "third.txt", b"third");
    let failed = run(
        "CAP-DEV-004",
        &[],
        &repo,
        &[
            ("scope", "all"),
            ("message", "preserved local commit"),
            ("remote", "missing"),
            ("branch", "feature"),
        ],
    )
    .err()
    .unwrap();
    assert!(failed.contains("阶段 2：提交成功"));
    assert!(failed.contains("本地提交已保留"));
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%s"]).trim(),
        "preserved local commit"
    );
    let ordinary = run("CAP-DEV-001", &[], dir.path(), &[]).unwrap();
    assert!(ordinary.output.contains("不是 Git 仓库"));
    fs::create_dir_all(repo.join("deep/sub")).unwrap();
    assert!(
        run("CAP-DEV-001", &[], &repo.join("deep/sub"), &[])
            .unwrap()
            .output
            .contains("是 Git 工作区")
    );
}
#[test]
fn git_all_from_subdirectory_stages_root_changes_and_deletions() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init(&repo);
    write(&repo, "old", b"old");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    fs::remove_file(repo.join("old")).unwrap();
    write(&repo, "new", b"new");
    fs::create_dir(repo.join("sub")).unwrap();
    let result = run(
        "CAP-DEV-004",
        &[],
        &repo.join("sub"),
        &[
            ("scope", "all"),
            ("message", "all fixture"),
            ("remote", "missing"),
        ],
    )
    .err()
    .unwrap();
    assert!(result.contains("提交成功"));
    assert!(git(&repo, &["status", "--porcelain"]).is_empty());
    let files = git(&repo, &["ls-tree", "--name-only", "HEAD"]);
    assert!(!files.contains("old"));
    assert!(files.contains("new"));
}
#[test]
fn zip_two_stages_preserve_source_intent_and_unique_names() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "name ' Unicode 文.txt", b"known payload");
    run(
        "CAP-ZIP-005",
        std::slice::from_ref(&source),
        dir.path(),
        &[
            ("destination", "collected"),
            ("name", "bundle.zip"),
            ("sourceIntent", "copy"),
        ],
    )
    .unwrap();
    assert!(source.exists());
    let mut archive =
        zip::ZipArchive::new(fs::File::open(dir.path().join("collected/bundle.zip")).unwrap())
            .unwrap();
    let mut body = String::new();
    archive
        .by_index(0)
        .unwrap()
        .read_to_string(&mut body)
        .unwrap();
    assert_eq!(body, "known payload");
    let moved = run(
        "CAP-ZIP-005",
        std::slice::from_ref(&source),
        dir.path(),
        &[
            ("destination", "collected"),
            ("name", "bundle.zip"),
            ("sourceIntent", "move"),
        ],
    )
    .unwrap();
    assert!(!source.exists());
    assert!(moved.output.contains("阶段 1：已移动"));
    assert_eq!(
        fs::read_dir(dir.path().join("collected")).unwrap().count(),
        4
    );
}
#[cfg(unix)]
#[test]
fn zip_failure_keeps_moved_directory_and_only_copy_of_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    write(&source, "precious.txt", b"only copy");
    std::os::unix::fs::symlink("precious.txt", source.join("link")).unwrap();
    let result = run(
        "CAP-ZIP-005",
        std::slice::from_ref(&source),
        dir.path(),
        &[("destination", "collected"), ("sourceIntent", "move")],
    )
    .err()
    .unwrap();
    assert!(result.contains("阶段 1：已移动"));
    assert!(result.contains("阶段 2：压缩失败"));
    assert!(!source.exists());
    assert_eq!(
        fs::read(dir.path().join("collected/source/precious.txt")).unwrap(),
        b"only copy"
    );
    assert!(!dir.path().join("collected/archive.zip").exists());
}
#[test]
fn zip_ratio_uses_entry_sizes_and_handles_empty_denominator() {
    let dir = tempfile::tempdir().unwrap();
    let empty = write(dir.path(), "empty", b"");
    let archive = dir.path().join("empty.zip");
    FileCapabilities::new()
        .zip_create(&[empty], &archive)
        .unwrap();
    let result = run(
        "CAP-ZIP-004",
        &[archive],
        dir.path(),
        &[("scope", "entries")],
    )
    .unwrap();
    assert!(result.output.contains("分母为 0"));
    let repeated = write(dir.path(), "repeat", vec![b'a'; 10000]);
    let archive = dir.path().join("repeat.zip");
    FileCapabilities::new()
        .zip_create(&[repeated], &archive)
        .unwrap();
    let result = run("CAP-ZIP-004", &[archive], dir.path(), &[]).unwrap();
    assert!(result.output.contains("原大小=10000 bytes"));
    assert!(result.output.contains("节省比例=(1−压缩后/原大小)×100"));
}
#[test]
fn classification_uses_selected_date_and_does_not_fallback_missing_photo_date() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "dated.txt", b"date fixture");
    let file = fs::OpenOptions::new().write(true).open(&source).unwrap();
    let stamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(86400);
    file.set_times(fs::FileTimes::new().set_modified(stamp))
        .unwrap();
    let result = run(
        "CAP-FILE-007",
        &[source],
        dir.path(),
        &[
            ("groupBy", "date"),
            ("dateSource", "modified"),
            ("timezone", "-01:00"),
            ("directoryFormat", "YYYY-MM-DD"),
        ],
    )
    .unwrap();
    assert!(!result.partial);
    assert_eq!(
        fs::read(dir.path().join("1970-01-01/dated.txt")).unwrap(),
        b"date fixture"
    );
    let photo = write(dir.path(), "no-exif.txt", b"not photo");
    let result = run(
        "CAP-FILE-007",
        std::slice::from_ref(&photo),
        dir.path(),
        &[("groupBy", "date"), ("dateSource", "taken")],
    )
    .unwrap();
    assert!(result.partial);
    assert!(photo.exists());
}
#[test]
fn calculations_use_decimal_rounding_units_and_explicit_complex_domain() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        run(
            "CAP-CALC-001",
            &[],
            dir.path(),
            &[("percent", "15"), ("base", "85.99")]
        )
        .unwrap()
        .output
        .contains("12.8985")
    );
    for (mode, expected) in [
        ("nearest", "-0.13"),
        ("floor", "-0.13"),
        ("ceil", "-0.12"),
        ("truncate", "-0.12"),
    ] {
        let result = run(
            "CAP-CALC-001",
            &[],
            dir.path(),
            &[
                ("percent", "-12.5"),
                ("base", "1"),
                ("precision", "2"),
                ("rounding", mode),
            ],
        )
        .unwrap();
        assert!(result.output.contains(expected), "{}", result.output);
    }
    assert!(
        run(
            "CAP-CALC-002",
            &[],
            dir.path(),
            &[("feet", "5"), ("inches", "11"), ("unit", "cm")]
        )
        .unwrap()
        .output
        .contains("180.3400")
    );
    assert!(
        run(
            "CAP-CALC-003",
            &[],
            dir.path(),
            &[("amount", "48"), ("from", "hours"), ("to", "days")]
        )
        .unwrap()
        .output
        .contains("2.0000")
    );
    assert!(run("CAP-CALC-004", &[], dir.path(), &[("value", "-9")]).is_err());
    assert!(
        run(
            "CAP-CALC-004",
            &[],
            dir.path(),
            &[("value", "-9"), ("domain", "complex")]
        )
        .unwrap()
        .output
        .contains("3.0000i")
    );
    assert!(run("CAP-CALC-004", &[], dir.path(), &[("value", "NaN")]).is_err());
}
#[cfg(target_os = "macos")]
#[test]
fn quarantine_removes_only_requested_attribute_without_recursing() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "download.txt", b"download");
    for (key, value) in [
        ("com.apple.quarantine", "0081;5e0be100;Fleqi;"),
        ("user.fleqi.fixture", "preserve"),
    ] {
        let status = Command::new("/usr/bin/xattr")
            .args(["-w", key, value])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let metadata = run(
        "CAP-FILE-015",
        std::slice::from_ref(&source),
        dir.path(),
        &[],
    )
    .unwrap();
    assert!(metadata.output.contains("隔离记录时间"));
    assert!(metadata.output.contains("来源 URL：未记录"));
    run(
        "CAP-DEV-007",
        std::slice::from_ref(&source),
        dir.path(),
        &[],
    )
    .unwrap();
    let attrs = Command::new("/usr/bin/xattr")
        .arg(&source)
        .output()
        .unwrap();
    let attrs = String::from_utf8(attrs.stdout).unwrap();
    assert!(attrs.contains("user.fleqi.fixture"));
    assert!(!attrs.contains("com.apple.quarantine"));
    let absent = run("CAP-DEV-007", &[source], dir.path(), &[]).unwrap();
    assert!(absent.output.contains("不存在，未修改"));
}
#[test]
fn brew_detection_and_readonly_inventory_do_not_modify_packages() {
    let dir = tempfile::tempdir().unwrap();
    let status = run("CAP-TOOLS-001", &[], dir.path(), &[]).unwrap();
    if status.output.contains("已检测到") {
        let inventory = run("CAP-TOOLS-002", &[], dir.path(), &[]).unwrap();
        assert!(inventory.output.contains("Homebrew 已安装清单"));
    } else {
        assert!(status.output.contains("未执行安装"));
    }
    assert!(run("CAP-TOOLS-003", &[], dir.path(), &[("package", "--force")]).is_err());
    assert!(
        run(
            "CAP-TOOLS-004",
            &[],
            dir.path(),
            &[("package", "https://example.invalid/package")]
        )
        .is_err()
    );
}
#[test]
fn cancellation_prevents_any_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "source", b"untouched");
    let result = file_operations::execute(
        "CAP-ZIP-005",
        std::slice::from_ref(&source),
        dir.path(),
        &BTreeMap::from([("destination".into(), "target".into())]),
        &AtomicBool::new(true),
    );
    assert!(result.is_err());
    assert!(source.exists());
    assert!(!dir.path().join("target").exists());
}

#[test]
fn rename_dates_cover_multiextension_extensionless_and_timezones() {
    let dir = tempfile::tempdir().unwrap();
    let first = write(dir.path(), "archive.tar.gz", b"archive");
    let second = write(dir.path(), "README", b"readme");
    for source in [&first, &second] {
        fs::OpenOptions::new()
            .write(true)
            .open(source)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(86400)),
            )
            .unwrap();
    }
    let west = run(
        "CAP-FILE-005",
        &[first],
        dir.path(),
        &[
            ("mode", "date"),
            ("dateSource", "modified"),
            ("timezone", "-01:00"),
            ("extensionRule", "all"),
        ],
    )
    .unwrap();
    assert!(west.output.contains("archive-1970-01-01.tar.gz"));
    assert!(dir.path().join("archive-1970-01-01.tar.gz").exists());
    run(
        "CAP-FILE-005",
        &[second],
        dir.path(),
        &[
            ("mode", "date"),
            ("dateSource", "modified"),
            ("timezone", "+08:00"),
        ],
    )
    .unwrap();
    assert!(dir.path().join("README-1970-01-02").exists());
}
#[test]
fn rename_affixes_preserve_extensions_and_number_order_is_parameterized() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "photo.jpg", b"a");
    run(
        "CAP-FILE-005",
        &[source],
        dir.path(),
        &[("mode", "affix"), ("suffix", "-reviewed")],
    )
    .unwrap();
    assert!(dir.path().join("photo-reviewed.jpg").exists());
    let alpha = write(dir.path(), "alpha.txt", b"a");
    let beta = write(dir.path(), "beta.txt", b"b");
    run(
        "CAP-FILE-006",
        &[beta.clone(), alpha.clone()],
        dir.path(),
        &[
            ("sort", "nameAsc"),
            ("width", "2"),
            ("position", "suffix"),
            ("start", "2"),
            ("step", "3"),
        ],
    )
    .unwrap();
    assert!(dir.path().join("alpha-02.txt").exists());
    assert!(dir.path().join("beta-05.txt").exists());
    let other = tempfile::tempdir().unwrap();
    let alpha = write(other.path(), "alpha", b"a");
    let beta = write(other.path(), "beta", b"b");
    run(
        "CAP-FILE-006",
        &[alpha, beta],
        other.path(),
        &[("sort", "nameDesc"), ("width", "4"), ("position", "suffix")],
    )
    .unwrap();
    assert!(other.path().join("beta-0001").exists());
    assert!(other.path().join("alpha-0002").exists());
}
#[test]
fn rename_detects_case_collision_and_preserves_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let original = write(dir.path(), "ABC.txt", b"original");
    let protected = write(dir.path(), "abc (1).txt", b"protected");
    let result = run(
        "CAP-FILE-005",
        std::slice::from_ref(&original),
        dir.path(),
        &[("mode", "template"), ("template", "abc (1).txt")],
    )
    .unwrap();
    assert!(result.output.contains("abc (1) (1).txt"));
    assert_eq!(fs::read(protected).unwrap(), b"protected");
    let case_dir = tempfile::tempdir().unwrap();
    let selected = write(case_dir.path(), "One.TXT", b"first");
    let other = write(case_dir.path(), "one.txt", b"second");
    if fs::read_dir(case_dir.path()).unwrap().count() == 2 {
        let result = run(
            "CAP-FILE-005",
            std::slice::from_ref(&selected),
            case_dir.path(),
            &[("mode", "case"), ("caseScope", "name")],
        )
        .unwrap();
        assert!(result.partial);
        assert_eq!(fs::read(selected).unwrap(), b"first");
        assert_eq!(fs::read(other).unwrap(), b"second");
    } else {
        let result = run(
            "CAP-FILE-005",
            std::slice::from_ref(&selected),
            case_dir.path(),
            &[("mode", "case"), ("caseScope", "name")],
        )
        .unwrap();
        assert!(!result.partial);
        assert_eq!(fs::read(other).unwrap(), b"second");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn source_url_is_read_from_actual_binary_plist_attribute() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "download.pdf", b"fixture");
    let plist=write(dir.path(),"source.plist",br#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><array><string>https://example.invalid/original.pdf?x=1&amp;y=2</string></array></plist>"#);
    assert!(
        Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1"])
            .arg(&plist)
            .status()
            .unwrap()
            .success()
    );
    let hex = fs::read(plist)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert!(
        Command::new("/usr/bin/xattr")
            .args(["-wx", "com.apple.metadata:kMDItemWhereFroms", &hex])
            .arg(&source)
            .status()
            .unwrap()
            .success()
    );
    let result = run("CAP-FILE-015", &[source], dir.path(), &[]).unwrap();
    assert!(!result.partial);
    assert!(
        result
            .output
            .contains("https://example.invalid/original.pdf?x=1&y=2")
    );
}
#[test]
fn git_pull_conflict_preserves_both_sides_and_reports_actual_failure() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let remote = dir.path().join("remote.git");
    let other = dir.path().join("other");
    init(&repo);
    git(dir.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    write(&repo, "shared.txt", b"base\n");
    run(
        "CAP-DEV-004",
        &[],
        &repo,
        &[("scope", "all"), ("message", "base")],
    )
    .unwrap();
    git(
        dir.path(),
        &[
            "clone",
            "-b",
            "main",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    for (key, value) in [
        ("user.name", "Fleqi Test"),
        ("user.email", "fixture@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        git(&other, &["config", key, value]);
    }
    write(&other, "shared.txt", b"remote side\n");
    git(&other, &["add", "."]);
    git(&other, &["commit", "-m", "remote"]);
    git(&other, &["push"]);
    write(&repo, "shared.txt", b"local side\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "local"]);
    let failed = run(
        "CAP-DEV-002",
        &[],
        &repo,
        &[("branch", "main"), ("strategy", "merge")],
    )
    .err()
    .unwrap();
    assert!(failed.contains("拉取失败"));
    assert!(failed.contains("未自动重置"));
    let conflict = fs::read_to_string(repo.join("shared.txt")).unwrap();
    assert!(conflict.contains("local side") && conflict.contains("remote side"));
    assert!(git(&repo, &["status", "--porcelain"]).contains("UU shared.txt"));
    assert!(
        run("CAP-DEV-001", &[], &remote, &[])
            .unwrap()
            .output
            .contains("裸仓库")
    );
}
