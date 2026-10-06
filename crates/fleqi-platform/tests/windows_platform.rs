use fleqi_application::ports::{CredentialError, CredentialPort};
use fleqi_platform::windows::credentials::{
    WindowsCredentials, credential_target, map_credential_code,
};

#[test]
fn credential_names_and_errors_keep_the_existing_contract() {
    assert_eq!(credential_target("应用", "键").unwrap(), "fleqi:应用:键");
    for invalid in ["", "a\0", "a\r", "a\n"] {
        assert!(credential_target(invalid, "key").is_err());
        assert!(credential_target("app", invalid).is_err());
    }
    assert!(map_credential_code(0).is_ok());
    assert_eq!(map_credential_code(1168), Err(CredentialError::NotFound));
    assert!(map_credential_code(183).is_err());
}

#[test]
#[cfg(not(windows))]
fn native_credentials_are_unavailable_off_windows() {
    let port = WindowsCredentials::new("app.fleqi.test.off-windows");
    assert!(matches!(
        port.load("probe"),
        Err(CredentialError::Unavailable(_))
    ));
}

#[test]
#[cfg(windows)]
fn native_credentials_round_trip_without_a_powershell_script() {
    let namespace = format!("app.fleqi.test.{}", std::process::id());
    let port = WindowsCredentials::new(&namespace);
    let key = "windows-round-trip";
    struct Cleanup<'a>(&'a WindowsCredentials, &'a str);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = self.0.delete(self.1);
        }
    }
    let _cleanup = Cleanup(&port, key);
    let _ = port.delete(key);
    assert!(!port.exists(key).unwrap());
    port.store(key, b"synthetic-test-value").unwrap();
    assert!(port.store(key, b"must-not-replace").is_err());
    assert_eq!(port.load(key).unwrap(), b"synthetic-test-value");
    port.replace(key, "中文测试".as_bytes()).unwrap();
    assert_eq!(port.load(key).unwrap(), "中文测试".as_bytes());
    assert!(port.replace(key, &vec![0; 2561]).is_err());
    assert_eq!(port.load(key).unwrap(), "中文测试".as_bytes());
    port.delete(key).unwrap();
    assert!(matches!(port.load(key), Err(CredentialError::NotFound)));
}
