//! Windows Credential Manager。
//!
//! 目标名是纯函数。秘密只经环境变量以 Base64 交给固定脚本，不写入脚本正文，也不写日志。

use fleqi_application::ports::{CredentialError, CredentialPort};

/// `ERROR_NOT_FOUND`。脚本在凭据缺失时返回这个 Win32 码。
pub const WIN32_ERROR_NOT_FOUND: i32 = 1168;
/// `ERROR_ALREADY_EXISTS`。`store` 发现同名项时由脚本返回，调用方应改走替换。
pub const WIN32_ERROR_ALREADY_EXISTS: i32 = 183;

const MAX_CREDENTIAL_BLOB: usize = 2560;

/// 固定脚本：通过 Add-Type 调用 CredWrite / CredRead / CredDelete。
/// 命名空间、键和秘密只从环境变量读取，正文不含格式占位符。
pub const CREDENTIAL_SCRIPT: &str = r#"
$OutputEncoding = [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public sealed class CredOutcome {
    public int Code;
    public string Blob = "";
}

public static class FleqiWinCred {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct CREDENTIAL {
        public uint Flags;
        public uint Type;
        public string TargetName;
        public string Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public IntPtr Attributes;
        public string TargetAlias;
        public string UserName;
    }

    [DllImport("advapi32.dll", EntryPoint = "CredWriteW", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool CredWrite([In] ref CREDENTIAL userCredential, uint flags);

    [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool CredRead(string target, uint type, uint reservedFlag, out IntPtr credentialPtr);

    [DllImport("advapi32.dll", EntryPoint = "CredDeleteW", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool CredDelete(string target, uint type, uint flags);

    [DllImport("advapi32.dll", EntryPoint = "CredFree", SetLastError = true)]
    public static extern void CredFree(IntPtr cred);

    public static CredOutcome Dispatch(string action, string ns, string key, string secretB64) {
        CredOutcome outcome = new CredOutcome();
        try {
            if (string.IsNullOrEmpty(ns) || string.IsNullOrEmpty(key)) {
                outcome.Code = 87;
                return outcome;
            }
            string target = "fleqi:" + ns + ":" + key;
            if (action == "store") {
                int exists = ExistsCode(target);
                if (exists == 0) {
                    outcome.Code = 183;
                    return outcome;
                }
                if (exists != 1168) {
                    outcome.Code = exists;
                    return outcome;
                }
                outcome.Code = Write(target, Decode(secretB64));
                return outcome;
            }
            if (action == "replace") {
                outcome.Code = Write(target, Decode(secretB64));
                return outcome;
            }
            if (action == "exists") {
                outcome.Code = ExistsCode(target);
                return outcome;
            }
            if (action == "delete") {
                outcome.Code = Delete(target);
                return outcome;
            }
            if (action == "load") {
                return Read(target);
            }
            outcome.Code = 87;
            return outcome;
        } catch {
            outcome.Code = 87;
            outcome.Blob = "";
            return outcome;
        }
    }

    static byte[] Decode(string secretB64) {
        if (string.IsNullOrEmpty(secretB64)) {
            return new byte[0];
        }
        return Convert.FromBase64String(secretB64);
    }

    static int Write(string target, byte[] secret) {
        if (secret == null) {
            secret = new byte[0];
        }
        if (secret.Length > 2560) {
            return 87;
        }
        IntPtr blob = IntPtr.Zero;
        try {
            if (secret.Length > 0) {
                blob = Marshal.AllocHGlobal(secret.Length);
                Marshal.Copy(secret, 0, blob, secret.Length);
            }
            CREDENTIAL cred = new CREDENTIAL();
            cred.Flags = 0;
            cred.Type = 1;
            cred.TargetName = target;
            cred.Comment = null;
            cred.CredentialBlobSize = (uint)secret.Length;
            cred.CredentialBlob = blob;
            cred.Persist = 2;
            cred.AttributeCount = 0;
            cred.Attributes = IntPtr.Zero;
            cred.TargetAlias = null;
            cred.UserName = "fleqi";
            bool ok = CredWrite(ref cred, 0);
            if (!ok) {
                return Marshal.GetLastWin32Error();
            }
            return 0;
        } finally {
            if (blob != IntPtr.Zero) {
                Marshal.FreeHGlobal(blob);
            }
        }
    }

    static CredOutcome Read(string target) {
        CredOutcome outcome = new CredOutcome();
        IntPtr ptr = IntPtr.Zero;
        bool ok = CredRead(target, 1, 0, out ptr);
        if (!ok) {
            outcome.Code = Marshal.GetLastWin32Error();
            return outcome;
        }
        try {
            CREDENTIAL cred = (CREDENTIAL)Marshal.PtrToStructure(ptr, typeof(CREDENTIAL));
            int size = (int)cred.CredentialBlobSize;
            if (size < 0 || size > 2560) {
                outcome.Code = 87;
                return outcome;
            }
            byte[] bytes = new byte[size];
            if (size > 0 && cred.CredentialBlob != IntPtr.Zero) {
                Marshal.Copy(cred.CredentialBlob, bytes, 0, size);
            }
            outcome.Code = 0;
            outcome.Blob = Convert.ToBase64String(bytes);
            return outcome;
        } finally {
            if (ptr != IntPtr.Zero) {
                CredFree(ptr);
            }
        }
    }

    static int ExistsCode(string target) {
        IntPtr ptr = IntPtr.Zero;
        bool ok = CredRead(target, 1, 0, out ptr);
        if (!ok) {
            return Marshal.GetLastWin32Error();
        }
        if (ptr != IntPtr.Zero) {
            CredFree(ptr);
        }
        return 0;
    }

    static int Delete(string target) {
        bool ok = CredDelete(target, 1, 0);
        if (!ok) {
            return Marshal.GetLastWin32Error();
        }
        return 0;
    }
}
'@
$outcome = [FleqiWinCred]::Dispatch($env:FLEQI_CRED_ACTION, $env:FLEQI_CRED_NAMESPACE, $env:FLEQI_CRED_KEY, $env:FLEQI_CRED_SECRET_B64)
Write-Output $outcome.Code
if ($outcome.Code -eq 0 -and $env:FLEQI_CRED_ACTION -eq 'load') {
    Write-Output $outcome.Blob
}
"#;

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

/// `fleqi:{namespace}:{key}`。空值以及 NUL、回车、换行和其它控制字符都拒绝。
pub fn credential_target(namespace: &str, key: &str) -> Result<String, CredentialError> {
    if !valid_component(namespace) || !valid_component(key) {
        return Err(CredentialError::Failed("凭据命名空间或键无效".into()));
    }
    Ok(format!("fleqi:{namespace}:{key}"))
}

fn valid_component(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control)
}

/// 把脚本返回的 Win32 风格状态码映射为凭据错误。`0` 表示成功。
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

fn unavailable() -> CredentialError {
    CredentialError::Unavailable("Windows 凭据管理器仅在 Windows 上可用".into())
}

impl CredentialPort for WindowsCredentials {
    fn namespace(&self) -> &str {
        &self.namespace
    }

    fn store(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        let (code, _) = self.dispatch("store", key, Some(secret))?;
        map_credential_code(code)
    }

    fn replace(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        let (code, _) = self.dispatch("replace", key, Some(secret))?;
        map_credential_code(code)
    }

    fn exists(&self, key: &str) -> Result<bool, CredentialError> {
        let (code, _) = self.dispatch("exists", key, None)?;
        match map_credential_code(code) {
            Ok(()) => Ok(true),
            Err(CredentialError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        let (code, _) = self.dispatch("delete", key, None)?;
        map_credential_code(code)
    }

    fn load(&self, key: &str) -> Result<Vec<u8>, CredentialError> {
        let (code, blob) = self.dispatch("load", key, None)?;
        map_credential_code(code)?;
        Ok(blob.unwrap_or_default())
    }
}

impl WindowsCredentials {
    fn dispatch(
        &self,
        action: &str,
        key: &str,
        secret: Option<&[u8]>,
    ) -> Result<(i32, Option<Vec<u8>>), CredentialError> {
        credential_target(&self.namespace, key)?;
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (action, secret);
            Err(unavailable())
        }
        #[cfg(target_os = "windows")]
        {
            backend::execute(action, &self.namespace, key, secret)
        }
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod backend {
    use super::{CREDENTIAL_SCRIPT, CredentialError, MAX_CREDENTIAL_BLOB};

    pub(super) fn execute(
        action: &str,
        namespace: &str,
        key: &str,
        secret: Option<&[u8]>,
    ) -> Result<(i32, Option<Vec<u8>>), CredentialError> {
        if let Some(bytes) = secret
            && bytes.len() > MAX_CREDENTIAL_BLOB
        {
            return Err(CredentialError::Failed(format!(
                "凭据超过 Windows 凭据块上限 {MAX_CREDENTIAL_BLOB} 字节"
            )));
        }
        let encoded = secret.map(encode_base64).unwrap_or_default();
        let output = super::super::process::run_fixed_script(
            CREDENTIAL_SCRIPT,
            &[
                ("FLEQI_CRED_ACTION", action),
                ("FLEQI_CRED_NAMESPACE", namespace),
                ("FLEQI_CRED_KEY", key),
                ("FLEQI_CRED_SECRET_B64", encoded.as_str()),
            ],
            false,
            true,
        )
        .map_err(|_| CredentialError::Failed("无法启动 Windows 凭据脚本".into()))?;
        if !output.success {
            return Err(CredentialError::Failed("Windows 凭据脚本执行失败".into()));
        }
        parse_outcome(&output.stdout, action == "load")
    }

    fn parse_outcome(
        stdout: &str,
        want_blob: bool,
    ) -> Result<(i32, Option<Vec<u8>>), CredentialError> {
        let mut lines = stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let code_line = lines
            .next()
            .ok_or_else(|| CredentialError::Failed("凭据脚本没有返回状态".into()))?;
        let code = code_line
            .parse::<i32>()
            .map_err(|_| CredentialError::Failed("凭据脚本返回了无法识别的状态".into()))?;
        if !want_blob || code != 0 {
            return Ok((code, None));
        }
        let Some(blob) = lines.next() else {
            return Ok((code, Some(Vec::new())));
        };
        let bytes = decode_base64(blob)
            .ok_or_else(|| CredentialError::Failed("凭据内容无法解码".into()))?;
        Ok((code, Some(bytes)))
    }

    fn encode_base64(data: &[u8]) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        let mut index = 0;
        while index + 3 <= data.len() {
            let chunk = ((data[index] as u32) << 16)
                | ((data[index + 1] as u32) << 8)
                | (data[index + 2] as u32);
            out.push(TABLE[((chunk >> 18) & 63) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 63) as usize] as char);
            out.push(TABLE[((chunk >> 6) & 63) as usize] as char);
            out.push(TABLE[(chunk & 63) as usize] as char);
            index += 3;
        }
        match data.len() - index {
            1 => {
                let chunk = (data[index] as u32) << 16;
                out.push(TABLE[((chunk >> 18) & 63) as usize] as char);
                out.push(TABLE[((chunk >> 12) & 63) as usize] as char);
                out.push('=');
                out.push('=');
            }
            2 => {
                let chunk = ((data[index] as u32) << 16) | ((data[index + 1] as u32) << 8);
                out.push(TABLE[((chunk >> 18) & 63) as usize] as char);
                out.push(TABLE[((chunk >> 12) & 63) as usize] as char);
                out.push(TABLE[((chunk >> 6) & 63) as usize] as char);
                out.push('=');
            }
            _ => {}
        }
        out
    }

    fn decode_base64(data: &str) -> Option<Vec<u8>> {
        fn value(byte: u8) -> Option<u8> {
            match byte {
                b'A'..=b'Z' => Some(byte - b'A'),
                b'a'..=b'z' => Some(byte - b'a' + 26),
                b'0'..=b'9' => Some(byte - b'0' + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        }
        let bytes = data.as_bytes();
        if !bytes.len().is_multiple_of(4) {
            return None;
        }
        let mut out = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            let (b0, b1, b2, b3) = (
                bytes[index],
                bytes[index + 1],
                bytes[index + 2],
                bytes[index + 3],
            );
            if b2 == b'=' && b3 != b'=' {
                return None;
            }
            let chunk = ((value(b0)? as u32) << 18)
                | ((value(b1)? as u32) << 12)
                | (if b2 == b'=' {
                    0
                } else {
                    (value(b2)? as u32) << 6
                })
                | (if b3 == b'=' { 0 } else { value(b3)? as u32 });
            out.push((chunk >> 16) as u8);
            if b2 != b'=' {
                out.push((chunk >> 8) as u8);
            }
            if b3 != b'=' {
                out.push(chunk as u8);
            }
            index += 4;
        }
        Some(out)
    }
}
