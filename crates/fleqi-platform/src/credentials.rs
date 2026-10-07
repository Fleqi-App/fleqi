//! 凭据自检：存、查、换、删，结束时清理。不读取、不记录秘密内容。

use fleqi_application::dto::CredentialStoreStatus;
use fleqi_application::ports::{CredentialError, CredentialPort};

/// 用独立测试项走完整流程。失败时 `available` 为 false，并带上平台错误说明。
pub fn self_test(port: &dyn CredentialPort, stamp: &str) -> CredentialStoreStatus {
    let key = format!("selftest-{}", stamp.replace([':', '-', '.'], ""));
    let namespace = port.namespace().to_owned();
    let outcome = (|| -> Result<(), CredentialError> {
        let _ = port.delete(&key);
        port.store(&key, b"fleqi-selftest-1")?;
        if !port.exists(&key)? {
            return Err(CredentialError::Failed("写入后读取不到测试项".into()));
        }
        port.replace(&key, b"fleqi-selftest-2")?;
        port.delete(&key)?;
        if port.exists(&key)? {
            return Err(CredentialError::Failed("删除后测试项仍存在".into()));
        }
        Ok(())
    })();
    let _ = port.delete(&key);
    match outcome {
        Ok(()) => CredentialStoreStatus {
            available: true,
            namespace,
            message: None,
        },
        Err(error) => CredentialStoreStatus {
            available: false,
            namespace,
            message: Some(error.to_string()),
        },
    }
}
