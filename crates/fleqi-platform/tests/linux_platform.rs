//! Linux 平台适配：不连接 D-Bus，不打开图形对话框。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fleqi_platform::credentials::self_test;
use fleqi_platform::linux::context::{
    ContextAvailability, ContextPort, LinuxContextPort, SessionKind, location_to_path, session_kind,
};
use fleqi_platform::linux::credentials::{
    CredentialError, CredentialPort, LinuxCredentials, SecretToolOutput, SecretToolRunner,
    SecretToolStatus,
};
use fleqi_platform::linux::observer::FileManagerActivationObserver;
use fleqi_platform::linux::permissions::{
    LinuxPermissions, Permission, PermissionPort, PermissionProcedure, PermissionStatus,
};
use fleqi_platform::linux::picker::{
    DialogTool, DirectoryPick, PathKind, classify_dialog_output, dialog_command,
    missing_dialog_tool, select_dialog_tool,
};
use fleqi_platform::linux::surface::{
    apply_material, file_manager_frame, layout_composer, material_kind, present, set_frame,
};
use fleqi_platform::linux::update_install::install_verified;
use fleqi_platform::scheduling::MainThreadExecutor;

const NAMESPACE: &str = "app.fleqi.desktop";

#[derive(Clone)]
struct FakeSecretTool {
    inner: Arc<Mutex<FakeState>>,
}

struct FakeState {
    items: HashMap<String, Vec<u8>>,
    calls: Vec<SecretCall>,
    mode: FakeMode,
}

#[derive(Clone, Copy, Debug)]
enum FakeMode {
    Service,
    BinaryMissing,
    Unavailable,
    SessionBusDown,
}

struct SecretCall {
    program: String,
    args: Vec<String>,
    stdin: Vec<u8>,
}

impl FakeSecretTool {
    fn new(mode: FakeMode) -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeState {
                items: HashMap::new(),
                calls: Vec::new(),
                mode,
            })),
        }
    }

    fn calls(&self) -> Vec<(String, Vec<String>, Vec<u8>)> {
        self.inner
            .lock()
            .expect("fake secret-tool")
            .calls
            .iter()
            .map(|call| (call.program.clone(), call.args.clone(), call.stdin.clone()))
            .collect()
    }
}

impl SecretToolRunner for FakeSecretTool {
    fn run(&self, program: &str, args: &[String], stdin: &[u8]) -> SecretToolOutput {
        let mut state = self.inner.lock().expect("fake secret-tool");
        state.calls.push(SecretCall {
            program: program.to_owned(),
            args: args.to_vec(),
            stdin: stdin.to_vec(),
        });
        match state.mode {
            FakeMode::BinaryMissing => SecretToolOutput {
                status: SecretToolStatus::BinaryMissing,
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
            FakeMode::Unavailable => SecretToolOutput {
                status: SecretToolStatus::Unavailable,
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
            FakeMode::SessionBusDown => SecretToolOutput {
                status: SecretToolStatus::Exited(1),
                stdout: Vec::new(),
                stderr: b"Cannot autolaunch D-Bus without a session bus".to_vec(),
            },
            FakeMode::Service => service_call(&mut state, args, stdin),
        }
    }
}

fn service_call(state: &mut FakeState, args: &[String], stdin: &[u8]) -> SecretToolOutput {
    let Some(verb) = args.first().map(String::as_str) else {
        return exited(2, Vec::new(), b"missing verb");
    };
    let Some(item) = item_key(args) else {
        return exited(2, Vec::new(), b"missing attributes");
    };
    match verb {
        "lookup" => match state.items.get(&item) {
            Some(secret) => exited(0, secret.clone(), b""),
            None => exited(1, Vec::new(), b""),
        },
        "store" => {
            state.items.insert(item, stdin.to_vec());
            exited(0, Vec::new(), b"")
        }
        "clear" => {
            if state.items.remove(&item).is_some() {
                exited(0, Vec::new(), b"")
            } else {
                exited(1, Vec::new(), b"")
            }
        }
        _ => exited(2, Vec::new(), b"unknown verb"),
    }
}

fn item_key(args: &[String]) -> Option<String> {
    let service = attribute(args, "service")?;
    let account = attribute(args, "account")?;
    Some(format!("{service}\u{1}{account}"))
}

fn attribute<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn exited(code: i32, stdout: Vec<u8>, stderr: &[u8]) -> SecretToolOutput {
    SecretToolOutput {
        status: SecretToolStatus::Exited(code),
        stdout,
        stderr: stderr.to_vec(),
    }
}

fn credentials(mode: FakeMode) -> (LinuxCredentials, FakeSecretTool) {
    let runner = FakeSecretTool::new(mode);
    let port = LinuxCredentials::with_runner(NAMESPACE, Arc::new(runner.clone()));
    (port, runner)
}

fn assert_unavailable(error: CredentialError) {
    match error {
        CredentialError::Unavailable(message) => {
            assert!(
                message.contains("Secret Service"),
                "message should name Secret Service: {message}"
            );
            assert!(
                message.contains("secret-tool"),
                "message should name secret-tool: {message}"
            );
        }
        other => panic!("expected unavailable, got {other:?}"),
    }
}

#[test]
fn secret_tool_argv_roundtrip_rejects_bad_keys_and_maps_not_found() {
    let (port, runner) = credentials(FakeMode::Service);
    assert_eq!(port.namespace(), NAMESPACE);
    assert_eq!(port.load("missing"), Err(CredentialError::NotFound));
    assert!(!port.exists("missing").unwrap());

    let before = runner.calls().len();
    assert!(matches!(
        port.store("", b"secret"),
        Err(CredentialError::Failed(_))
    ));
    assert!(matches!(
        port.store("bad\nkey", b"secret"),
        Err(CredentialError::Failed(_))
    ));
    assert!(matches!(
        port.store("bad\0key", b"secret"),
        Err(CredentialError::Failed(_))
    ));
    assert_eq!(runner.calls().len(), before, "invalid keys must not spawn");

    port.store("api-key", b"secret-bytes").unwrap();
    let calls = runner.calls();
    let lookup = calls.iter().find(|call| {
        call.1.first().map(String::as_str) == Some("lookup")
            && call.1.last().map(String::as_str) == Some("api-key")
    });
    let store = calls
        .iter()
        .find(|call| call.1.first().map(String::as_str) == Some("store"));
    let (program, args, _) = lookup.expect("exists lookup");
    assert_eq!(program, "secret-tool");
    assert_eq!(
        args,
        &vec![
            "lookup".to_owned(),
            "service".to_owned(),
            NAMESPACE.to_owned(),
            "account".to_owned(),
            "api-key".to_owned(),
        ]
    );
    let (program, args, stdin) = store.expect("store");
    assert_eq!(program, "secret-tool");
    assert_eq!(
        args,
        &vec![
            "store".to_owned(),
            "--label".to_owned(),
            "fleqi".to_owned(),
            "service".to_owned(),
            NAMESPACE.to_owned(),
            "account".to_owned(),
            "api-key".to_owned(),
        ]
    );
    assert_eq!(stdin, b"secret-bytes");
    assert!(calls.iter().all(|call| {
        call.0 == "secret-tool"
            && call
                .1
                .iter()
                .all(|arg| arg != "secret-bytes" && arg != "-c")
    }));

    let exists_error = port.store("api-key", b"again").unwrap_err();
    match exists_error {
        CredentialError::Failed(message) => {
            assert!(message.contains("已存在"), "{message}");
            assert!(message.contains("替换"), "{message}");
        }
        other => panic!("expected failed, got {other:?}"),
    }
    let stores_after_reject = runner
        .calls()
        .iter()
        .filter(|call| call.1.first().map(String::as_str) == Some("store"))
        .count();
    assert_eq!(
        stores_after_reject, 1,
        "store must not overwrite an existing item"
    );

    port.replace("api-key", b"replaced").unwrap();
    assert_eq!(port.load("api-key").unwrap(), b"replaced");
    port.delete("api-key").unwrap();
    assert!(!port.exists("api-key").unwrap());
    assert_eq!(port.load("api-key"), Err(CredentialError::NotFound));
    assert_eq!(port.delete("api-key"), Err(CredentialError::NotFound));
    let clear = runner
        .calls()
        .into_iter()
        .find(|call| call.1.first().map(String::as_str) == Some("clear"))
        .expect("clear");
    assert_eq!(clear.0, "secret-tool");
    assert_eq!(
        clear.1,
        vec![
            "clear".to_owned(),
            "service".to_owned(),
            NAMESPACE.to_owned(),
            "account".to_owned(),
            "api-key".to_owned(),
        ]
    );
}

#[test]
fn self_test_succeeds_on_fake_runner_and_fails_when_unavailable() {
    let (port, _) = credentials(FakeMode::Service);
    let status = self_test(&port, "2026-09-17T12:00:00.123Z");
    assert!(status.available, "{status:?}");
    assert_eq!(status.namespace, NAMESPACE);
    assert!(status.message.is_none());
    assert!(!port.exists("selftest-20260917T120000123Z").unwrap());

    for mode in [
        FakeMode::BinaryMissing,
        FakeMode::Unavailable,
        FakeMode::SessionBusDown,
    ] {
        let (port, _) = credentials(mode);
        let status = self_test(&port, "stamp");
        assert!(!status.available, "{mode:?} {status:?}");
        let message = status.message.expect("unavailable message");
        assert!(message.contains("Secret Service"), "{message}");
        assert!(message.contains("secret-tool"), "{message}");
        assert_unavailable(port.load("anything").unwrap_err());
    }
}

#[test]
fn session_kind_uses_session_type_then_display_variables() {
    assert_eq!(
        session_kind(Some("wayland"), Some("wayland-0"), Some(":0")),
        SessionKind::Wayland
    );
    assert_eq!(
        session_kind(Some("x11"), Some("wayland-0"), Some(":0")),
        SessionKind::X11
    );
    assert_eq!(
        session_kind(Some("tty"), None, Some(":0")),
        SessionKind::Unknown
    );
    assert_eq!(
        session_kind(None, Some("wayland-0"), Some(":0")),
        SessionKind::Wayland
    );
    assert_eq!(session_kind(None, Some("  "), Some(":0")), SessionKind::X11);
    assert_eq!(session_kind(None, None, Some(":1")), SessionKind::X11);
    assert_eq!(session_kind(None, None, None), SessionKind::Unknown);
    assert_eq!(session_kind(Some("X11"), None, None), SessionKind::X11);
}

struct InlineMain;

impl MainThreadExecutor for InlineMain {
    fn run(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
}

#[test]
fn capture_reports_no_directory_and_location_parser_rejects_escape() {
    let port = LinuxContextPort::new(Arc::new(InlineMain));
    let raw = port.capture();
    assert!(raw.directory.is_none());
    assert!(raw.selection.is_empty());
    assert!(raw.source_window_id.is_none());
    let ContextAvailability::NoDirectory { reason } = raw.unavailable.expect("unavailable") else {
        panic!("capture must not invent a file manager directory");
    };
    assert!(reason.contains("文件管理器"), "{reason}");
    assert!(reason.contains("Finder"), "{reason}");
    let kind = session_kind(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    );
    match kind {
        SessionKind::X11 => assert!(reason.contains("X11"), "{reason}"),
        SessionKind::Wayland => assert!(reason.contains("Wayland"), "{reason}"),
        SessionKind::Unknown => assert!(reason.contains("未知"), "{reason}"),
    }

    assert_eq!(
        location_to_path("file:///tmp/a%20b"),
        Some(PathBuf::from("/tmp/a b"))
    );
    assert_eq!(
        location_to_path("file://localhost/tmp/a%20b"),
        Some(PathBuf::from("/tmp/a b"))
    );
    assert!(location_to_path("http://example.com/a").is_none());
    assert!(location_to_path("https://example.com/file").is_none());
    assert!(location_to_path("file:///tmp/../etc/passwd").is_none());
    assert!(location_to_path("file:///tmp/%2e%2e/etc").is_none());
    assert!(location_to_path("/tmp/../etc").is_none());
    assert!(location_to_path("../etc").is_none());
    assert!(location_to_path("file://other.example/tmp/a").is_none());
}

#[test]
fn picker_classifies_cancel_directory_and_missing_tool_without_a_gui() {
    assert_eq!(select_dialog_tool(true, true), Some(DialogTool::Zenity));
    assert_eq!(select_dialog_tool(false, true), Some(DialogTool::Kdialog));
    assert_eq!(select_dialog_tool(false, false), None);

    let (program, args) = dialog_command(DialogTool::Zenity);
    assert_eq!(program, "zenity");
    assert_eq!(
        args,
        &[
            "--file-selection",
            "--directory",
            "--title",
            "选择 Fleqi 的工作文件夹"
        ]
    );
    let (program, args) = dialog_command(DialogTool::Kdialog);
    assert_eq!(program, "kdialog");
    assert_eq!(
        args,
        &[
            "--getexistingdirectory",
            "--title",
            "选择 Fleqi 的工作文件夹"
        ]
    );
    assert_eq!(args[1], "--title");
    assert!(!args.iter().any(|arg| arg.starts_with("--title=")));

    assert!(matches!(
        classify_dialog_output(1, "/tmp\n", true),
        DirectoryPick::Cancelled
    ));
    assert!(matches!(
        classify_dialog_output(5, "", false),
        DirectoryPick::Cancelled
    ));
    assert!(matches!(
        classify_dialog_output(0, " \n", true),
        DirectoryPick::Cancelled
    ));

    let selected = classify_dialog_output(0, "/tmp/work\n", true);
    match selected {
        DirectoryPick::Selected(path) => {
            assert_eq!(path.native, PathBuf::from("/tmp/work"));
            assert_eq!(path.kind, PathKind::Directory);
        }
        other => panic!("expected a directory, got {other:?}"),
    }
    match classify_dialog_output(0, "/missing/fleqi", false) {
        DirectoryPick::Failed(message) => assert!(message.contains("不是目录"), "{message}"),
        other => panic!("expected failure, got {other:?}"),
    }

    match missing_dialog_tool() {
        DirectoryPick::Failed(message) => {
            assert!(message.contains("zenity"), "{message}");
            assert!(message.contains("kdialog"), "{message}");
            assert!(!message.contains("已选择"));
        }
        other => panic!("missing tool must not look like a chosen directory: {other:?}"),
    }
}

#[test]
fn permissions_fail_and_do_not_say_they_are_granted() {
    #[allow(clippy::default_constructed_unit_structs)]
    let from_default = LinuxPermissions::default();
    for permissions in [
        from_default,
        LinuxPermissions::for_installed_build(std::path::Path::new("/tmp/fleqi-unused")),
    ] {
        assert!(permissions.reset_error().is_none());
        for permission in [Permission::FinderAutomation, Permission::Accessibility] {
            let passive = permissions.check(permission, PermissionProcedure::Passive);
            let explicit = permissions.check(permission, PermissionProcedure::Explicit);
            assert_eq!(passive, explicit);
            assert_eq!(passive.status, PermissionStatus::Failed);
            assert_ne!(passive.status, PermissionStatus::Allowed);
            let message = passive.error.expect("error");
            assert!(message.contains("Linux"), "{message}");
            assert!(message.contains("Finder"), "{message}");
            assert!(message.contains("X11"), "{message}");
            assert!(message.contains("Wayland"), "{message}");
            assert!(!message.contains("已授予"), "{message}");
            assert!(!message.contains("已允许"), "{message}");
            assert!(
                !message.to_ascii_lowercase().contains("granted"),
                "{message}"
            );
            assert!(
                !message.to_ascii_lowercase().contains("allowed"),
                "{message}"
            );
        }
    }
}

#[test]
fn material_stays_solid_and_frame_is_not_invented() {
    assert_eq!(material_kind(true), "solid");
    assert_eq!(material_kind(false), "solid");
    let frame = file_manager_frame();
    assert!(!frame.has_window);
    assert_eq!(frame.width, 0.0);
    assert_eq!(frame.height, 0.0);
    assert_eq!(frame.window_id, 0);
    unsafe {
        apply_material(std::ptr::null_mut(), true, true, 1);
        set_frame(std::ptr::null_mut(), 1.0, 2.0, 3.0, 4.0);
        layout_composer(std::ptr::null_mut(), 8.0);
        present(std::ptr::null_mut(), true, true, true, true);
    }
}

#[test]
fn observer_debounces_activation_and_install_does_not_emit() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let observer = FileManagerActivationObserver::new(Box::new(move |active| {
        record.lock().expect("observer log").push(active);
    }));
    assert!(observer.install().is_ok());
    assert!(observer.install().is_ok());
    assert!(seen.lock().expect("observer log").is_empty());

    observer.note_activation(true);
    observer.note_activation(false);
    assert_eq!(seen.lock().expect("observer log").as_slice(), &[true]);

    std::thread::sleep(Duration::from_millis(300));
    observer.note_activation(false);
    assert_eq!(
        seen.lock().expect("observer log").as_slice(),
        &[true, false]
    );
}

#[test]
fn install_verified_rejects_empty_bytes_and_macos_bundle() {
    let empty = install_verified(b"", "1.2.3").unwrap_err();
    assert!(empty.contains("1.2.3"), "{empty}");
    assert!(empty.contains("载荷为空"), "{empty}");
    assert!(empty.contains("保持原样"), "{empty}");

    let other = install_verified(b"not-a-bundle", "4.5.6").unwrap_err();
    assert!(other.contains("4.5.6"), "{other}");
    assert!(other.contains("不是 macOS 应用包"), "{other}");
    assert!(other.contains("保持原样"), "{other}");
    assert!(!other.contains("载荷为空"), "{other}");

    let bundle = install_verified(&macos_bundle_gzip(), "3.1.0").unwrap_err();
    assert!(bundle.contains("3.1.0"), "{bundle}");
    assert!(bundle.contains("是 macOS 应用包 Fleqi.app"), "{bundle}");
    assert!(bundle.contains("保持原样"), "{bundle}");

    let escaped = install_verified(&traversal_gzip(), "3.1.1").unwrap_err();
    assert!(escaped.contains("3.1.1"), "{escaped}");
    assert!(escaped.contains("保持原样"), "{escaped}");
    assert!(escaped.contains("不安全"), "{escaped}");
}

fn macos_bundle_gzip() -> Vec<u8> {
    gzip_store(&tar_file("Fleqi.app/Contents/MacOS/fleqi-desktop", b"new"))
}

fn traversal_gzip() -> Vec<u8> {
    gzip_store(&tar_file("../escape", b"x"))
}

fn tar_file(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut archive = Vec::new();
    archive.extend_from_slice(&ustar_header(name, contents.len() as u64));
    archive.extend_from_slice(contents);
    let pad = (512 - (contents.len() % 512)) % 512;
    archive.extend(std::iter::repeat_n(0, pad));
    archive.extend(std::iter::repeat_n(0, 1024));
    archive
}

fn ustar_header(name: &str, size: u64) -> [u8; 512] {
    assert!(name.len() < 100);
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    write_octal(&mut header[100..108], 0o644);
    write_octal(&mut header[108..116], 0);
    write_octal(&mut header[116..124], 0);
    write_octal(&mut header[124..136], size);
    write_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..262].copy_from_slice(b"ustar");
    header[263] = b'0';
    header[264] = b'0';
    let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let digits = format!("{sum:06o}");
    header[148..154].copy_from_slice(digits.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    header
}

fn write_octal(slot: &mut [u8], value: u64) {
    let digits = format!("{:0width$o}", value, width = slot.len() - 1);
    slot[..digits.len()].copy_from_slice(digits.as_bytes());
    slot[slot.len() - 1] = 0;
}

fn gzip_store(data: &[u8]) -> Vec<u8> {
    assert!(data.len() <= 65535);
    let mut out = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff];
    out.push(0x01);
    let len = u16::try_from(data.len()).expect("stored block length");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(u32::try_from(data.len()).unwrap_or(u32::MAX)).to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
