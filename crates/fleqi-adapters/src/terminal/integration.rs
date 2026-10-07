//! Fleqi 自带的 zsh shell integration（architecture.md §5.2）：不改写用户的 shell 配置文件。
//! 通过临时 ZDOTDIR 包装：包装文件先加载用户原有 .zshenv/.zprofile/.zshrc/.zlogin，
//! 再安装钩子，用私有 OSC 7331 报告 prompt/preexec/编辑行长度/cwd。

use std::path::{Path, PathBuf};

pub const OSC_CODE: &str = "7331";

/// 写入包装目录，返回 ZDOTDIR 路径。`user_zdotdir` 为用户原 ZDOTDIR（无则 HOME）。
pub fn install(base: &Path, user_zdotdir: &Path) -> std::io::Result<PathBuf> {
    let dir = base.join("zsh-integration");
    std::fs::create_dir_all(&dir)?;
    let user = user_zdotdir.to_string_lossy().replace('\'', "'\\''");
    let zshenv = format!(
        r#"# Fleqi zsh integration (generated; do not edit)
export FLEQI_USER_ZDOTDIR='{user}'
[[ -f "$FLEQI_USER_ZDOTDIR/.zshenv" ]] && ZDOTDIR="$FLEQI_USER_ZDOTDIR" source "$FLEQI_USER_ZDOTDIR/.zshenv"
"#
    );
    let zprofile = r#"[[ -f "$FLEQI_USER_ZDOTDIR/.zprofile" ]] && ZDOTDIR="$FLEQI_USER_ZDOTDIR" source "$FLEQI_USER_ZDOTDIR/.zprofile"
"#;
    let zlogin = r#"[[ -f "$FLEQI_USER_ZDOTDIR/.zlogin" ]] && ZDOTDIR="$FLEQI_USER_ZDOTDIR" source "$FLEQI_USER_ZDOTDIR/.zlogin"
"#;
    let zshrc = format!(
        r#"# Fleqi zsh integration (generated; do not edit)
[[ -f "$FLEQI_USER_ZDOTDIR/.zshrc" ]] && ZDOTDIR="$FLEQI_USER_ZDOTDIR" source "$FLEQI_USER_ZDOTDIR/.zshrc"
export ZDOTDIR="$FLEQI_USER_ZDOTDIR"

__fleqi_osc() {{ builtin printf '\e]{osc};%s\a' "$1" }}
__fleqi_hex() {{ builtin printf '%s' "$1" | command od -An -v -tx1 | command tr -d ' \n' }}
__fleqi_precmd() {{ __fleqi_osc "prompt;cwd=$(__fleqi_hex "$PWD")" }}
__fleqi_preexec() {{ __fleqi_osc "preexec" }}
__fleqi_line_init() {{ __fleqi_osc "edit;len=0" }}
__fleqi_line_redraw() {{ __fleqi_osc "edit;len=${{#BUFFER}}" }}
autoload -Uz add-zsh-hook
add-zsh-hook precmd __fleqi_precmd
add-zsh-hook preexec __fleqi_preexec
autoload -Uz add-zle-hook-widget
add-zle-hook-widget line-init __fleqi_line_init
add-zle-hook-widget line-pre-redraw __fleqi_line_redraw
setopt HIST_IGNORE_SPACE
"#,
        osc = OSC_CODE
    );
    std::fs::write(dir.join(".zshenv"), zshenv)?;
    std::fs::write(dir.join(".zprofile"), zprofile)?;
    std::fs::write(dir.join(".zlogin"), zlogin)?;
    std::fs::write(dir.join(".zshrc"), zshrc)?;
    Ok(dir)
}

/// 目录控制消息（terminal 模块生成，不是 AI 命令）：单引号字面量 + `--`，
/// 空格/引号/换行/前导连字符/`$()`/反引号都不改变指令结构；前导空格使命令不进历史。
pub fn cd_control_line(target: &Path) -> Vec<u8> {
    let quoted = target.to_string_lossy().replace('\'', "'\\''");
    format!(" builtin cd -- '{quoted}'\r").into_bytes()
}

/// PowerShell 目录控制消息。同样使用单引号字面量，不经 Invoke-Expression。
pub fn powershell_cd_line(target: &Path) -> Vec<u8> {
    let quoted = target.to_string_lossy().replace('\'', "''");
    format!(" Set-Location -LiteralPath '{quoted}'\r").into_bytes()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
    Bash,
    PowerShell,
}

impl ShellKind {
    pub fn label(self) -> &'static str {
        match self {
            ShellKind::Zsh => "/bin/zsh",
            ShellKind::Bash => "/bin/bash",
            ShellKind::PowerShell => "powershell.exe",
        }
    }
}

/// macOS 固定 zsh。Windows 使用 PowerShell。其它 Unix 在存在 zsh 时沿用同一套钩子，否则用 bash。
pub fn detect_shell() -> ShellKind {
    if cfg!(target_os = "windows") {
        ShellKind::PowerShell
    } else if cfg!(target_os = "macos") || Path::new("/bin/zsh").is_file() {
        ShellKind::Zsh
    } else {
        ShellKind::Bash
    }
}

/// bash 包装：不改用户配置。用 `--rcfile` 加载本文件。
/// 私有模块只安装到当前 shell；失败时保留原始终端，不能退回 OSC 判定空闲。
pub fn install_bash(base: &Path) -> std::io::Result<PathBuf> {
    let dir = base.join("bash-integration");
    std::fs::create_dir_all(&dir)?;
    let rc = dir.join("bashrc");
    let script = r#"# Fleqi bash integration (generated; do not edit)
if [[ -f "$HOME/.bashrc" ]]; then
  source "$HOME/.bashrc"
fi
if [[ -n ${FLEQI_BASH_BRIDGE-} && -n ${FLEQI_BASH_CONTROL-} && -n ${FLEQI_BASH_INITIAL_CWD-} ]] &&
   builtin enable -f "$FLEQI_BASH_BRIDGE" fleqi_sync 2>/dev/null &&
   builtin fleqi_sync "$FLEQI_BASH_CONTROL" "$FLEQI_BASH_HOST_PID" "$FLEQI_BASH_INITIAL_CWD"; then
  :
else
  builtin printf '%s\n' 'Fleqi: Bash 自动目录同步不可用；请在终端手动操作。' >&2
fi
unset FLEQI_BASH_BRIDGE FLEQI_BASH_CONTROL FLEQI_BASH_HOST_PID FLEQI_BASH_INITIAL_CWD
"#;
    std::fs::write(&rc, script)?;
    Ok(rc)
}

/// PowerShell 包装：不改用户配置。提示符报告 OSC 7331。
pub fn install_powershell(base: &Path) -> std::io::Result<PathBuf> {
    let dir = base.join("powershell-integration");
    std::fs::create_dir_all(&dir)?;
    let script = dir.join("fleqi-profile.ps1");
    let mut body = b"\xef\xbb\xbf".to_vec();
    body.extend_from_slice(include_bytes!("powershell.ps1"));
    std::fs::write(&script, body)?;
    Ok(script)
}

#[cfg(test)]
mod shell_tests {
    use super::*;

    #[test]
    fn bash_and_powershell_control_lines_keep_paths_literal() {
        let path = Path::new("/tmp/a b/'$(whoami)'");
        let bash = String::from_utf8(cd_control_line(path)).unwrap();
        assert!(bash.contains("builtin cd --"));
        assert!(bash.contains("$(whoami)"));
        assert!(!bash.contains("whoami)'\n"));
        let ps = String::from_utf8(powershell_cd_line(path)).unwrap();
        assert!(ps.contains("Set-Location -LiteralPath"));
        assert!(ps.contains("''$(whoami)''"));
        assert!(!ps.contains("Invoke-Expression"));
    }

    #[test]
    fn generated_integrations_emit_private_osc() {
        let dir = tempfile::tempdir().unwrap();
        let bash = std::fs::read_to_string(install_bash(dir.path()).unwrap()).unwrap();
        assert!(bash.contains("builtin enable -f"));
        assert!(bash.contains("builtin fleqi_sync"));
        assert!(!bash.contains("PROMPT_COMMAND="));
        assert!(!bash.contains("edit;len=0"));
        let ps = std::fs::read_to_string(install_powershell(dir.path()).unwrap()).unwrap();
        assert!(ps.contains("NamedPipeClientStream"));
        assert!(ps.contains("PowerShell.OnIdle"));
        assert!(ps.contains("GetBufferState"));
        assert!(!ps.contains("{osc}"));
    }
}
