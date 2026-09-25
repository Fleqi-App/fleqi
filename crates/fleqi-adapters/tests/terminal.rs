//! M2.3 终端适配器集成测试：真实 /bin/zsh + PTY。
//! 覆盖：启动到安全提示符、输出、目录控制消息（空格/引号/换行/前导连字符）、
//! 前台程序忙碌与 Ctrl+C、非空编辑行、resize、订阅游标、结束后无孤儿进程。

use fleqi_adapters::terminal::{TerminalEvent, TerminalManager, TerminalOptions};
use fleqi_domain::revision::Revision;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn wait_for<F: FnMut(&TerminalEvent) -> bool>(
    rx: &Receiver<TerminalEvent>,
    timeout: Duration,
    mut pred: F,
) -> Option<TerminalEvent> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                if pred(&event) {
                    return Some(event);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return None,
        }
    }
    None
}

fn wait_prompt(rx: &Receiver<TerminalEvent>) -> String {
    match wait_for(rx, Duration::from_secs(15), |e| {
        matches!(e, TerminalEvent::PromptReady { .. })
    }) {
        Some(TerminalEvent::PromptReady { cwd }) => cwd,
        other => panic!("未收到 PromptReady：{other:?}"),
    }
}

fn collect_output(rx: &Receiver<TerminalEvent>, timeout: Duration, needle: &str) -> bool {
    let deadline = Instant::now() + timeout;
    let mut acc = Vec::new();
    while Instant::now() < deadline {
        if let Ok(TerminalEvent::Output { bytes, .. }) = rx.recv_timeout(Duration::from_millis(50))
        {
            acc.extend_from_slice(&bytes);
            if String::from_utf8_lossy(&acc).contains(needle) {
                return true;
            }
        }
    }
    false
}

fn spawn(dir: &std::path::Path) -> (TerminalManager, Receiver<TerminalEvent>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = TerminalManager::spawn(TerminalOptions {
        session_id: "s-test".into(),
        cwd: dir.to_path_buf(),
        cols: 100,
        rows: 30,
        data_dir: dir.join("fleqi-terminal-data"),
        max_persisted_bytes: 100 * 1024 * 1024,
        events: tx,
    })
    .expect("启动 zsh");
    (manager, rx)
}

fn canonical(p: &std::path::Path) -> PathBuf {
    p.canonicalize().unwrap()
}

#[test]
fn shell_reaches_safe_prompt_in_initial_directory_and_echo_output_flows() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, rx) = spawn(dir.path());
    let cwd = wait_prompt(&rx);
    assert_eq!(canonical(Path::new(&cwd)), canonical(dir.path()));
    let readiness = manager.readiness();
    assert!(
        readiness.prompt_ready && readiness.edit_line_empty && readiness.foreground_is_shell,
        "{readiness:?}"
    );
    manager.write_input(b"echo fleqi-marker-42\r").unwrap();
    assert!(collect_output(
        &rx,
        Duration::from_secs(10),
        "fleqi-marker-42"
    ));
    wait_prompt(&rx);
    manager.shutdown();
}

#[test]
fn control_cd_reaches_directories_with_special_characters_without_executing_their_names() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, rx) = spawn(dir.path());
    wait_prompt(&rx);
    let names = [
        "with space",
        "quote'inside",
        "double\"quote",
        "-leading-dash",
        "semi;colon $(echo pwned) `id`",
        "new\nline",
        "中文 目录",
    ];
    for name in names {
        let target = dir.path().join(name);
        std::fs::create_dir_all(&target).unwrap();
        manager.send_cd(&target, Revision::new(1)).unwrap();
        let event = wait_for(&rx, Duration::from_secs(10), |e| {
            matches!(e, TerminalEvent::CdResult { .. })
        })
        .expect("cd 回执");
        match event {
            TerminalEvent::CdResult {
                revision, cwd, ok, ..
            } => {
                assert_eq!(revision, Revision::new(1));
                assert!(ok, "进入 {name:?} 失败");
                assert_eq!(canonical(Path::new(&cwd)), canonical(&target), "{name:?}");
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(!dir.path().join("pwned").exists());
    // 不存在的目录：失败并保留真实 cwd。
    let last = canonical(&dir.path().join("中文 目录"));
    manager
        .send_cd(&dir.path().join("does-not-exist"), Revision::new(2))
        .unwrap();
    match wait_for(&rx, Duration::from_secs(10), |e| {
        matches!(e, TerminalEvent::CdResult { .. })
    })
    .unwrap()
    {
        TerminalEvent::CdResult {
            ok,
            cwd,
            revision,
            message,
        } => {
            assert!(!ok);
            assert_eq!(revision, Revision::new(2));
            assert_eq!(canonical(Path::new(&cwd)), last);
            assert!(message.is_some());
        }
        other => panic!("{other:?}"),
    }
    manager.shutdown();
}

#[test]
fn foreground_program_makes_shell_busy_and_ctrl_c_restores_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, rx) = spawn(dir.path());
    wait_prompt(&rx);
    manager.write_input(b"sleep 30\r").unwrap();
    wait_for(&rx, Duration::from_secs(5), |e| {
        matches!(e, TerminalEvent::Preexec)
    })
    .expect("preexec");
    std::thread::sleep(Duration::from_millis(300));
    let readiness = manager.readiness();
    assert!(!readiness.prompt_ready, "{readiness:?}");
    assert!(
        !readiness.foreground_is_shell,
        "前台进程组应为 sleep：{readiness:?}"
    );
    assert!(manager.foreground_process().is_some());
    manager.write_input(b"\x03").unwrap();
    wait_prompt(&rx);
    std::thread::sleep(Duration::from_millis(200));
    assert!(manager.readiness().is_safe());
    manager.shutdown();
}

#[test]
fn non_empty_edit_line_is_not_safe_until_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, rx) = spawn(dir.path());
    wait_prompt(&rx);
    manager.write_input(b"abc").unwrap();
    wait_for(
        &rx,
        Duration::from_secs(5),
        |e| matches!(e, TerminalEvent::EditLine { len } if *len == 3),
    )
    .expect("edit len 3");
    let readiness = manager.readiness();
    assert!(
        readiness.prompt_ready && !readiness.edit_line_empty,
        "{readiness:?}"
    );
    manager.write_input(b"\x15").unwrap(); // Ctrl+U 清空编辑行
    wait_for(
        &rx,
        Duration::from_secs(5),
        |e| matches!(e, TerminalEvent::EditLine { len } if *len == 0),
    )
    .expect("edit len 0");
    assert!(manager.readiness().is_safe());
    manager.shutdown();
}

#[test]
fn resize_and_snapshot_cursor_allow_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, rx) = spawn(dir.path());
    wait_prompt(&rx);
    manager.resize(120, 40).unwrap();
    manager.write_input(b"echo before-reconnect\r").unwrap();
    assert!(collect_output(
        &rx,
        Duration::from_secs(10),
        "before-reconnect"
    ));
    wait_prompt(&rx);
    let snapshot = manager.snapshot();
    assert_eq!((snapshot.size.cols, snapshot.size.rows), (120, 40));
    assert!(
        snapshot.screen.contains("before-reconnect"),
        "屏幕快照应包含已输出内容"
    );
    let cursor: u64 = snapshot.stream_cursor.parse().unwrap();
    assert!(cursor > 0);
    // 从游标继续订阅：只收到之后的字节。
    let (tx2, rx2) = std::sync::mpsc::channel();
    manager.subscribe_from(cursor, tx2).unwrap();
    manager.write_input(b"echo after-reconnect\r").unwrap();
    let mut acc = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !acc.contains("after-reconnect") {
        if let Ok(TerminalEvent::Output { bytes, cursor: c }) =
            rx2.recv_timeout(Duration::from_millis(50))
        {
            assert!(c > cursor);
            acc.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    assert!(acc.contains("after-reconnect"));
    assert!(
        !acc.contains("before-reconnect"),
        "重连不应重放游标之前的输出"
    );
    manager.shutdown();
}

#[test]
fn shutdown_kills_process_tree_without_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, rx) = spawn(dir.path());
    wait_prompt(&rx);
    manager.write_input(b"sleep 300 &\r sleep 400\r").unwrap();
    wait_for(&rx, Duration::from_secs(5), |e| {
        matches!(e, TerminalEvent::Preexec)
    })
    .expect("preexec");
    std::thread::sleep(Duration::from_millis(300));
    let shell_pid = manager.shell_pid();
    manager.shutdown();
    std::thread::sleep(Duration::from_millis(500));
    let out = std::process::Command::new("pgrep")
        .args(["-f", "--", "sleep 300"])
        .output()
        .unwrap();
    let out2 = std::process::Command::new("pgrep")
        .args(["-f", "--", "sleep 400"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "后台 sleep 300 残留"
    );
    assert!(
        String::from_utf8_lossy(&out2.stdout).trim().is_empty(),
        "前台 sleep 400 残留"
    );
    let alive = std::process::Command::new("kill")
        .args(["-0", &shell_pid.to_string()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "shell 仍存活");
}

#[test]
fn explicit_unsubscribe_drops_sender_without_waiting_for_more_output() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, rx) = spawn(dir.path());
    wait_prompt(&rx);
    for _ in 0..24 {
        let (sender, receiver) = std::sync::mpsc::channel();
        let cursor = manager.snapshot().stream_cursor.parse().unwrap();
        let subscription = manager.subscribe_from(cursor, sender).unwrap();
        fleqi_application::ports::TerminalHandle::unsubscribe(&manager, subscription).unwrap();
        // Drain any prompt bytes racing with subscribe, then the channel must be disconnected.
        loop {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(_) => continue,
                Err(error) => panic!("subscription still retained: {error}"),
            }
        }
    }
    manager.shutdown();
}
