//! M3.1 ProcessRunner 集成测试：真实进程、argv/cwd/env、取消、输出、退出码、无孤儿。

use fleqi_adapters::process::{ProcessEvent, ProcessRunner, SpawnRequest};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn wait_output(rx: &Receiver<ProcessEvent>, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut acc = String::new();
    while Instant::now() < deadline {
        if let Ok(ProcessEvent::Output { bytes, .. }) = rx.recv_timeout(Duration::from_millis(50)) {
            acc.push_str(&String::from_utf8_lossy(&bytes));
            if acc.contains(needle) {
                return true;
            }
        }
    }
    false
}

fn wait_exit(rx: &Receiver<ProcessEvent>, timeout: Duration) -> Option<i32> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(ProcessEvent::Exited { status }) = rx.recv_timeout(Duration::from_millis(50)) {
            return status;
        }
    }
    None
}

#[test]
#[cfg(target_os = "macos")]
fn finder_launched_task_extracts_pdf_with_system_only_path() {
    let directory = tempfile::tempdir().unwrap();
    let source = fleqi_adapters::capabilities::FileCapabilities::new()
        .generate_test_pdf(directory.path(), "中文 空格 ' PDF.pdf", 2)
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut runner = ProcessRunner::spawn(
        SpawnRequest {
            executable: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "command -v pdftotext && pdfinfo \"$1\" && pdftotext -layout \"$1\" -".into(),
                "fleqi-pdf-test".into(),
                source.display().to_string(),
            ],
            cwd: directory.path().to_owned(),
            env: vec![("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into())],
        },
        tx,
    )
    .unwrap();
    let mut output = String::new();
    let status = loop {
        match rx
            .recv_timeout(Duration::from_secs(15))
            .expect("PDF extraction completed")
        {
            ProcessEvent::Output { bytes, .. } => output.push_str(&String::from_utf8_lossy(&bytes)),
            ProcessEvent::Exited { status } => break status,
        }
    };
    assert_eq!(status, Some(0), "{output}");
    assert!(output.contains("Fleqi test page 1"), "{output}");
    assert!(output.contains("Fleqi test page 2"), "{output}");
    assert!(output.contains("Pages:"), "{output}");
    runner.wait();
}

#[test]
fn runs_argv_with_cwd_and_env_and_reports_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut runner = ProcessRunner::spawn(
        SpawnRequest {
            executable: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "printf \"%s \" \"$FLEQI_TEST_VAR\"; /bin/pwd".into(),
            ],
            cwd: dir.path().to_path_buf(),
            env: vec![("FLEQI_TEST_VAR".into(), "hello-env".into())],
        },
        tx,
    )
    .expect("spawn sh");
    // /bin/pwd 输出物理 cwd（chdir 不更新 $PWD 环境变量）。
    let dir_name = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        wait_output(&rx, "hello-env", Duration::from_secs(10)),
        "应输出环境变量"
    );
    assert!(
        wait_output(&rx, &dir_name, Duration::from_secs(10)),
        "cwd 应为临时目录"
    );
    assert_eq!(wait_exit(&rx, Duration::from_secs(10)), Some(0));
    runner.wait();
}

#[test]
fn nonzero_exit_code_reported_without_panic() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut runner = ProcessRunner::spawn(
        SpawnRequest {
            executable: "/bin/sh".into(),
            args: vec!["-c".into(), "echo doomed >&2; exit 42".into()],
            cwd: std::env::temp_dir(),
            env: vec![],
        },
        tx,
    )
    .expect("spawn sh");
    assert!(wait_output(&rx, "doomed", Duration::from_secs(10)));
    assert_eq!(wait_exit(&rx, Duration::from_secs(10)), Some(42));
    runner.wait();
}

#[test]
fn cancel_terminates_process_tree_without_orphans() {
    let script = tempfile::tempdir().unwrap();
    let marker = script.path().join("still-alive");
    let child_cmd = "sleep 300 & echo $! > /tmp/fleqi-proc-test-child.pid; sleep 400".to_string();
    std::fs::write(
        script.path().join("run.sh"),
        format!("#!/bin/sh\n{child_cmd}\n"),
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut runner = ProcessRunner::spawn(
        SpawnRequest {
            executable: "/bin/sh".into(),
            args: vec![script.path().join("run.sh").to_string_lossy().into_owned()],
            cwd: script.path().to_path_buf(),
            env: vec![("FLEQI_MARKER".into(), marker.to_string_lossy().into_owned())],
        },
        tx,
    )
    .expect("spawn");
    std::thread::sleep(Duration::from_millis(500));
    runner.cancel();
    let _ = wait_exit(&rx, Duration::from_secs(5));
    let _ = marker;
    std::thread::sleep(Duration::from_millis(300));
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("kill -0 $(cat /tmp/fleqi-proc-test-child.pid) 2>/dev/null && echo ALIVE || echo GONE")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "GONE",
        "子进程应被终止"
    );
    let _ = std::fs::remove_file("/tmp/fleqi-proc-test-child.pid");
}

#[test]
fn path_arguments_with_special_characters_never_split_into_commands() {
    let dir = tempfile::tempdir().unwrap();
    let weird = dir.path().join("a b;rm -rf pwned $(id) `echo x`");
    std::fs::create_dir_all(&weird).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut runner = ProcessRunner::spawn(
        SpawnRequest {
            executable: "/usr/bin/stat".into(),
            args: vec![
                "-f".into(),
                "%N".into(),
                weird.to_string_lossy().into_owned(),
            ],
            cwd: dir.path().to_path_buf(),
            env: vec![],
        },
        tx,
    )
    .expect("spawn stat");
    assert!(wait_output(&rx, "a b;rm -rf", Duration::from_secs(10)));
    assert_eq!(wait_exit(&rx, Duration::from_secs(10)), Some(0));
    assert!(!dir.path().join("pwned").exists());
    assert!(!PathBuf::from("x").exists());
    runner.wait();
}
