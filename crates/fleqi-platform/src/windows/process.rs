//! 固定 PowerShell 脚本的启动器。脚本正文来自常量；参数只经环境变量传入。
//! 非 Windows 编译这份代码以便类型检查，但调用点被 `cfg` 排除，不会启动子进程。

#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::io::Write;
use std::process::{Command, Stdio};

pub(crate) struct ScriptOutput {
    pub success: bool,
    pub stdout: String,
}

pub(crate) fn run_fixed_script(
    script: &str,
    env: &[(&str, &str)],
    sta: bool,
    non_interactive: bool,
) -> Result<ScriptOutput, String> {
    let mut command = Command::new("powershell.exe");
    if sta {
        command.arg("-STA");
    }
    command.arg("-NoProfile");
    if non_interactive {
        command.arg("-NonInteractive");
    }
    command.args(["-Command", "-"]);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    for &(key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let write_result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "无法写入 PowerShell 输入".to_owned())?;
        stdin
            .write_all(script.as_bytes())
            .map_err(|error| error.to_string())
    })();
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    Ok(ScriptOutput {
        success: output.status.success(),
        stdout: decode_output(&output.stdout),
    })
}

fn decode_output(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units = bytes[2..]
            .chunks(2)
            .filter_map(|chunk| {
                chunk
                    .try_into()
                    .ok()
                    .map(|pair: [u8; 2]| u16::from_le_bytes(pair))
            })
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    String::from_utf8_lossy(bytes).into_owned()
}
