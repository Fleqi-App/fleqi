use fleqi_adapters::native_steps::NativeSteps;
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

fn execute(
    directory: &Path,
    operation: &str,
    source: Option<&Path>,
    params: &[(&str, &str)],
    cancelled: bool,
) -> Result<String, String> {
    let paths = Arc::new(PathRegistry::new());
    let input_refs = source
        .map(|path| vec![paths.register(path, PathKind::File).id])
        .unwrap_or_default();
    let parameters: BTreeMap<_, _> = params.iter().copied().collect();
    let step = ExecutionStep {
        script_runtime: None,
        kind: StepKind::Native,
        operation: operation.into(),
        executable_ref: None,
        script: None,
        args: vec![serde_json::to_string(&parameters).unwrap()],
        cwd_ref: None,
        env_refs: vec![],
        input_refs,
        expected_outputs: vec![],
    };
    NativeSteps::new(paths)
        .execute(&step, directory, &AtomicBool::new(cancelled))
        .map(|report| report.output)
}

const TEXT_CREATE: &str = if cfg!(windows) {
    "CAP-FILE-001"
} else {
    "CAP-TEXT-002"
};

#[cfg(windows)]
#[test]
fn windows_output_names_cannot_target_streams_devices_or_trimmed_aliases() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "normal.txt:stream",
        "NUL",
        "CON.txt",
        "COM¹",
        "trailing.",
        "trailing ",
        "wild*card",
    ] {
        let result = execute(
            root.path(),
            TEXT_CREATE,
            None,
            &[
                ("name", name),
                ("content", "new"),
                ("encoding", "utf-8"),
                ("newline", "lf"),
            ],
            false,
        );
        assert!(result.is_err(), "{name}: {result:?}");
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn overwrite_replaces_exact_existing_output_and_unique_mode_keeps_both() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("同名 文件.txt");
    std::fs::write(&target, "old").unwrap();
    let text = |policy| {
        [
            ("name", "同名 文件.txt"),
            ("content", "new"),
            ("encoding", "utf-8"),
            ("newline", "lf"),
            ("_nameConflict", policy),
        ]
    };
    execute(root.path(), TEXT_CREATE, None, &text("uniqueName"), false).unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    assert!(root.path().join("同名 文件 (1).txt").is_file());
    execute(root.path(), TEXT_CREATE, None, &text("overwrite"), false).unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    assert!(!root.path().join("同名 文件 (2).txt").exists());
    assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".fleqi-output-")
    }));
}

#[test]
fn failed_or_cancelled_generation_never_truncates_existing_output() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("photo.png");
    let target = root.path().join("photo.jpg");
    std::fs::write(&input, "broken input").unwrap();
    std::fs::write(&target, "valuable existing bytes").unwrap();
    let params = [
        ("format", "jpg"),
        ("quality", "85"),
        ("_nameConflict", "overwrite"),
    ];
    assert!(execute(root.path(), "CAP-IMAGE-001", Some(&input), &params, false).is_err());
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "valuable existing bytes"
    );
    assert!(execute(root.path(), "CAP-IMAGE-001", Some(&input), &params, true).is_err());
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "valuable existing bytes"
    );
}

#[test]
fn overwrite_protects_input_files_and_existing_directories() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("photo.png");
    image::RgbImage::from_pixel(3, 3, image::Rgb([0, 0, 255]))
        .save(&input)
        .unwrap();
    let original = std::fs::read(&input).unwrap();
    execute(
        root.path(),
        "CAP-IMAGE-001",
        Some(&input),
        &[
            ("format", "png"),
            ("quality", "85"),
            ("_nameConflict", "overwrite"),
        ],
        false,
    )
    .unwrap();
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert!(root.path().join("photo (1).png").is_file());
    std::fs::create_dir(root.path().join("folder.txt")).unwrap();
    std::fs::write(root.path().join("folder.txt/keep"), "keep").unwrap();
    execute(
        root.path(),
        TEXT_CREATE,
        None,
        &[
            ("name", "folder.txt"),
            ("content", "new"),
            ("encoding", "utf-8"),
            ("newline", "lf"),
            ("_nameConflict", "overwrite"),
        ],
        false,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("folder.txt/keep")).unwrap(),
        "keep"
    );
    assert!(root.path().join("folder (1).txt").is_file());
}
