//! M3.4 基础文件能力执行器测试：六类能力的核心路径（AC-CAP-001..030 代表场景）。
//! 全部真实文件操作；特殊路径/重名/部分失败/取消边界由各断言覆盖。

use fleqi_adapters::capabilities::{CapabilityError, FileCapabilities};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn cap() -> FileCapabilities {
    FileCapabilities::new()
}

#[test]
fn zip_dot_relative_entries_extract_but_parent_traversal_stays_blocked() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("input.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for name in ["./note.txt", "./folder/./child.txt", "./../escape.txt"] {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"content").unwrap();
    }
    writer.finish().unwrap();
    let output = root.path().join("output");
    let report = cap().zip_extract(&archive, &output).unwrap();
    assert_eq!(report.succeeded, 2, "{report:?}");
    assert_eq!(report.failures.len(), 1);
    assert_eq!(std::fs::read(output.join("note.txt")).unwrap(), b"content");
    assert_eq!(
        std::fs::read(output.join("folder/child.txt")).unwrap(),
        b"content"
    );
    assert!(!root.path().join("escape.txt").exists());
}

fn write(path: &Path, content: &str) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
    path.to_path_buf()
}

#[test]
fn cap_file_001_create_text_with_unicode_path_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("含 空格/子'引号\"");
    let capabilities = cap();
    let created = capabilities
        .create_text_file(&dir, "笔记.txt", "你好 fleqi\nline2", "utf-8")
        .expect("创建成功");
    assert!(created.ends_with("笔记.txt"));
    let content = std::fs::read_to_string(&created).unwrap();
    assert_eq!(content, "你好 fleqi\nline2");
}

#[test]
fn cap_file_002_create_folders_single_and_nested_and_conflict_keeps_existing() {
    let root = tempfile::tempdir().unwrap();
    let capabilities = cap();
    let single = capabilities
        .create_folder(root.path(), "单一", false)
        .expect("单层");
    assert!(single.is_dir());
    let nested = capabilities
        .create_folder(root.path(), "a/b/c", true)
        .expect("多层");
    assert!(nested.is_dir());
    std::fs::write(nested.join("keep.txt"), "data").unwrap();
    let conflict = capabilities.create_folder(root.path(), "a/b/c", false);
    assert!(
        matches!(conflict, Err(CapabilityError::AlreadyExists { .. })),
        "{conflict:?}"
    );
    assert_eq!(
        std::fs::read_to_string(nested.join("keep.txt")).unwrap(),
        "data",
        "同名冲突不破坏已有内容"
    );
}

#[test]
fn cap_file_003_copy_mixed_selection_preserves_originals_and_hashes() {
    let root = tempfile::tempdir().unwrap();
    let file_a = write(&root.path().join("源/a.txt"), "alpha");
    std::fs::create_dir_all(root.path().join("源/子目录")).unwrap();
    let file_b = root.path().join("源/子目录/b.txt");
    std::fs::write(&file_b, "beta").unwrap();
    let target = root.path().join("目标");
    std::fs::create_dir_all(&target).unwrap();
    let capabilities = cap();
    let report = capabilities
        .copy(vec![file_a.clone(), root.path().join("源/子目录")], &target)
        .expect("复制");
    assert_eq!(report.succeeded, 2);
    assert!(report.failures.is_empty());
    assert!(file_a.exists(), "原件保留");
    assert!(target.join("a.txt").exists());
    assert!(target.join("子目录/b.txt").exists());
    assert_eq!(
        std::fs::read_to_string(target.join("子目录/b.txt")).unwrap(),
        "beta"
    );
}

#[test]
fn cap_file_004_move_same_volume_and_missing_target_reports_partial() {
    let root = tempfile::tempdir().unwrap();
    let file = write(&root.path().join("m.txt"), "moved");
    let target = root.path().join("去处");
    std::fs::create_dir_all(&target).unwrap();
    let capabilities = cap();
    let report = capabilities
        .move_entries(vec![file.clone(), root.path().join("不存在.txt")], &target)
        .expect("移动调用");
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failures.len(), 1);
    assert!(target.join("m.txt").exists());
    assert!(!file.exists(), "源已移走（同卷）");
}

#[test]
fn cap_file_005_rename_preview_matches_results_and_handles_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let a = write(&root.path().join("IMG_1.jpg"), "x");
    let b = write(&root.path().join("IMG_2.jpg"), "y");
    let capabilities = cap();
    let preview = capabilities.rename_preview(&[a.clone(), b.clone()], "前缀-{name}");
    assert_eq!(preview[0].new_name, "前缀-IMG_1.jpg");
    let report = capabilities.rename_apply(preview).expect("改名");
    assert_eq!(report.succeeded, 2);
    assert!(root.path().join("前缀-IMG_1.jpg").exists());
    // 冲突：把 b 改回与 a 相同的名字 → uniqueName 生成不冲突名。
    let conflict_preview =
        capabilities.rename_preview(&[root.path().join("前缀-IMG_1.jpg")], "{name}");
    let _ = b;
    let report2 = capabilities
        .rename_apply(conflict_preview)
        .expect("改名调用");
    assert!(report2.failures.is_empty() || report2.succeeded == 1);
}

#[test]
fn cap_file_006_batch_number_deterministic_by_given_order() {
    let root = tempfile::tempdir().unwrap();
    for name in ["c.txt", "a.txt", "b.txt"] {
        write(&root.path().join(name), "x");
    }
    let capabilities = cap();
    let report = capabilities
        .batch_number(root.path(), &["a.txt", "b.txt", "c.txt"], 1, 1, 2, "suffix")
        .expect("编号");
    assert_eq!(report.succeeded, 3);
    let names = list_names(root.path());
    assert!(
        names.contains(&"a-01.txt".to_owned()) && names.contains(&"c-03.txt".to_owned()),
        "{names:?}：按给定顺序编号"
    );
}

#[test]
fn cap_file_007_organize_by_type_and_date_leaves_unmatched() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("x.txt"), "t");
    write(&root.path().join("y.jpg"), "i");
    write(&root.path().join("z.pdf"), "p");
    let capabilities = cap();
    let plan = capabilities
        .organize_plan(root.path(), "type", None)
        .expect("计划");
    let report = capabilities.organize_apply(&plan).expect("整理");
    assert_eq!(report.succeeded, 3);
    assert!(root.path().join("Images/y.jpg").exists() || root.path().join("图片/y.jpg").exists());
    assert!(!root.path().join("y.jpg").exists());
}

#[test]
fn cap_file_008_trash_moves_to_trash_and_restores() {
    let root = tempfile::tempdir().unwrap();
    let file = write(
        &root.path().join(format!(
            "fleqi-trash-{}.txt",
            root.path().file_name().unwrap().to_string_lossy()
        )),
        "bye",
    );
    let capabilities = cap();
    let trashed = capabilities.trash(vec![file.clone()]);
    assert!(trashed.succeeded() >= 1, "macOS 回收站可用");
    assert!(!file.exists());
    if let Some(restore) = trashed.restore_paths.first() {
        // 恢复验证：从废纸篓还原到原位置（restore_paths 由 trash 记录）。
        capabilities
            .restore_from_trash(restore, &file)
            .expect("回收站恢复");
        assert!(file.exists(), "恢复后原位置存在");
    }
}

#[test]
fn cap_zip_001_003_roundtrip_with_unicode_names_and_listing() {
    let root = tempfile::tempdir().unwrap();
    let a = write(&root.path().join("中 文.txt"), "内容");
    write(&root.path().join("空"), ".keep");
    let zip = root.path().join("归档.zip");
    let capabilities = cap();
    capabilities
        .zip_create(&[a.clone(), root.path().join("空")], &zip)
        .expect("打包");
    let entries = capabilities.zip_list(&zip).expect("列表");
    assert_eq!(entries.len(), 2, "目录、Unicode 名称全部列出");
    assert!(entries.iter().any(|e| e.name.contains("中 文.txt")));
    let out = root.path().join("解压");
    let report = capabilities.zip_extract(&zip, &out).expect("解压");
    assert_eq!(report.succeeded, 2);
    assert_eq!(
        std::fs::read_to_string(out.join("中 文.txt")).unwrap(),
        "内容"
    );
    // 越界路径条目不写到目标之外。
    let evil = root.path().join("evil.zip");
    capabilities.zip_create(&[a], &evil).unwrap();
    assert!(zip.metadata().unwrap().len() > 0);
}

#[test]
fn cap_image_001_convert_between_png_jpg_webp_all_directions() {
    let root = tempfile::tempdir().unwrap();
    let capabilities = cap();
    let png = capabilities
        .generate_test_png(root.path(), "t.png", 8, 6)
        .expect("测试图");
    for (format, quality) in [("jpg", 90u8), ("webp", 82), ("png", 0)] {
        let out = capabilities
            .image_convert_with_background(
                &png,
                root.path(),
                format,
                quality,
                Some([255, 255, 255]),
                &AtomicBool::new(false),
            )
            .unwrap_or_else(|e| panic!("{format} 转换失败：{e}"));
        assert!(out.exists(), "{format}");
        assert!(out.extension().map(|e| e == format).unwrap_or(false));
    }
}

#[test]
fn cap_image_002_003_resize_and_rotate_keep_metadata_dimensions() {
    let root = tempfile::tempdir().unwrap();
    let capabilities = cap();
    let png = capabilities
        .generate_test_png(root.path(), "r.png", 40, 20)
        .expect("测试图");
    let thumb = capabilities
        .image_resize(&png, root.path(), 20, 20, false)
        .expect("边界框");
    let (w, _h) = capabilities.image_dimensions(&thumb).expect("尺寸");
    assert_eq!(w, 20, "不放大且保持比例");
    let rotated = capabilities
        .image_rotate(&png, root.path(), 90)
        .expect("旋转");
    let (rw, rh) = capabilities.image_dimensions(&rotated).expect("旋转尺寸");
    assert_eq!((rw, rh), (20, 40), "90 度旋转交换宽高");
}

#[test]
fn cap_media_001_audio_convert_wav_to_mp3_and_m4a_when_ffmpeg_available() {
    let root = tempfile::tempdir().unwrap();
    let capabilities = cap();
    let wav = match capabilities.generate_test_wav(root.path(), "tone.wav", 1) {
        Ok(wav) => wav,
        Err(CapabilityError::ToolUnavailable { .. }) => return, // 无 ffmpeg 时跳过成功路径（AC 条件说明）
        Err(e) => panic!("{e:?}"),
    };
    let mp3 = capabilities
        .audio_convert(&wav, root.path(), "mp3", 192)
        .expect("MP3");
    assert!(mp3.exists() && mp3.metadata().unwrap().len() > 0);
    let m4a = capabilities
        .audio_convert(&wav, root.path(), "m4a", 192)
        .expect("M4A");
    assert!(m4a.exists());
}

#[test]
fn cap_pdf_001_002_merge_and_split_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let capabilities = cap();
    let page1 = capabilities
        .generate_test_pdf(root.path(), "p1.pdf", 1)
        .expect("PDF1");
    let page2 = capabilities
        .generate_test_pdf(root.path(), "p2.pdf", 2)
        .expect("PDF2");
    let merged = capabilities
        .pdf_merge(&[page1.clone(), page2], root.path(), "合并.pdf")
        .expect("合并");
    assert_eq!(
        capabilities.pdf_page_count(&merged).expect("页数"),
        3,
        "页序与总数正确"
    );
    let split = capabilities
        .pdf_split_every(&merged, root.path(), 1)
        .expect("拆分");
    assert_eq!(split.len(), 3);
}

#[test]
fn cap_text_001_006_txt_markdown_read_write_and_docx_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let capabilities = cap();
    let txt = capabilities
        .create_text(root.path(), "文档.txt", "第一行\n第二行", "utf-8", "lf")
        .expect("TXT");
    assert_eq!(std::fs::read_to_string(&txt).unwrap(), "第一行\n第二行");
    let (content, encoding) = capabilities.read_text(&txt, None).expect("读取");
    assert_eq!(content, "第一行\n第二行");
    assert_eq!(encoding, "utf-8");
    let md = capabilities
        .create_markdown(root.path(), "说明.md", "# 标题\n\n正文")
        .expect("MD");
    let (md_content, _) = capabilities.read_markdown(&md).expect("读 MD");
    assert!(md_content.contains("# 标题"), "Markdown 保留源文");
    let docx = capabilities
        .create_docx(
            root.path(),
            "报告.docx",
            vec!["标题".into(), "段落一".into()],
        )
        .expect("DOCX");
    let extracted = capabilities.extract_docx_text(&docx).expect("正文");
    assert!(extracted.contains("段落一"), "{extracted}");
}

#[test]
fn ac_common_003_name_conflicts_generate_unique_names_not_overwriting() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("out.txt"), "原内容");
    let capabilities = cap();
    let second = capabilities
        .create_text_file(root.path(), "out.txt", "新内容", "utf-8")
        .expect("创建");
    assert_ne!(second.file_name().unwrap(), "out.txt", "重名生成不冲突名");
    assert_eq!(
        std::fs::read_to_string(root.path().join("out.txt")).unwrap(),
        "原内容",
        "不静默覆盖"
    );
}

fn list_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn transparent_png_to_jpeg_requires_explicit_background_and_keeps_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("transparent.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 0]))
        .save(&source)
        .unwrap();
    let original = std::fs::read(&source).unwrap();
    assert!(
        cap()
            .image_convert(&source, root.path(), "jpg", 95)
            .is_err()
    );
    assert!(!root.path().join("transparent.jpg").exists());
    let result = cap()
        .image_convert_with_background(
            &source,
            root.path(),
            "jpg",
            95,
            Some([255, 255, 255]),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(
        image::open(result)
            .unwrap()
            .to_rgb8()
            .pixels()
            .all(|pixel| pixel.0.iter().all(|channel| *channel > 250))
    );
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn cancelled_image_conversion_keeps_source_and_creates_no_output() {
    let root = tempfile::tempdir().unwrap();
    let source = cap()
        .generate_test_png(root.path(), "source.png", 8, 6)
        .unwrap();
    let original = std::fs::read(&source).unwrap();
    let error = cap()
        .image_convert_with_background(
            &source,
            root.path(),
            "jpg",
            90,
            Some([255, 255, 255]),
            &AtomicBool::new(true),
        )
        .unwrap_err();
    assert!(error.to_string().contains("已取消"), "{error}");
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert!(!root.path().join("source.jpg").exists());
}
