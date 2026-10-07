use super::*;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

fn until(terminal: &TerminalManager, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "Bash timed out: {:?}\n{}",
            terminal.readiness(),
            terminal.snapshot().screen
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn event(
    rx: &Receiver<TerminalEvent>,
    mut predicate: impl FnMut(&TerminalEvent) -> bool,
) -> TerminalEvent {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
            && predicate(&event)
        {
            return event;
        }
    }
    panic!("Bash control event timed out");
}

fn cwd(terminal: &TerminalManager) -> PathBuf {
    PathBuf::from(terminal.current_directory())
        .canonicalize()
        .unwrap()
}

fn request(terminal: &TerminalManager, rx: &Receiver<TerminalEvent>, target: &Path, id: u64) {
    let revision = Revision::new(id);
    terminal.send_cd(target, revision).unwrap();
    let result = event(
        rx,
        |event| matches!(event, TerminalEvent::CdResult { revision: current, .. } if *current == revision),
    );
    assert!(
        matches!(result, TerminalEvent::CdResult { ok: true, .. }),
        "{result:?}"
    );
    until(terminal, || terminal.readiness().is_safe());
    assert_eq!(cwd(terminal), target.canonicalize().unwrap());
}

#[test]
fn bash_private_directory_sync_preserves_input_and_cancels_stale_requests() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("A");
    let b = root.path().join("中文 B '$(touch pwned)'\nline");
    let c = root.path().join("C");
    for directory in [&a, &b, &c] {
        std::fs::create_dir(directory).unwrap();
    }
    let (tx, rx) = channel();
    let terminal = TerminalManager::spawn_with_shell(
        TerminalOptions {
            session_id: "bash-private-sync".into(),
            cwd: a.clone(),
            cols: 120,
            rows: 30,
            data_dir: root.path().join("state"),
            max_persisted_bytes: 1024 * 1024,
            events: tx,
        },
        integration::ShellKind::Bash,
    )
    .unwrap();
    if std::env::var_os("FLEQI_TEST_EXPECT_UNSUPPORTED").is_some() {
        assert_eq!(
            terminal.snapshot().shell_readiness,
            ShellReadinessState::Unknown
        );
        assert!(terminal.send_cd(&b, Revision::new(1)).is_err());
        terminal
            .write_input(b"printf manual > unsupported.txt\r")
            .unwrap();
        until(&terminal, || a.join("unsupported.txt").exists());
        assert_eq!(std::fs::read(a.join("unsupported.txt")).unwrap(), b"manual");
        terminal.shutdown();
        return;
    }
    until(&terminal, || terminal.readiness().is_safe());
    request(&terminal, &rx, &b, 1);
    assert!(!b.join("pwned").exists());

    // 提示符展开可以运行用户程序；同步确认必须等其结束，避免抢先投递等待命令。
    terminal
        .write_input(b"PS1='$(sleep 0.2; printf done > prompt-done) test> '\r")
        .unwrap();
    until(&terminal, || terminal.readiness().is_safe());
    terminal.send_cd(&a, Revision::new(40)).unwrap();
    event(
        &rx,
        |event| matches!(event, TerminalEvent::CdResult { revision, ok: true, .. } if revision.value() == 40),
    );
    assert_eq!(std::fs::read(a.join("prompt-done")).unwrap(), b"done");
    until(&terminal, || terminal.readiness().is_safe());
    terminal.write_input(b"PS1='fleqi> '\r").unwrap();
    until(&terminal, || terminal.readiness().is_safe());
    request(&terminal, &rx, &b, 42);
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&b, &alias).unwrap();
    request(&terminal, &rx, &alias, 41);

    terminal
        .write_input(b"read -e -t 0.2 timeout_answer; printf timed-out > timeout.txt\r")
        .unwrap();
    until(&terminal, || b.join("timeout.txt").exists());
    until(&terminal, || terminal.readiness().is_safe());

    terminal.write_input(b"printf kept > draft.txt").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    terminal.send_cd(&a, Revision::new(2)).unwrap();
    terminal.send_cd(&c, Revision::new(3)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(cwd(&terminal), b.canonicalize().unwrap());
    assert!(!b.join("draft.txt").exists());
    assert!(!terminal.readiness().is_safe());
    terminal.write_input(b"\x15").unwrap();
    event(
        &rx,
        |event| matches!(event, TerminalEvent::CdResult { revision, ok: true, .. } if revision.value() == 3),
    );
    until(&terminal, || terminal.readiness().is_safe());
    assert_eq!(cwd(&terminal), c.canonicalize().unwrap());

    terminal
        .write_input(b"read -e -p 'READ_INPUT> ' answer; printf '%s' \"$answer\" > answer.txt\r")
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    terminal.send_cd(&a, Revision::new(4)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(cwd(&terminal), c.canonicalize().unwrap());
    terminal.write_input(b"exact-user-answer\r").unwrap();
    event(
        &rx,
        |event| matches!(event, TerminalEvent::CdResult { revision, ok: true, .. } if revision.value() == 4),
    );
    assert_eq!(
        std::fs::read(c.join("answer.txt")).unwrap(),
        b"exact-user-answer"
    );
    until(&terminal, || terminal.readiness().is_safe());

    terminal.write_input(b"printf '%s' 'unfinished\r").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    terminal.send_cd(&b, Revision::new(5)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(cwd(&terminal), a.canonicalize().unwrap());
    terminal.write_input(b"\x03").unwrap();
    event(
        &rx,
        |event| matches!(event, TerminalEvent::CdResult { revision, ok: true, .. } if revision.value() == 5),
    );
    until(&terminal, || terminal.readiness().is_safe());

    terminal.write_input(b"cat <<'EOF'\r").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    terminal.send_cd(&c, Revision::new(6)).unwrap();
    terminal.set_directory_visibility(false, true);
    terminal.write_input(b"EOF\r").unwrap();
    event(
        &rx,
        |event| matches!(event, TerminalEvent::CdCancelled { revision } if revision.value() == 6),
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(cwd(&terminal), b.canonicalize().unwrap());
    terminal.set_directory_visibility(true, false);
    until(&terminal, || terminal.readiness().is_safe());

    terminal.write_input(b"sleep 0.7\r").unwrap();
    terminal.send_cd(&a, Revision::new(7)).unwrap();
    terminal.cancel_cd();
    event(
        &rx,
        |event| matches!(event, TerminalEvent::CdCancelled { revision } if revision.value() == 7),
    );
    until(&terminal, || terminal.readiness().is_safe());
    assert_eq!(cwd(&terminal), b.canonicalize().unwrap());

    terminal.write_input(b"set -o vi\r").unwrap();
    until(&terminal, || terminal.readiness().is_safe());
    request(&terminal, &rx, &c, 8);
    terminal.write_input(b"false\r").unwrap();
    until(&terminal, || terminal.readiness().is_safe());
    request(&terminal, &rx, &a, 9);
    terminal
        .write_input(b"printf '%s' \"$?\" > status.txt\r")
        .unwrap();
    until(&terminal, || a.join("status.txt").exists());
    assert_eq!(std::fs::read(a.join("status.txt")).unwrap(), b"1");
    until(&terminal, || terminal.readiness().is_safe());

    terminal.write_input(b"shopt -s cdspell\r").unwrap();
    until(&terminal, || terminal.readiness().is_safe());
    terminal
        .send_cd(&root.path().join("CC"), Revision::new(10))
        .unwrap();
    event(
        &rx,
        |event| matches!(event, TerminalEvent::CdResult { revision, ok: false, .. } if revision.value() == 10),
    );
    assert_eq!(cwd(&terminal), a.canonicalize().unwrap());
    until(&terminal, || terminal.readiness().is_safe());

    // 普通 PTY 输出不能伪造忙碌 shell 的空闲状态或目录。
    terminal
        .write_input(b"printf '\\033]7331;prompt;cwd=2f\\007'; sleep 0.8\r")
        .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    assert!(!terminal.readiness().is_safe());
    assert_eq!(cwd(&terminal), a.canonicalize().unwrap());
    until(&terminal, || terminal.readiness().is_safe());

    terminal.write_input(b"enable -d fleqi_sync\r").unwrap();
    until(&terminal, || {
        terminal.snapshot().shell_readiness == ShellReadinessState::Unknown
    });
    assert!(terminal.send_cd(&b, Revision::new(11)).is_err());
    terminal
        .write_input(b"printf manual > fallback.txt\r")
        .unwrap();
    until(&terminal, || a.join("fallback.txt").exists());
    assert_eq!(std::fs::read(a.join("fallback.txt")).unwrap(), b"manual");
    terminal.shutdown();
}
