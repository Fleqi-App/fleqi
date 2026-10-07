//! Linux 凭据端口：只通过 `secret-tool` 访问 Secret Service。
//!
//! 参数是固定 argv，不经过 shell。密钥只走标准输入或标准输出，不写入日志，
//! 也不在二进制缺失或会话总线不可用时退回文件存储。

pub use fleqi_application::ports::{CredentialError, CredentialPort};
use std::sync::Arc;

pub use crate::credentials::self_test;

const PROGRAM: &str = "secret-tool";
const LABEL: &str = "fleqi";

/// `secret-tool` 一次调用的原始结果。测试用假运行器填充，生产由进程运行器填充。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretToolOutput {
    pub status: SecretToolStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// 进程是否启动，以及退出码。退出码 1 在 lookup/clear 上表示项不存在，
/// 但标准错误若说明会话总线不可用，则优先视为服务不可用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretToolStatus {
    Exited(i32),
    /// `PATH` 上没有 `secret-tool`。
    BinaryMissing,
    /// 进程无法启动，或运行器已判定会话总线不可用。
    Unavailable,
}

/// 可替换的命令运行器。生产实现启动 `secret-tool`；测试提供内存实现。
pub trait SecretToolRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String], stdin: &[u8]) -> SecretToolOutput;
}

pub struct LinuxCredentials {
    namespace: String,
    runner: Arc<dyn SecretToolRunner>,
}

impl LinuxCredentials {
    pub fn new(namespace: impl Into<String>) -> Self {
        Self::with_runner(namespace, Arc::new(ProcessSecretTool))
    }

    pub fn with_runner(namespace: impl Into<String>, runner: Arc<dyn SecretToolRunner>) -> Self {
        Self {
            namespace: namespace.into(),
            runner,
        }
    }

    fn invoke(&self, args: &[&str], stdin: &[u8]) -> Result<SecretToolOutput, CredentialError> {
        let owned = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let output = self.runner.run(PROGRAM, &owned, stdin);
        if let Some(error) = unavailable_from(&output) {
            return Err(error);
        }
        Ok(output)
    }
}

impl CredentialPort for LinuxCredentials {
    fn namespace(&self) -> &str {
        &self.namespace
    }

    fn store(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        validate_key(key)?;
        if self.exists(key)? {
            return Err(CredentialError::Failed(format!(
                "凭据项 {key} 已存在，请使用替换"
            )));
        }
        self.write(key, secret)
    }

    fn replace(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        validate_key(key)?;
        self.write(key, secret)
    }

    fn exists(&self, key: &str) -> Result<bool, CredentialError> {
        match self.load(key) {
            Ok(_) => Ok(true),
            Err(CredentialError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        validate_key(key)?;
        let output = self.invoke(&["clear", "service", &self.namespace, "account", key], &[])?;
        match exit_code(&output)? {
            0 => Ok(()),
            1 => Err(CredentialError::NotFound),
            code => Err(failed_command("clear", code, &output.stderr)),
        }
    }

    fn load(&self, key: &str) -> Result<Vec<u8>, CredentialError> {
        validate_key(key)?;
        let output = self.invoke(&["lookup", "service", &self.namespace, "account", key], &[])?;
        match exit_code(&output)? {
            0 => Ok(output.stdout),
            1 => Err(CredentialError::NotFound),
            code => Err(failed_command("lookup", code, &output.stderr)),
        }
    }
}

impl LinuxCredentials {
    fn write(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
        let output = self.invoke(
            &[
                "store",
                "--label",
                LABEL,
                "service",
                &self.namespace,
                "account",
                key,
            ],
            secret,
        )?;
        match exit_code(&output)? {
            0 => Ok(()),
            code => Err(failed_command("store", code, &output.stderr)),
        }
    }
}

fn validate_key(key: &str) -> Result<(), CredentialError> {
    if key.is_empty() {
        return Err(CredentialError::Failed("凭据键不能为空".into()));
    }
    if key.chars().any(char::is_control) {
        return Err(CredentialError::Failed(
            "凭据键包含控制字符或 NUL，已拒绝".into(),
        ));
    }
    Ok(())
}

fn exit_code(output: &SecretToolOutput) -> Result<i32, CredentialError> {
    match output.status {
        SecretToolStatus::Exited(code) => Ok(code),
        SecretToolStatus::BinaryMissing | SecretToolStatus::Unavailable => {
            Err(unavailable_from(output).unwrap_or_else(session_unavailable))
        }
    }
}

fn unavailable_from(output: &SecretToolOutput) -> Option<CredentialError> {
    match output.status {
        SecretToolStatus::BinaryMissing => Some(binary_missing()),
        SecretToolStatus::Unavailable => Some(session_unavailable_detail(&output.stderr)),
        SecretToolStatus::Exited(_) if session_bus_down(&output.stderr) => {
            Some(session_unavailable_detail(&output.stderr))
        }
        SecretToolStatus::Exited(_) => None,
    }
}

fn session_bus_down(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    text.contains("session bus")
        || text.contains("org.freedesktop.secrets")
        || text.contains("secret service")
        || text.contains("d-bus")
        || text.contains("dbus")
}

fn binary_missing() -> CredentialError {
    CredentialError::Unavailable(
        "Secret Service 不可用：找不到 secret-tool。请安装 secret-tool，并确认桌面会话总线可用。Fleqi 不会改用文件保存密钥。"
            .into(),
    )
}

fn session_unavailable() -> CredentialError {
    session_unavailable_detail(&[])
}

fn session_unavailable_detail(stderr: &[u8]) -> CredentialError {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        CredentialError::Unavailable(
            "Secret Service 不可用：secret-tool 无法连接会话总线。请确认 Secret Service 正在运行。Fleqi 不会改用文件保存密钥。"
                .into(),
        )
    } else {
        let detail = truncate_chars(detail, 240);
        CredentialError::Unavailable(format!(
            "Secret Service 不可用：secret-tool 无法连接会话总线（{detail}）。Fleqi 不会改用文件保存密钥。"
        ))
    }
}

fn failed_command(action: &str, code: i32, stderr: &[u8]) -> CredentialError {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        CredentialError::Failed(format!("secret-tool {action} 失败，退出码 {code}"))
    } else {
        let detail = truncate_chars(detail, 240);
        CredentialError::Failed(format!(
            "secret-tool {action} 失败，退出码 {code}：{detail}"
        ))
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect()
}

struct ProcessSecretTool;

impl SecretToolRunner for ProcessSecretTool {
    fn run(&self, program: &str, args: &[String], stdin_bytes: &[u8]) -> SecretToolOutput {
        if program != PROGRAM {
            return SecretToolOutput {
                status: SecretToolStatus::Unavailable,
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
        }
        let mut command = std::process::Command::new(PROGRAM);
        command
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return SecretToolOutput {
                    status: SecretToolStatus::BinaryMissing,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                };
            }
            Err(error) => {
                return SecretToolOutput {
                    status: SecretToolStatus::Unavailable,
                    stdout: Vec::new(),
                    stderr: error.to_string().into_bytes(),
                };
            }
        };
        let write_error = if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let result = stdin.write_all(stdin_bytes).and_then(|()| stdin.flush());
            drop(stdin);
            result.err()
        } else {
            None
        };
        let output = match child.wait_with_output() {
            Ok(output) => output,
            Err(error) => {
                return SecretToolOutput {
                    status: SecretToolStatus::Unavailable,
                    stdout: Vec::new(),
                    stderr: error.to_string().into_bytes(),
                };
            }
        };
        if let Some(error) = write_error {
            return SecretToolOutput {
                status: SecretToolStatus::Unavailable,
                stdout: Vec::new(),
                stderr: format!("secret-tool 标准输入失败：{error}").into_bytes(),
            };
        }
        SecretToolOutput {
            status: SecretToolStatus::Exited(output.status.code().unwrap_or(128)),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}
