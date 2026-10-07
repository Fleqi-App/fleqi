#![cfg(windows)]
use fleqi_adapters::process::{ProcessEvent, ProcessRunner, ProcessRunnerPort, SpawnRequest};
use fleqi_adapters::terminal::{TerminalManager, TerminalOptions};
use fleqi_application::ports::{ProcessEvent as PortEvent, ProcessPort};
use fleqi_domain::execution::ScriptRuntime;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "Windows 状态未在超时前到达");
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn terminal_until(terminal: &TerminalManager, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "终端未就绪：{:?}\n{}",
            terminal.readiness(),
            terminal.snapshot().screen
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn powershell_script_has_real_output_exit_and_literal_working_directory() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("中文 空格 'literal'");
    std::fs::create_dir(&cwd).unwrap();
    let (tx, rx) = mpsc::channel();
    let process = ProcessRunnerPort.spawn_script(ScriptRuntime::WindowsPowerShell,
        "[IO.File]::WriteAllText((Join-Path $PWD 'result.txt'), '真实结果'); Write-Output 'done'", &cwd, tx).unwrap();
    let mut output = String::new();
    loop {
        match rx.recv_timeout(STARTUP_TIMEOUT).unwrap() {
            PortEvent::Output { bytes, .. } => output.push_str(&String::from_utf8_lossy(&bytes)),
            PortEvent::Exited { status } => {
                assert_eq!(status, Some(0));
                break;
            }
        }
    }
    assert_eq!(process.wait(), Some(0));
    assert!(output.contains("done"));
    assert_eq!(
        std::fs::read_to_string(cwd.join("result.txt")).unwrap(),
        "真实结果"
    );
}

#[test]
fn terminal_reads_real_prompt_and_waits_for_empty_edit_line_before_cd() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("a");
    let target = root.path().join("中文 '目标'");
    std::fs::create_dir(&cwd).unwrap();
    std::fs::create_dir(&target).unwrap();
    let (tx, _rx) = mpsc::channel();
    let terminal = TerminalManager::spawn(TerminalOptions {
        session_id: "windows-terminal".into(),
        cwd: cwd.clone(),
        cols: 100,
        rows: 30,
        data_dir: root.path().join("state"),
        max_persisted_bytes: 1024 * 1024,
        events: tx,
    })
    .unwrap();
    terminal_until(&terminal, || terminal.readiness().is_safe());
    terminal.write_input(b"Write-Output 'typed'").unwrap();
    std::thread::sleep(Duration::from_millis(700));
    assert!(!terminal.readiness().is_safe());
    terminal
        .send_cd(&target, fleqi_domain::revision::Revision::new(1))
        .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(
        std::path::Path::new(&terminal.current_directory())
            .canonicalize()
            .unwrap(),
        cwd.canonicalize().unwrap()
    );
    terminal.write_input(b"\x03").unwrap();
    terminal_until(&terminal, || {
        terminal.readiness().is_safe()
            && std::path::Path::new(&terminal.current_directory())
                .canonicalize()
                .ok()
                == target.canonicalize().ok()
    });
    terminal
        .write_input("[IO.File]::WriteAllText((Join-Path $PWD '确认.txt'), '中文')\r".as_bytes())
        .unwrap();
    until(|| target.join("确认.txt").exists());
    assert_eq!(
        std::fs::read_to_string(target.join("确认.txt")).unwrap(),
        "中文"
    );
    terminal_until(&terminal, || terminal.readiness().is_safe());
    terminal.write_input(b"Start-Sleep -Seconds 1\r").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(!terminal.readiness().is_safe());
    terminal_until(&terminal, || terminal.readiness().is_safe());
    terminal.write_input(b"exit\r").unwrap();
    terminal_until(&terminal, || terminal.has_exited());
    terminal.shutdown();
}

#[test]
fn cancelling_a_process_returns_an_exit_event() {
    let root = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut runner = ProcessRunner::spawn(
        SpawnRequest {
            executable: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                "Write-Output started; Start-Sleep -Seconds 30".into(),
            ],
            cwd: root.path().into(),
            env: vec![],
        },
        tx,
    )
    .unwrap();
    assert!(matches!(
        rx.recv_timeout(STARTUP_TIMEOUT).unwrap(),
        ProcessEvent::Output { .. }
    ));
    runner.cancel();
    until(|| matches!(rx.try_recv(), Ok(ProcessEvent::Exited { .. })));
    assert_ne!(runner.wait(), Some(0));
}

#[test]
fn cancelled_or_hidden_windows_cd_never_applies_later() {
    use fleqi_domain::revision::Revision;
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("initial");
    let target = root.path().join("target");
    std::fs::create_dir(&cwd).unwrap();
    std::fs::create_dir(&target).unwrap();
    let (tx, rx) = mpsc::channel();
    let terminal = TerminalManager::spawn(TerminalOptions {
        session_id: "windows-cancel".into(),
        cwd: cwd.clone(),
        cols: 100,
        rows: 30,
        data_dir: root.path().join("state"),
        max_persisted_bytes: 1024 * 1024,
        events: tx,
    })
    .unwrap();
    terminal_until(&terminal, || terminal.readiness().is_safe());
    for mode in ["cancel", "hide", "pause"] {
        terminal.write_input(b"Write-Output 'draft'").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        terminal.send_cd(&target, Revision::new(1)).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        if mode == "cancel" {
            terminal.cancel_cd();
        } else {
            terminal.set_directory_visibility(false, mode == "hide");
        }
        until(|| {
            matches!(
                rx.try_recv(),
                Ok(fleqi_adapters::terminal::TerminalEvent::CdCancelled { .. })
            )
        });
        terminal.write_input(b"\x03").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            std::path::Path::new(&terminal.current_directory())
                .canonicalize()
                .unwrap(),
            cwd.canonicalize().unwrap()
        );
        terminal.set_directory_visibility(true, false);
        terminal_until(&terminal, || terminal.readiness().is_safe());
    }
    terminal.send_cd(&target, Revision::new(2)).unwrap();
    terminal_until(&terminal, || {
        terminal.readiness().is_safe()
            && std::path::Path::new(&terminal.current_directory())
                .canonicalize()
                .ok()
                == target.canonicalize().ok()
    });
    terminal.shutdown();
}

#[test]
fn cancelling_a_run_terminates_its_descendant_process() {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let root = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut runner = ProcessRunner::spawn(SpawnRequest {
        executable: "powershell.exe".into(),
        args: vec!["-NoProfile".into(), "-Command".into(), r#"$p=Start-Process (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoProfile -Command "Start-Sleep -Seconds 60"' -WindowStyle Hidden -PassThru; Write-Output $p.Id; Start-Sleep -Seconds 60"#.into()],
        cwd: root.path().into(), env: vec![],
    }, tx).unwrap();
    let mut output = String::new();
    let child_id = loop {
        match rx.recv_timeout(STARTUP_TIMEOUT).unwrap() {
            ProcessEvent::Output { bytes, .. } => {
                output.push_str(&String::from_utf8_lossy(&bytes));
                if output.contains('\n') {
                    break output.trim().parse::<u32>().unwrap();
                }
            }
            ProcessEvent::Exited { status } => panic!("fixture exited early {status:?}"),
        }
    };
    // 保留子进程句柄，按进程对象核验退出，不用可能被重用的 PID 判断。
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, child_id) }.unwrap();
    let _owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    runner.cancel();
    assert_ne!(runner.wait(), Some(0));
    until(|| {
        let mut code = 259;
        unsafe { GetExitCodeProcess(handle, &mut code) }.unwrap();
        code != 259
    });
}

#[test]
fn windows_files_and_zip_preserve_contents_and_reject_escapes() {
    use fleqi_adapters::capabilities::FileCapabilities;
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let caps = FileCapabilities::new();
    let source = caps
        .create_text_file(root.path(), "中文 '原件'.txt", "正文\n第二行", "utf-8")
        .unwrap();
    let folder = caps
        .create_folder(root.path(), "目录/子目录", true)
        .unwrap();
    assert_eq!(
        caps.copy(vec![source.clone()], &folder).unwrap().succeeded,
        1
    );
    assert_eq!(
        std::fs::read(&source).unwrap(),
        std::fs::read(folder.join(source.file_name().unwrap())).unwrap()
    );
    let moved = caps.create_folder(root.path(), "移动", false).unwrap();
    let copy = folder.join(source.file_name().unwrap());
    assert_eq!(
        caps.move_entries(vec![copy.clone()], &moved)
            .unwrap()
            .succeeded,
        1
    );
    assert!(!copy.exists());
    let item = moved.join(source.file_name().unwrap());
    assert_eq!(
        caps.rename_apply(caps.rename_preview(&[item], "改名-{name}"))
            .unwrap()
            .succeeded,
        1
    );
    let numbered = caps.create_folder(root.path(), "编号", false).unwrap();
    std::fs::write(numbered.join("a.txt"), b"a").unwrap();
    std::fs::write(numbered.join("b.txt"), b"b").unwrap();
    assert_eq!(
        caps.batch_number(&numbered, &["a.txt", "b.txt"], 1, 1, 2, "prefix")
            .unwrap()
            .succeeded,
        2
    );
    let plan = caps.organize_plan(&numbered, "extension", None).unwrap();
    assert_eq!(caps.organize_apply(&plan).unwrap().succeeded, 2);
    let nested = numbered.join("inside");
    std::fs::create_dir(&nested).unwrap();
    assert_eq!(
        caps.copy(vec![numbered.clone()], &nested)
            .unwrap()
            .succeeded,
        0
    );
    assert_eq!(
        caps.move_entries(vec![numbered.clone()], &nested)
            .unwrap()
            .succeeded,
        0
    );
    let archive = root.path().join("files.zip");
    caps.zip_create(&[source.clone()], &archive).unwrap();
    assert_eq!(caps.zip_list(&archive).unwrap().len(), 1);
    let output = root.path().join("unpacked");
    assert_eq!(caps.zip_extract(&archive, &output).unwrap().succeeded, 1);
    assert_eq!(
        std::fs::read(output.join(source.file_name().unwrap())).unwrap(),
        std::fs::read(&source).unwrap()
    );
    let tree_zip = root.path().join("tree.zip");
    let tree_out = root.path().join("tree-out");
    caps.zip_create(&[numbered.clone()], &tree_zip).unwrap();
    let report = caps.zip_extract(&tree_zip, &tree_out).unwrap();
    assert!(report.failures.is_empty(), "{report:?}");
    assert_eq!(
        std::fs::read(tree_out.join("编号/ByDate/01-a.txt")).unwrap(),
        b"a"
    );
    let archive = root.path().join("unsafe.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for name in [
        "../escape.txt",
        "..\\escape.txt",
        "C:/escape.txt",
        "file.txt:stream",
        "CON.txt",
        "trailing.",
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"must stay inside").unwrap();
    }
    zip.finish().unwrap();
    let report = caps
        .zip_extract(&archive, &root.path().join("blocked"))
        .unwrap();
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failures.len(), 6);
    assert!(!root.path().join("escape.txt").exists());
}

#[test]
fn windows_path_boundaries_preserve_originals() {
    use fleqi_adapters::capabilities::FileCapabilities;
    use std::os::windows::{fs::OpenOptionsExt, process::CommandExt};
    let source_root = tempfile::tempdir().unwrap();
    let artifact_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/.artifacts/windows-files");
    std::fs::create_dir_all(&artifact_root).unwrap();
    let destination_root = tempfile::tempdir_in(&artifact_root).unwrap();
    let caps = FileCapabilities::new();
    let source = source_root.path().join("跨卷 '原件'.txt");
    std::fs::write(&source, b"cross-volume").unwrap();
    let report = caps
        .move_entries(vec![source.clone()], destination_root.path())
        .unwrap();
    assert_eq!(report.succeeded, 1, "{report:?}");
    assert!(!source.exists());
    assert_eq!(
        std::fs::read(destination_root.path().join(source.file_name().unwrap())).unwrap(),
        b"cross-volume"
    );
    eprintln!(
        "move volumes: {:?} -> {:?}",
        source_root.path().components().next(),
        destination_root
            .path()
            .canonicalize()
            .unwrap()
            .components()
            .next()
    );
    let locked = source_root.path().join("locked.txt");
    std::fs::write(&locked, b"original").unwrap();
    let _lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked)
        .unwrap();
    assert_eq!(
        caps.move_entries(vec![locked.clone()], destination_root.path())
            .unwrap()
            .succeeded,
        0
    );
    assert!(locked.exists());
    let long_parent = destination_root
        .path()
        .join("目录".repeat(40))
        .join("层".repeat(80));
    let long = caps
        .create_text_file(&long_parent, "中文.txt", "long", "utf-8")
        .unwrap();
    assert_eq!(std::fs::read_to_string(long).unwrap(), "long");
    let archive = source_root.path().join("linked.zip");
    let tree = source_root.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("payload.txt"), b"must not escape").unwrap();
    caps.zip_create(&[tree], &archive).unwrap();
    let extraction = source_root.path().join("extract");
    std::fs::create_dir(&extraction).unwrap();
    let output = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:FLEQI_LINK -Target $env:FLEQI_TARGET | Out-Null"])
        .env("FLEQI_LINK", extraction.join("tree")).env("FLEQI_TARGET", destination_root.path()).creation_flags(0x08000000).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = caps.zip_extract(&archive, &extraction).unwrap();
    assert_eq!(report.succeeded, 0, "{report:?}");
    assert!(!destination_root.path().join("payload.txt").exists());
}

#[test]
fn windows_images_and_pdf_generate_readable_outputs() {
    use fleqi_adapters::capabilities::FileCapabilities;
    let root = tempfile::tempdir().unwrap();
    let caps = FileCapabilities::new();
    let source = caps
        .generate_test_png(root.path(), "原图.png", 80, 40)
        .unwrap();
    for format in ["jpg", "webp", "png"] {
        let converted = caps
            .image_convert(&source, root.path(), format, 85)
            .unwrap();
        assert_eq!(caps.image_dimensions(&converted).unwrap(), (80, 40));
    }
    let small = caps
        .image_resize(&source, root.path(), 20, 20, false)
        .unwrap();
    assert_eq!(caps.image_dimensions(&small).unwrap(), (20, 10));
    let rotated = caps.image_rotate(&source, root.path(), 90).unwrap();
    assert_eq!(caps.image_dimensions(&rotated).unwrap(), (40, 80));
    let jpeg = caps.image_convert(&source, root.path(), "jpg", 90).unwrap();
    let compressed = caps.image_convert(&jpeg, root.path(), "jpg", 30).unwrap();
    assert_eq!(caps.image_dimensions(&compressed).unwrap(), (80, 40));
    let first = caps.generate_test_pdf(root.path(), "a.pdf", 2).unwrap();
    let second = caps.generate_test_pdf(root.path(), "b.pdf", 1).unwrap();
    let merged = caps
        .pdf_merge(&[first.clone(), second], root.path(), "merged.pdf")
        .unwrap();
    assert_eq!(caps.pdf_page_count(&merged).unwrap(), 3);
    let split = caps.pdf_split_every(&merged, root.path(), 1).unwrap();
    assert_eq!(split.len(), 3);
    assert!(
        split
            .iter()
            .all(|path| caps.pdf_page_count(path).unwrap() == 1)
    );
    let selected = caps
        .pdf_extract_pages(&merged, &[3, 1], root.path(), "selected.pdf")
        .unwrap();
    assert_eq!(caps.pdf_page_count(&selected).unwrap(), 2);
    let turned = caps
        .pdf_rotate_pages(&merged, &[1], 90, root.path(), "rotated.pdf")
        .unwrap();
    let document = lopdf::Document::load(&turned).unwrap();
    assert_eq!(
        document
            .get_dictionary(document.get_pages()[&1])
            .unwrap()
            .get(b"Rotate")
            .unwrap()
            .as_i64()
            .unwrap(),
        90
    );
    let (compressed, _, _) = caps
        .pdf_compress(&merged, root.path(), "compressed.pdf")
        .unwrap();
    assert_eq!(caps.pdf_page_count(&compressed).unwrap(), 3);
    assert_eq!(caps.pdf_page_count(&first).unwrap(), 2);
    assert!(source.exists());
}

#[test]
fn windows_recycle_can_be_restored_by_the_system() {
    use fleqi_adapters::capabilities::FileCapabilities;
    use std::os::windows::process::CommandExt;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let root = tempfile::Builder::new()
        .prefix("fleqi-recycle-long-parent-")
        .tempdir()
        .unwrap();
    let long_parent = root.path().canonicalize().unwrap();
    let expected = long_parent.to_string_lossy();
    let expected = expected.strip_prefix(r"\\?\").unwrap_or(&expected);
    let mut short = vec![0u16; 32768];
    // SAFETY: 根目录仍存在，缓冲区足够容纳 Win32 路径；卷未开启短名时使用原路径。
    let count = unsafe {
        GetShortPathNameW(
            &windows::core::HSTRING::from(root.path().as_os_str()),
            Some(&mut short),
        )
    };
    let parent = if count > 0 && (count as usize) < short.len() {
        std::path::PathBuf::from(String::from_utf16(&short[..count as usize]).unwrap())
    } else {
        root.path().to_path_buf()
    };
    let source = parent.join(format!("fleqi-recycle-{}.txt", std::process::id()));
    std::fs::write(&source, b"restorable fixture").unwrap();
    let report = FileCapabilities::new().trash(vec![source.clone()]);
    assert_eq!(report.succeeded, 1, "{report:?}");
    assert!(!source.exists());
    // 原目录由本测试独占，按它匹配不依赖 Shell 的显示名、扩展名隐藏设置或语言。
    let script = "$ErrorActionPreference='Stop'; $parent=$env:FLEQI_TEST_RECYCLE_PARENT; $shell=New-Object -ComObject Shell.Application; $items=@($shell.NameSpace(10).Items() | Where-Object { $_.ExtendedProperty('System.Recycle.DeletedFrom') -eq $parent }); if ($items.Count -ne 1) { throw ('fixture not uniquely found in recycle bin; source=' + $env:FLEQI_TEST_RECYCLE_PATH + '; canonical parent=' + $parent + '; matches=' + $items.Count) }; $items[0].InvokeVerb('undelete')";
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("FLEQI_TEST_RECYCLE_PATH", &source)
        .env("FLEQI_TEST_RECYCLE_PARENT", expected)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    until(|| source.exists());
    assert_eq!(std::fs::read(&source).unwrap(), b"restorable fixture");
}
