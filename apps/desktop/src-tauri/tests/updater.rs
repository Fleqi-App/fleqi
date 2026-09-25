//! Real updater transport and signature verification; never installs an app.
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    time::Duration,
};
use tauri_plugin_updater::UpdaterExt;

#[test]
fn signed_download_accepts_original_and_rejects_tampering_and_downgrades() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let key = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
    let fixture: Arc<[u8]> = std::env::var_os("FLEQI_UPDATE_PACKAGE")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| {
            include_bytes!("../../../../tests/fixtures/updater/payload.txt").to_vec()
        })
        .into();
    let signature = std::env::var_os("FLEQI_UPDATE_SIGNATURE")
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|| {
            include_str!("../../../../tests/fixtures/updater/payload.txt.sig").to_owned()
        });
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), config["plugins"]["updater"].clone());
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().pubkey(key).build())
        .build(context)
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory
        .path()
        .join("Fleqi.app/Contents/MacOS/fleqi-desktop");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, "test").unwrap();
    for (version, tampered) in [("999.0.0", false), ("999.0.0", true), ("0.0.0", false)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let feed = serde_json::to_vec(&serde_json::json!({"version":version,"platforms":{"darwin-aarch64":{"url":format!("http://{address}/payload"),"signature":signature.trim()}}})).unwrap();
        let requests = if version == "0.0.0" { 1 } else { 2 };
        let served = Arc::clone(&fixture);
        let server = std::thread::spawn(move || {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 4096];
                let length = stream.read(&mut request).unwrap();
                let payload: &[u8] =
                    if String::from_utf8_lossy(&request[..length]).starts_with("GET /feed ") {
                        feed.as_slice()
                    } else if tampered {
                        b"modified payload"
                    } else {
                        served.as_ref()
                    };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len()).unwrap();
                stream.write_all(payload).unwrap();
            }
        });
        let updater = app
            .updater_builder()
            .endpoints(vec![format!("http://{address}/feed").parse().unwrap()])
            .unwrap()
            .target("darwin-aarch64")
            .executable_path(&executable)
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        tauri::async_runtime::block_on(async {
            let update = updater.check().await.unwrap();
            if version == "0.0.0" {
                assert!(update.is_none());
                return;
            }
            let bytes = update.unwrap().download(|_, _| {}, || {}).await;
            if tampered {
                assert!(
                    bytes.is_err(),
                    "tampered update must never reach installation"
                );
            } else {
                assert_eq!(bytes.unwrap(), fixture.as_ref());
            }
        });
        server.join().unwrap();
    }
}
