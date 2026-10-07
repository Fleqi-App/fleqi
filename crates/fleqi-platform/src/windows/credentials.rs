//! Windows Credential Manager。秘密只传给系统 API，不启动脚本或写入参数、日志。
use fleqi_application::ports::{CredentialError, CredentialPort};

pub const WIN32_ERROR_NOT_FOUND: i32 = 1168;
pub const WIN32_ERROR_ALREADY_EXISTS: i32 = 183;

pub fn credential_target(namespace: &str, key: &str) -> Result<String, CredentialError> {
    if [namespace, key]
        .iter()
        .any(|part| part.is_empty() || part.chars().any(char::is_control))
    {
        return Err(CredentialError::Failed("凭据命名空间或键无效".into()));
    }
    Ok(format!("fleqi:{namespace}:{key}"))
}

pub fn map_credential_code(code: i32) -> Result<(), CredentialError> {
    match code {
        0 => Ok(()),
        WIN32_ERROR_NOT_FOUND => Err(CredentialError::NotFound),
        WIN32_ERROR_ALREADY_EXISTS => {
            Err(CredentialError::Failed("凭据项已存在，请使用替换".into()))
        }
        other => Err(CredentialError::Failed(format!(
            "Windows 凭据管理器返回错误 {other}"
        ))),
    }
}

pub struct WindowsCredentials {
    namespace: String,
}

impl WindowsCredentials {
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
        }
    }
}

impl CredentialPort for WindowsCredentials {
    fn namespace(&self) -> &str {
        &self.namespace
    }
    fn store(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        if self.exists(key)? {
            return map_credential_code(WIN32_ERROR_ALREADY_EXISTS);
        }
        self.replace(key, secret)
    }
    fn replace(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        let target = credential_target(&self.namespace, key)?;
        backend::write(&target, secret)
    }
    fn exists(&self, key: &str) -> Result<bool, CredentialError> {
        match self.load(key) {
            Ok(_) => Ok(true),
            Err(CredentialError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        backend::delete(&credential_target(&self.namespace, key)?)
    }
    fn load(&self, key: &str) -> Result<Vec<u8>, CredentialError> {
        backend::read(&credential_target(&self.namespace, key)?)
    }
}

#[cfg(windows)]
mod backend {
    use super::*;
    use windows::Win32::Security::Credentials::*;
    use windows::core::{HSTRING, PCWSTR, PWSTR};

    fn error(error: windows::core::Error) -> CredentialError {
        map_credential_code((error.code().0 as u32 & 0xffff) as i32)
            .err()
            .unwrap_or_else(|| CredentialError::Failed("凭据服务失败".into()))
    }
    pub fn write(target: &str, secret: &[u8]) -> Result<(), CredentialError> {
        if secret.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err(CredentialError::Failed(
                "凭据超过 Windows 凭据块上限 2560 字节".into(),
            ));
        }
        let name = HSTRING::from(target);
        let username = HSTRING::from("fleqi");
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(name.as_ptr().cast_mut()),
            CredentialBlobSize: secret.len() as u32,
            CredentialBlob: secret.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(username.as_ptr().cast_mut()),
            ..Default::default()
        };
        // SAFETY: API 同步复制数据；字符串及 secret 在整个调用期间有效。
        unsafe { CredWriteW(&credential, 0) }.map_err(error)
    }
    pub fn read(target: &str) -> Result<Vec<u8>, CredentialError> {
        let name = HSTRING::from(target);
        let mut raw = std::ptr::null_mut();
        // SAFETY: CredReadW 分配的结构由本函数释放，数据在释放前复制。
        unsafe {
            CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None, &mut raw).map_err(error)?;
            let credential = &*raw;
            let data = if credential.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec()
            };
            CredFree(raw.cast());
            Ok(data)
        }
    }
    pub fn delete(target: &str) -> Result<(), CredentialError> {
        let name = HSTRING::from(target);
        // SAFETY: name 在同步调用期间有效。
        unsafe { CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None) }.map_err(error)
    }
}

#[cfg(not(windows))]
mod backend {
    use super::CredentialError;
    fn unavailable() -> CredentialError {
        CredentialError::Unavailable("Windows 凭据管理器仅在 Windows 上可用".into())
    }
    pub fn write(_: &str, _: &[u8]) -> Result<(), CredentialError> {
        Err(unavailable())
    }
    pub fn read(_: &str) -> Result<Vec<u8>, CredentialError> {
        Err(unavailable())
    }
    pub fn delete(_: &str) -> Result<(), CredentialError> {
        Err(unavailable())
    }
}
