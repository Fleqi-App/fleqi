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
