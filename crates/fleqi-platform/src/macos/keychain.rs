//! macOS Keychain 凭据端口（architecture.md §12.3）：只操作 Fleqi 自有命名空间
//! （kSecAttrService = namespace），不同步 iCloud；不提供前端读回秘密的 IPC。

use fleqi_application::dto::CredentialStoreStatus;
use fleqi_application::ports::{CredentialError, CredentialPort};
use security_framework::base::Error as SecError;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

pub struct KeychainCredentials {
    service: String,
}

impl KeychainCredentials {
    /// `namespace` 形如 `app.fleqi.desktop`；测试使用独立命名空间并清理。
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            service: namespace.into(),
        }
    }
}

fn map_error(error: SecError) -> CredentialError {
    if error.code() == ERR_SEC_ITEM_NOT_FOUND {
        CredentialError::NotFound
    } else {
        CredentialError::Failed(format!("Keychain 错误 {}：{}", error.code(), error))
    }
}

impl CredentialPort for KeychainCredentials {
    fn namespace(&self) -> &str {
        &self.service
    }

    fn store(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        if self.exists(key)? {
            return Err(CredentialError::Failed(format!(
                "凭据项 {key} 已存在，请使用替换"
            )));
        }
        set_generic_password(&self.service, key, secret).map_err(map_error)
    }

    fn replace(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        set_generic_password(&self.service, key, secret).map_err(map_error)
    }

    fn exists(&self, key: &str) -> Result<bool, CredentialError> {
        match get_generic_password(&self.service, key) {
            Ok(_) => Ok(true),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(false),
            Err(error) => Err(map_error(error)),
        }
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        delete_generic_password(&self.service, key).map_err(map_error)
    }

    fn load(&self, key: &str) -> Result<Vec<u8>, CredentialError> {
        get_generic_password(&self.service, key).map_err(map_error)
    }
}

/// 自检：用独立命名的测试项走存/查/换/删全流程，结束清理；不触碰用户凭据。
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
