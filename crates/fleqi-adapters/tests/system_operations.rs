use fleqi_adapters::system_operations::execute;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

fn parameters(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}

fn read_http_request(socket: &mut TcpStream) {
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut request = Vec::new();
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let mut buffer = [0; 1024];
        let count = socket.read(&mut buffer).unwrap();
        assert!(count > 0, "request ended before headers");
        request.extend_from_slice(&buffer[..count]);
        assert!(
            request.len() < 32768,
            "request headers are unexpectedly large"
        );
    }
}

fn response_server(response: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        read_http_request(&mut socket);
        socket.write_all(&response).unwrap();
    });
    (format!("http://{address}/input"), handle)
}

#[test]
fn downloads_exact_bytes_and_renames_without_overwriting() {
    let temporary = tempfile::tempdir().unwrap();
    let original = temporary.path().join("file '中文'.bin");
    std::fs::write(&original, "existing").unwrap();
    let payload = b"\0\xffbinary\r\n";
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Disposition: attachment; filename=../../outside\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )
    .into_bytes();
    response.extend_from_slice(payload);
    let (url, server) = response_server(response);
    let result = execute(
        "CAP-NETWORK-002",
        &[],
        temporary.path(),
        &parameters(&[("url", &url), ("destination", "file '中文'.bin")]),
        &AtomicBool::new(false),
    )
    .unwrap();
    server.join().unwrap();
    let result: serde_json::Value = serde_json::from_str(&result.output).unwrap();
    let output = Path::new(result["file"].as_str().unwrap());
    assert_eq!(std::fs::read(output).unwrap(), payload);
    assert_eq!(std::fs::read(&original).unwrap(), b"existing");
    assert_ne!(output, original);
    assert_eq!(result["bytes"], payload.len());
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 2);
}

#[test]
fn http_error_and_truncated_body_preserve_existing_file_and_remove_temporary() {
    for response in [
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial".to_vec(),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("output.bin");
        std::fs::write(&target, "keep me").unwrap();
        let (url, server) = response_server(response);
        let result = execute(
            "CAP-NETWORK-002",
            &[],
            temporary.path(),
            &parameters(&[
                ("url", &url),
                ("destination", "output.bin"),
                ("collision", "overwrite"),
            ]),
            &AtomicBool::new(false),
        );
        server.join().unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"keep me");
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 1);
    }
}

#[test]
fn cancelled_download_kills_transfer_and_never_publishes_partial_file() {
    let temporary = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stall", listener.local_addr().unwrap());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        read_http_request(&mut socket);
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\npartial",
            )
            .unwrap();
        ready_tx.send(()).unwrap();
        stop_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let cancellation = cancel.clone();
    let cwd = temporary.path().to_path_buf();
    let worker = thread::spawn(move || {
        execute(
            "CAP-NETWORK-002",
            &[],
            &cwd,
            &parameters(&[("url", &url), ("destination", "output.bin")]),
            &cancellation,
        )
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancelled_at = Instant::now();
    cancel.store(true, Ordering::Release);
    let result = worker.join().unwrap();
    assert!(result.is_err());
    assert!(cancelled_at.elapsed() < Duration::from_secs(2));
    stop_tx.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 0);
}

#[test]
fn follows_http_redirect_and_records_resolved_source() {
    let temporary = tempfile::tempdir().unwrap();
    let (final_url, final_server) = response_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone".to_vec(),
    );
    let (initial_url, initial_server) = response_server(format!("HTTP/1.1 302 Found\r\nLocation: {final_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes());
    let output = execute(
        "CAP-NETWORK-002",
        &[],
        temporary.path(),
        &parameters(&[("url", &initial_url), ("destination", "result")]),
        &AtomicBool::new(false),
    )
    .unwrap();
    initial_server.join().unwrap();
    final_server.join().unwrap();
    let output: serde_json::Value = serde_json::from_str(&output.output).unwrap();
    assert_eq!(output["resolvedSource"], final_url);
    assert_eq!(
        std::fs::read(temporary.path().join("result")).unwrap(),
        b"done"
    );
}

#[cfg(unix)]
#[test]
fn explicit_download_overwrite_replaces_symlink_without_touching_link_target() {
    let temporary = tempfile::tempdir().unwrap();
    let unrelated = temporary.path().join("unrelated");
    let destination = temporary.path().join("output.bin");
    std::fs::write(&unrelated, "keep existing target").unwrap();
    std::os::unix::fs::symlink(&unrelated, &destination).unwrap();
    let (url, server) = response_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnew!".to_vec(),
    );
    execute(
        "CAP-NETWORK-002",
        &[],
        temporary.path(),
        &parameters(&[
            ("url", &url),
            ("destination", "output.bin"),
            ("collision", "overwrite"),
        ]),
        &AtomicBool::new(false),
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep existing target");
    assert_eq!(std::fs::read(&destination).unwrap(), b"new!");
    assert!(
        !std::fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 2);
}

#[test]
fn refuses_non_http_redirects_and_untrusted_parameters() {
    let temporary = tempfile::tempdir().unwrap();
    let (url, server) = response_server(b"HTTP/1.1 302 Found\r\nLocation: file:///etc/hosts\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec());
    assert!(
        execute(
            "CAP-NETWORK-002",
            &[],
            temporary.path(),
            &parameters(&[("url", &url), ("destination", "output")]),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    server.join().unwrap();
    for target in [
        "file:///etc/hosts",
        "https://user:secret@example.com/data",
        "--output=/tmp/file",
    ] {
        assert!(
            execute(
                "CAP-NETWORK-002",
                &[],
                temporary.path(),
                &parameters(&[("url", target), ("destination", "output")]),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    for host in ["--help", "127.0.0.1;touch evil", "$(id)"] {
        assert!(
            execute(
                "CAP-NETWORK-001",
                &[],
                temporary.path(),
                &parameters(&[("host", host)]),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 0);
}

#[test]
fn ping_loopback_reports_actual_protocol_and_packets() {
    let output = execute(
        "CAP-NETWORK-001",
        &[],
        Path::new("/tmp"),
        &parameters(&[
            ("host", "127.0.0.1"),
            ("count", "1"),
            ("timeoutSeconds", "1"),
        ]),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(output.output.contains("ICMP"));
    assert!(output.output.contains("1 packets transmitted"));
    assert!(output.output.contains("0.0% packet loss") || output.output.contains("0% packet loss"));
}

#[cfg(target_os = "macos")]
#[test]
fn reads_real_system_facts_without_changing_settings() {
    for capability in [
        "CAP-SYSTEM-009",
        "CAP-SYSTEM-010",
        "CAP-SYSTEM-011",
        "CAP-SYSTEM-012",
        "CAP-SYSTEM-013",
    ] {
        let output = execute(
            capability,
            &[],
            Path::new("/tmp"),
            &BTreeMap::new(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!output.output.trim().is_empty());
        assert!(!output.partial);
    }
    let memory = execute(
        "CAP-SYSTEM-010",
        &[],
        Path::new("/tmp"),
        &parameters(&[("unit", "bytes"), ("scope", "total")]),
        &AtomicBool::new(false),
    )
    .unwrap();
    let memory: serde_json::Value = serde_json::from_str(&memory.output).unwrap();
    let actual = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .unwrap();
    let actual: f64 = String::from_utf8(actual.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(memory["total"].as_f64().unwrap(), actual);
}

#[cfg(target_os = "macos")]
#[test]
fn hidden_flag_round_trips_only_on_owned_temporary_file() {
    use std::os::macos::fs::MetadataExt;
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("quoted ' 文本 ; $(touch injection)");
    std::fs::write(&path, "contents").unwrap();
    for hidden in ["true", "false"] {
        execute(
            "CAP-SYSTEM-014",
            std::slice::from_ref(&path),
            temporary.path(),
            &parameters(&[("hidden", hidden)]),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().st_flags() & libc::UF_HIDDEN != 0,
            hidden == "true"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"contents");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn hidden_flag_applies_to_symlink_without_changing_its_target() {
    use std::os::macos::fs::MetadataExt;
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("target");
    let link = temporary.path().join("link");
    std::fs::write(&target, "untouched").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    execute(
        "CAP-SYSTEM-014",
        std::slice::from_ref(&link),
        temporary.path(),
        &parameters(&[("hidden", "true")]),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        std::fs::symlink_metadata(&target).unwrap().st_flags() & libc::UF_HIDDEN,
        0
    );
    assert_ne!(
        std::fs::symlink_metadata(&link).unwrap().st_flags() & libc::UF_HIDDEN,
        0
    );
}

#[cfg(target_os = "macos")]
#[test]
fn caffeinate_has_a_real_deadline_and_cancellation_reaps_its_process() {
    let started = Instant::now();
    let result = execute(
        "CAP-SYSTEM-004",
        &[],
        Path::new("/tmp"),
        &parameters(&[("seconds", "1"), ("type", "idle")]),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(result.output.contains("释放"));
    let cancel = Arc::new(AtomicBool::new(false));
    let cancellation = cancel.clone();
    let worker = thread::spawn(move || {
        execute(
            "CAP-SYSTEM-004",
            &[],
            Path::new("/tmp"),
            &parameters(&[("seconds", "60"), ("type", "idle")]),
            &cancellation,
        )
    });
    let started = Instant::now();
    let pid = loop {
        let processes = std::process::Command::new("/usr/bin/pgrep")
            .args(["-P", &std::process::id().to_string(), "-x", "caffeinate"])
            .output()
            .unwrap();
        if processes.status.success() {
            break String::from_utf8(processes.stdout)
                .unwrap()
                .trim()
                .to_owned();
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "caffeinate child did not start"
        );
        thread::yield_now();
    };
    let assertions = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "assertions"])
        .output()
        .unwrap();
    let assertion_text = String::from_utf8(assertions.stdout).unwrap();
    assert!(
        assertion_text.contains(&format!("pid {pid}(caffeinate)")),
        "{assertion_text}"
    );
    cancel.store(true, Ordering::Release);
    assert!(worker.join().unwrap().is_err());
    let remaining = std::process::Command::new("/bin/ps")
        .args(["-p", &pid, "-o", "comm="])
        .output()
        .unwrap();
    assert!(!remaining.status.success());
}

#[cfg(target_os = "macos")]
#[test]
fn rejects_system_volume_and_side_effecting_invalid_parameters_before_execution() {
    let temporary = tempfile::tempdir().unwrap();
    let volume_error = execute(
        "CAP-SYSTEM-003",
        &[],
        temporary.path(),
        &parameters(&[("volume", "/")]),
        &AtomicBool::new(false),
    )
    .err()
    .unwrap();
    assert!(volume_error.contains("拒绝弹出系统卷"));
    for (id, invalid) in [
        ("CAP-SYSTEM-004", parameters(&[("seconds", "0")])),
        ("CAP-SYSTEM-005", parameters(&[("appearance", "inject")])),
        ("CAP-SYSTEM-006", parameters(&[("visible", "inject")])),
        ("CAP-SYSTEM-007", parameters(&[("copies", "0")])),
        ("CAP-SYSTEM-008", parameters(&[("delaySeconds", "-1")])),
        (
            "CAP-SYSTEM-015",
            parameters(&[
                ("recipient", "invalid recipient"),
                ("text", "must not send"),
            ]),
        ),
    ] {
        assert!(
            execute(id, &[], temporary.path(), &invalid, &AtomicBool::new(false)).is_err(),
            "{id}"
        );
        assert!(
            execute(
                id,
                &[],
                temporary.path(),
                &BTreeMap::new(),
                &AtomicBool::new(true)
            )
            .is_err(),
            "{id} pre-cancel"
        );
    }
}

#[test]
#[ignore = "需要真实 Open-Meteo 网络服务；按需显式运行"]
fn live_weather_returns_distinct_coordinates_times_and_ambiguous_candidates() {
    let mut observed = Vec::new();
    for location in ["35.68,139.69", "40.71,-74.0"] {
        let result = execute(
            "CAP-NETWORK-003",
            &[],
            Path::new("/tmp"),
            &parameters(&[("location", location)]),
            &AtomicBool::new(false),
        )
        .unwrap();
        let data: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(data["data"]["current"]["time"].as_str().is_some());
        assert!(data["data"]["current"]["temperature_2m"].as_f64().is_some());
        assert!(data["source"].as_str().unwrap().contains("Open-Meteo"));
        observed.push(data["data"]["longitude"].as_f64().unwrap());
    }
    assert!(observed[0] > 100.0 && observed[1] < -50.0);
    let error = execute(
        "CAP-NETWORK-003",
        &[],
        Path::new("/tmp"),
        &parameters(&[("location", "Springfield")]),
        &AtomicBool::new(false),
    )
    .err()
    .unwrap();
    assert!(
        error.contains("歧义") && error.contains("locationId"),
        "{error}"
    );
}
