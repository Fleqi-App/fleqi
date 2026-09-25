//! Keychain 端口测试：独立命名空间的自有测试项，结束清理（M1.6 资源回收证据）。
#![cfg(target_os = "macos")]

use fleqi_application::ports::{CredentialError, CredentialPort};
use fleqi_platform::macos::keychain::{KeychainCredentials, self_test};

const TEST_NAMESPACE: &str = "app.fleqi.desktop.test";

fn unique_key(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("test-{tag}-{nanos}")
}

#[test]
fn store_replace_exists_delete_roundtrip_and_cleanup() {
    let port = KeychainCredentials::new(TEST_NAMESPACE);
    let key = unique_key("roundtrip");
    assert_eq!(port.namespace(), TEST_NAMESPACE);
    assert!(!port.exists(&key).unwrap());
    port.store(&key, b"first").unwrap();
    assert!(port.exists(&key).unwrap());
    assert!(
        matches!(port.store(&key, b"again"), Err(CredentialError::Failed(_))),
        "store 不覆盖已有项"
    );
    port.replace(&key, b"second").unwrap();
    port.delete(&key).unwrap();
    assert!(!port.exists(&key).unwrap(), "删除后不存在（资源回收）");
    assert_eq!(port.delete(&key), Err(CredentialError::NotFound));
}

#[test]
fn self_test_reports_available_and_leaves_no_item() {
    let port = KeychainCredentials::new(TEST_NAMESPACE);
    let status = self_test(&port, "2026-09-17T12:00:00.123Z");
    assert!(status.available, "{status:?}");
    assert_eq!(status.namespace, TEST_NAMESPACE);
    assert!(!port.exists("selftest-20260917T120000123Z").unwrap());
}
