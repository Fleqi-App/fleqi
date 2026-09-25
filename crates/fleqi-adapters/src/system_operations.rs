//! 系统与网络能力。外部程序只接收独立参数，系统动作返回受理事实而非猜测结果。
use crate::native_steps::{run_command, run_command_with_input};
use fleqi_application::run_service::NativeOutput;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

type Parameters = BTreeMap<String, String>;

fn value<'a>(parameters: &'a Parameters, key: &str, default: &'a str) -> &'a str {
    parameters.get(key).map(String::as_str).unwrap_or(default)
}

fn integer(
    parameters: &Parameters,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, String> {
    let number = parameters
        .get(key)
        .map_or(Ok(default), |s| s.parse::<u64>())
        .map_err(|_| format!("{key} 必须为整数"))?;
    if !(min..=max).contains(&number) {
        return Err(format!("{key} 范围为 {min}–{max}"));
    }
    Ok(number)
}

fn choice<'a>(
    parameters: &'a Parameters,
    key: &str,
    default: &'a str,
    choices: &[&str],
) -> Result<&'a str, String> {
    let selected = value(parameters, key, default);
    if !choices.contains(&selected) {
        return Err(format!("{key} 选项无效"));
    }
    Ok(selected)
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("已取消".into())
    } else {
        Ok(())
    }
}

fn command(program: &str, args: &[&str], cancel: &AtomicBool) -> Result<String, String> {
    check_cancel(cancel)?;
    let mut command = Command::new(program);
    command.args(args).env("LC_ALL", "C");
    run_command(command, cancel)
}

fn json_command(program: &str, args: &[&str], cancel: &AtomicBool) -> Result<Value, String> {
    serde_json::from_str(&command(program, args, cancel)?)
        .map_err(|e| format!("系统返回的 JSON 无效：{e}"))
}

fn plist_json(plist: &str, cancel: &AtomicBool) -> Result<Value, String> {
    let mut convert = Command::new("/usr/bin/plutil");
    convert.args(["-convert", "json", "-o", "-", "--", "-"]);
    let output = run_command_with_input(convert, Some(plist.as_bytes()), cancel)?;
    serde_json::from_str(&output).map_err(|e| format!("系统 plist 无法解析：{e}"))
}

fn timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn absolute(path: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    path.canonicalize()
        .map_err(|e| format!("{}：{e}", path.display()))
}

/// CAP-SYSTEM-001..015 与 CAP-NETWORK-001..003 的实际执行入口。
pub fn execute(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    parameters: &Parameters,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    check_cancel(cancel)?;
    let output = match operation {
        "CAP-NETWORK-001" => ping(parameters, cancel)?,
        "CAP-NETWORK-002" => download(cwd, parameters, cancel)?,
        "CAP-NETWORK-003" => weather(parameters, cancel)?,
        operation if operation.starts_with("CAP-SYSTEM-") => {
            if !cfg!(target_os = "macos") {
                return Err("此系统能力目前需要 macOS 平台适配".into());
            }
            match operation {
                "CAP-SYSTEM-001" => open_editor(sources, cwd, parameters, cancel)?,
                "CAP-SYSTEM-002" => open_terminal(cwd, parameters, cancel)?,
                "CAP-SYSTEM-003" => eject_volume(cwd, parameters, cancel)?,
                "CAP-SYSTEM-004" => caffeinate(parameters, cancel)?,
                "CAP-SYSTEM-005" => appearance(parameters, cancel)?,
                "CAP-SYSTEM-006" => finder_hidden(parameters, cancel)?,
                "CAP-SYSTEM-007" => print_files(sources, cwd, parameters, cancel)?,
                "CAP-SYSTEM-008" => sleep_request(parameters, cancel)?,
                "CAP-SYSTEM-009" => processor(parameters, cancel)?,
                "CAP-SYSTEM-010" => memory(parameters, cancel)?,
                "CAP-SYSTEM-011" => displays(parameters, cancel)?,
                "CAP-SYSTEM-012" => charger(parameters, cancel)?,
                "CAP-SYSTEM-013" => battery(cancel)?,
                "CAP-SYSTEM-014" => hidden_files(sources, cwd, parameters, cancel)?,
                "CAP-SYSTEM-015" => messages(sources, cwd, parameters, cancel)?,
                _ => return Err("未登记的系统能力".into()),
            }
        }
        _ => return Err("未登记的系统或网络能力".into()),
    };
    Ok(NativeOutput {
        output,
        partial: false,
    })
}

fn open_editor(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Parameters,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if sources.is_empty() {
        return Err("请先选择要打开的文件".into());
    }
    let mut open = Command::new("/usr/bin/open");
    let editor = value(p, "editor", "");
    if editor.trim().is_empty() {
        open.arg("-t");
    } else {
        open.args(["-a", editor]);
    }
    open.arg("--");
    for path in sources {
        open.arg(absolute(path, cwd)?);
    }
    run_command(open, cancel)?;
    Ok(format!(
        "系统已受理在{}打开 {} 个文件的请求",
        if editor.is_empty() {
            "默认文本编辑器"
        } else {
            editor
        },
        sources.len()
    ))
}

// 文件路径经 argv 进入 AppleScript，再由 quoted form 传给新终端的 shell。
// 从不把输入当 AppleScript 源码，也不向已有 PTY 或已有终端标签注入文本。
const TERMINAL_SCRIPT: &str = r#"on run argv
    set destination to item 1 of argv
    tell application "Terminal"
        set targetTab to do script ("cd -- " & quoted form of destination & " && /bin/pwd")
        activate
        return "Terminal 已创建新终端并提交目录切换；目标：" & destination
    end tell
end run"#;

const ITERM_SCRIPT: &str = r#"on run argv
    set destination to item 1 of argv
    tell application "iTerm"
        set targetWindow to (create window with default profile command ("/bin/zsh -l -c " & quoted form of ("cd -- " & quoted form of destination & " && exec /bin/zsh -l")))
        activate
        return "iTerm 已创建新终端；目标：" & destination
    end tell
end run"#;

fn open_terminal(cwd: &Path, p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let target = choice(p, "terminal", "Terminal", &["Terminal", "iTerm"])?;
    let directory = absolute(cwd, cwd)?;
    if !directory.is_dir() {
        return Err("终端目录不是文件夹".into());
    }
    let mut script = Command::new("/usr/bin/osascript");
    script
        .args([
            "-e",
            if target == "Terminal" {
                TERMINAL_SCRIPT
            } else {
                ITERM_SCRIPT
            },
            "--",
        ])
        .arg(directory);
    run_command(script, cancel)
        .map_err(|e| format!("无法打开 {target}；请确认应用已安装且允许自动化：{e}"))
}

fn eject_volume(cwd: &Path, p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let requested = value(p, "volume", "");
    let path = if requested.is_empty() {
        absolute(cwd, cwd)?
    } else {
        absolute(Path::new(requested), cwd)?
    };
    let mut inspect = Command::new("/usr/sbin/diskutil");
    inspect.args(["info", "-plist"]).arg(&path);
    let info = plist_json(&run_command(inspect, cancel)?, cancel)?;
    validate_eject(&info)?;
    let root = plist_json(
        &command("/usr/sbin/diskutil", &["info", "-plist", "/"], cancel)?,
        cancel,
    )?;
    if info["ParentWholeDisk"].is_string() && info["ParentWholeDisk"] == root["ParentWholeDisk"] {
        return Err("拒绝弹出承载当前系统卷的磁盘".into());
    }
    let device = info["DeviceIdentifier"].as_str().ok_or("卷缺少设备标识")?;
    if !device.starts_with("disk") || !device.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("卷设备标识无效".into());
    }
    let result = command("/usr/sbin/diskutil", &["eject", device], cancel)?;
    // diskutil success is an OS acknowledgment; verify the original mount no longer names that device.
    let mount = info["MountPoint"].as_str().ok_or("卷未挂载")?;
    let remaining = command("/usr/sbin/diskutil", &["info", "-plist", mount], cancel)
        .ok()
        .and_then(|text| plist_json(&text, cancel).ok());
    if remaining.as_ref().is_some_and(|v| {
        v["DeviceIdentifier"] == info["DeviceIdentifier"] && v["MountPoint"].as_str() == Some(mount)
    }) {
        return Err("系统返回后卷仍挂载，不能确认弹出成功".into());
    }
    Ok(format!(
        "系统已弹出卷 {}（{}）\n{}",
        path.display(),
        device,
        result.trim()
    ))
}

fn validate_eject(info: &Value) -> Result<(), String> {
    let mount = info["MountPoint"].as_str().unwrap_or("");
    if mount.is_empty()
        || mount == "/"
        || mount.starts_with("/System/")
        || info["Internal"] != false
        || (info["RemovableMediaOrExternalDevice"] != true && info["Ejectable"] != true)
    {
        return Err("拒绝弹出系统卷、内部磁盘或不可移除卷；请选择已挂载的外部可移除卷".into());
    }
    Ok(())
}

/// 不使用通用命令的 30 分钟上限；自身有明确期限，且父进程退出时释放断言。
fn caffeinate(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let seconds = integer(p, "seconds", 3600, 1, 86400)?;
    let kind = choice(p, "type", "idle", &["idle", "display", "system"])?;
    let flag = match kind {
        "display" => "-d",
        "system" => "-s",
        _ => "-i",
    };
    if kind == "system" && !command("/usr/bin/pmset", &["-g", "batt"], cancel)?.contains("AC Power")
    {
        return Err("阻止系统休眠断言需要连接交流电源；可改用 idle 类型".into());
    }
    let mut process = Command::new("/usr/bin/caffeinate");
    process.args([
        flag,
        "-t",
        &seconds.to_string(),
        "-w",
        &std::process::id().to_string(),
    ]);
    let result = run_status(process, cancel, Duration::from_secs(seconds + 10))?;
    if !result.success {
        return Err(format!("保持唤醒失败：{}", result.stderr));
    }
    Ok(format!(
        "保持唤醒任务结束；类型 {kind}，期限 {seconds} 秒；电源断言已随进程退出释放"
    ))
}

const APPEARANCE_SCRIPT: &str = r#"on run argv
    tell application "System Events"
        tell appearance preferences
            set requested to item 1 of argv
            if requested is "toggle" then
                set dark mode to not dark mode
            else
                set dark mode to (requested is "dark")
            end if
            if dark mode then return "系统外观：dark"
            return "系统外观：light"
        end tell
    end tell
end run"#;

fn appearance(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let selected = choice(p, "appearance", "toggle", &["toggle", "light", "dark"])?;
    command(
        "/usr/bin/osascript",
        &["-e", APPEARANCE_SCRIPT, "--", selected],
        cancel,
    )
    .map_err(|e| format!("系统外观未成功设置；请检查系统自动化权限：{e}"))
}

fn finder_hidden(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let visible = choice(p, "visible", "true", &["true", "false"])?;
    command(
        "/usr/bin/defaults",
        &[
            "write",
            "com.apple.finder",
            "AppleShowAllFiles",
            "-bool",
            visible,
        ],
        cancel,
    )?;
    let readback = command(
        "/usr/bin/defaults",
        &["read", "com.apple.finder", "AppleShowAllFiles"],
        cancel,
    )?;
    if (readback.trim() == "1") != (visible == "true") {
        return Err("Finder 显示设置回读不一致".into());
    }
    // Finder caches this setting. Restart is a documented effect in the form description.
    command("/usr/bin/killall", &["Finder"], cancel)
        .map_err(|e| format!("Finder 设置已写入，但刷新失败；需重新启动 Finder：{e}"))?;
    command("/usr/bin/open", &["-a", "Finder"], cancel)?;
    Ok(format!(
        "Finder 隐藏项显示：{visible}；设置已回读，并已重新启动 Finder 刷新窗口"
    ))
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
        && !value.starts_with('-')
}

fn print_files(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Parameters,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if sources.is_empty() {
        return Err("请先选择要打印的文件".into());
    }
    let copies = integer(p, "copies", 1, 1, 999)?;
    let printer = value(p, "printer", "");
    let media = value(p, "media", "");
    let sides = choice(
        p,
        "sides",
        "one-sided",
        &["one-sided", "two-sided-long-edge", "two-sided-short-edge"],
    )?;
    if (!printer.is_empty() && !token(printer)) || (!media.is_empty() && !token(media)) {
        return Err("打印机或纸张参数含无效字符".into());
    }
    let pages = value(p, "pages", "");
    if !pages.is_empty() {
        validate_pages(pages)?;
    }
    let mut print = Command::new("/usr/bin/lp");
    print
        .env("LC_ALL", "C")
        .args(["-n", &copies.to_string(), "-o", &format!("sides={sides}")]);
    if !printer.is_empty() {
        print.args(["-d", printer]);
    }
    if !media.is_empty() {
        print.args(["-o", &format!("media={media}")]);
    }
    if !pages.is_empty() {
        print.args(["-P", pages]);
    }
    print.arg("--");
    for source in sources {
        let path = absolute(source, cwd)?;
        if !path.is_file() {
            return Err("打印输入必须为文件".into());
        }
        print.arg(path);
    }
    let result = run_command(print, cancel)?;
    if result.trim().is_empty() {
        return Err("打印服务未返回任务 ID，不能确认受理".into());
    }
    Ok(format!(
        "打印队列已受理；这不表示纸张已打印完成。\n{}\n份数：{copies}；双面：{sides}；页范围：{}；纸张：{}",
        result.trim(),
        if pages.is_empty() { "全部" } else { pages },
        if media.is_empty() {
            "打印机默认"
        } else {
            media
        }
    ))
}

fn validate_pages(pages: &str) -> Result<(), String> {
    for part in pages.split(',') {
        let mut range = part.split('-');
        let start = range
            .next()
            .unwrap_or("")
            .parse::<u32>()
            .map_err(|_| "打印页范围无效")?;
        let end = range
            .next()
            .map(|v| v.parse::<u32>())
            .transpose()
            .map_err(|_| "打印页范围无效")?
            .unwrap_or(start);
        if start == 0 || end < start || range.next().is_some() {
            return Err("打印页范围无效".into());
        }
    }
    Ok(())
}

fn sleep_request(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let delay = integer(p, "delaySeconds", 0, 0, 86400)?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(delay) {
        check_cancel(cancel)?;
        std::thread::sleep(Duration::from_millis(50));
    }
    check_cancel(cancel)?;
    // RunService 在调用 NativeStepPort 前已持久化计划、参数和 Running 起始时间。
    // 此处的 pmset 退出只能证明请求受理；实际休眠/恢复由宿主系统事件核对。
    command("/usr/bin/pmset", &["sleepnow"], cancel)?;
    Ok("已提交休眠请求，实际进入/恢复待系统事件核对；请求参数与开始时间已记录于任务历史".into())
}

fn processor(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let fields = choice(p, "fields", "all", &["all", "name", "architecture"])?;
    let architecture = command("/usr/sbin/sysctl", &["-n", "hw.machine"], cancel)?;
    let name = match command(
        "/usr/sbin/sysctl",
        &["-n", "machdep.cpu.brand_string"],
        cancel,
    ) {
        Ok(name) if !name.trim().is_empty() => name,
        _ => format!(
            "芯片名称不可读；设备型号 {}",
            command("/usr/sbin/sysctl", &["-n", "hw.model"], cancel)?.trim()
        ),
    };
    Ok(match fields {
        "name" => name.trim().into(),
        "architecture" => architecture.trim().into(),
        _ => format!(
            "CPU/SoC：{}\n架构：{}\n来源：sysctl；读取时间：{}",
            name.trim(),
            architecture.trim(),
            timestamp()
        ),
    })
}

fn memory(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let unit = choice(p, "unit", "GiB", &["bytes", "GiB", "GB"])?;
    let scope = choice(p, "scope", "all", &["total", "usage", "all"])?;
    let total: u64 = command("/usr/sbin/sysctl", &["-n", "hw.memsize"], cancel)?
        .trim()
        .parse()
        .map_err(|_| "无法读取物理内存总量")?;
    let factor = match unit {
        "GiB" => 1_073_741_824.0,
        "GB" => 1_000_000_000.0,
        _ => 1.0,
    };
    let mut result =
        json!({"unit":unit,"source":"sysctl hw.memsize / vm_stat","readAt":timestamp()});
    if scope != "usage" {
        result["total"] = json!(total as f64 / factor);
    }
    if scope != "total" {
        let stats = command("/usr/bin/vm_stat", &[], cancel)?;
        let (page_size, pages) = parse_vm_stat(&stats)?;
        for key in [
            "Pages free",
            "Pages active",
            "Pages inactive",
            "Pages wired down",
            "Pages occupied by compressor",
            "Pages speculative",
        ] {
            let count = pages
                .get(key)
                .ok_or_else(|| format!("系统未提供内存字段 {key}"))?;
            result[key] = json!((*count as f64 * page_size as f64) / factor);
        }
        result["scope"] = json!(
            "分页分类快照；active/inactive 不是应用使用量，不将可回收缓存计为不可用；压缩器项为实际占用物理页"
        );
    }
    Ok(result.to_string())
}

fn parse_vm_stat(text: &str) -> Result<(u64, BTreeMap<String, u64>), String> {
    let page_size = text
        .lines()
        .next()
        .and_then(|s| s.split("page size of ").nth(1))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .ok_or("无法读取内存页大小")?;
    let mut pages = BTreeMap::new();
    for line in text.lines().skip(1) {
        if let Some((key, count)) = line.split_once(':')
            && let Ok(count) = count.trim().trim_end_matches('.').parse::<u64>()
        {
            pages.insert(key.to_string(), count);
        }
    }
    Ok((page_size, pages))
}

fn displays(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let scope = choice(p, "scope", "both", &["both", "physical", "logical"])?;
    let target = value(p, "display", "all");
    let data = json_command(
        "/usr/sbin/system_profiler",
        &["SPDisplaysDataType", "-json"],
        cancel,
    )?;
    let mut displays = vec![];
    if let Some(gpus) = data["SPDisplaysDataType"].as_array() {
        for gpu in gpus {
            if let Some(screens) = gpu["spdisplays_ndrvs"].as_array() {
                for screen in screens {
                    if target != "all" && screen["_spdisplays_displayID"].as_str() != Some(target) {
                        continue;
                    }
                    let mut info = json!({"id":screen["_spdisplays_displayID"],"name":screen["_name"],"online":screen["spdisplays_online"]});
                    if scope != "logical" {
                        info["physicalPixels"] = screen["_spdisplays_pixels"].clone();
                    }
                    if scope != "physical" {
                        info["logicalResolution"] = screen["_spdisplays_resolution"].clone();
                    }
                    if let (Some((px, py)), Some((lx, ly))) = (
                        dimensions(&screen["_spdisplays_pixels"]),
                        dimensions(&screen["_spdisplays_resolution"]),
                    ) && lx > 0
                        && ly > 0
                    {
                        info["pixelToLogicalRatio"] =
                            json!([px as f64 / lx as f64, py as f64 / ly as f64]);
                    }
                    info["scope"] = json!(
                        "system_profiler 报告的像素与逻辑分辨率；字段缺失显示 null，不推断屏幕原生面板尺寸"
                    );
                    displays.push(info);
                }
            }
        }
    }
    if displays.is_empty() {
        return Err("未找到指定显示器或系统未提供显示器信息".into());
    }
    Ok(json!({"source":"system_profiler SPDisplaysDataType","readAt":timestamp(),"displays":displays}).to_string())
}

fn dimensions(value: &Value) -> Option<(u64, u64)> {
    let mut parts = value.as_str()?.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.nth(1)?.parse().ok()?))
}

fn charger(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let scope = choice(p, "scope", "rated", &["rated", "measured"])?;
    let data = json_command(
        "/usr/sbin/system_profiler",
        &["SPPowerDataType", "-json"],
        cancel,
    )?;
    let charger = data["SPPowerDataType"].as_array().and_then(|rows| {
        rows.iter()
            .find(|row| row["_name"] == "sppower_ac_charger_information")
    });
    let Some(charger) = charger else {
        return Ok("系统未提供充电适配器信息；可能无电池或未接电源，功率未知".into());
    };
    if charger["sppower_battery_charger_connected"] != "TRUE" {
        return Ok("当前未连接充电适配器；功率未知".into());
    }
    if scope == "measured" {
        return Ok(
            "已连接适配器；此系统接口未提供有明确口径的实时输入功率，不用额定功率代替实时测量"
                .into(),
        );
    }
    let watts = charger["sppower_ac_charger_watts"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            charger["sppower_ac_charger_watts"]
                .as_u64()
                .map(|n| n.to_string())
        });
    Ok(match watts {
        Some(watts) => format!(
            "适配器报告的额定/协商功率：{watts} W（非实时耗电或电池充电功率）\n来源：system_profiler SPPowerDataType；读取时间：{}",
            timestamp()
        ),
        None => "已连接适配器，但系统没有提供功率字段".into(),
    })
}

fn battery(cancel: &AtomicBool) -> Result<String, String> {
    let text = command("/usr/bin/pmset", &["-g", "batt"], cancel)?;
    let line = text.lines().find(|line| line.contains("InternalBattery"));
    let state = match line {
        None => "系统未报告内置电池，充满时间不适用",
        Some(line) if line.contains("charged;") => "电池已充满",
        Some(line)
            if line.contains("discharging;")
                || line.contains("not charging;")
                || line.contains("finishing charge;") =>
        {
            "当前未正常充电；剩余时间不表示预计充满时间"
        }
        Some(line) if line.contains("charging;") && line.contains("no estimate") => {
            "正在充电；系统暂无充满时间估计"
        }
        Some(line) if line.contains("charging;") => "正在充电；下面的 remaining 是系统估计充满时间",
        _ => "电池状态或充满时间未知；保留系统原始信息",
    };
    Ok(format!(
        "{state}\n{}\n来源：pmset -g batt；读取时间：{}",
        text.trim(),
        timestamp()
    ))
}

fn hidden_files(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Parameters,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if sources.is_empty() {
        return Err("请先选择文件".into());
    }
    let hidden = choice(p, "hidden", "true", &["true", "false"])?;
    let mut results = Vec::new();
    for source in sources {
        check_cancel(cancel)?;
        // 保留最后一个路径组件，修改选中链接自身，而非其目标文件的属性。
        let requested = if source.is_absolute() {
            source.clone()
        } else {
            cwd.join(source)
        };
        let filename = requested.file_name().ok_or("输入缺少文件名")?;
        let path = requested
            .parent()
            .ok_or("输入缺少父目录")?
            .canonicalize()
            .map_err(|e| e.to_string())?
            .join(filename);
        std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let mut change = Command::new("/usr/bin/chflags");
        change.arg("-h");
        change
            .arg(if hidden == "true" {
                "hidden"
            } else {
                "nohidden"
            })
            .arg(&path);
        run_command(change, cancel)?;
        #[cfg(target_os = "macos")]
        {
            use std::os::macos::fs::MetadataExt;
            let flags = std::fs::symlink_metadata(&path)
                .map_err(|e| e.to_string())?
                .st_flags();
            if (flags & libc::UF_HIDDEN != 0) != (hidden == "true") {
                return Err(format!("{}：hidden 属性回读不一致", path.display()));
            }
        }
        results.push(format!("{}：hidden={hidden}", path.display()));
    }
    Ok(format!(
        "{}\nFinder 开启显示隐藏项时仍可看到这些文件；以点开头的名称不会因移除 hidden 属性而变为普通可见文件。",
        results.join("\n")
    ))
}

const MESSAGES_SCRIPT: &str = r#"on run argv
    set recipientHandle to item 1 of argv
    set textBody to item 2 of argv
    tell application "Messages"
        set availableAccounts to every account whose service type is iMessage
        if (count of availableAccounts) is 0 then error "没有已配置的 iMessage 账号"
        set targetAccount to item 1 of availableAccounts
        if connection status of targetAccount is not connected then error "iMessage 账号未连接，请先登录 Messages"
        set targetPerson to participant recipientHandle of targetAccount
        if textBody is not "" then send textBody to targetPerson
        if (count of argv) > 2 then
            repeat with index from 3 to count of argv
                send (POSIX file (item index of argv)) to targetPerson
            end repeat
        end if
    end tell
    return "Messages 已受理发送请求；收件人：" & recipientHandle & "；服务：iMessage。服务受理不代表收件人已收到。"
end run"#;

fn messages(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Parameters,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let send = messages_command(sources, cwd, p)?;
    check_cancel(cancel)?;
    run_command(send, cancel).map_err(|e| format!("Messages 发送未确认；如部分内容已受理，重试可能重复发送。请检查账号、收件人和自动化权限：{e}"))
}

fn messages_command(sources: &[PathBuf], cwd: &Path, p: &Parameters) -> Result<Command, String> {
    choice(p, "service", "iMessage", &["iMessage"])?;
    let recipient = value(p, "recipient", "").trim();
    let phone = recipient.starts_with('+')
        && recipient.len() >= 8
        && recipient[1..].bytes().all(|b| b.is_ascii_digit());
    let email = recipient.split_once('@').is_some_and(|(user, host)| {
        !user.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
    });
    if recipient.len() > 254
        || recipient.contains(|c: char| c.is_whitespace() || c.is_control())
        || (!phone && !email)
    {
        return Err("请输入显式收件人：含国家码的电话号码或 iMessage 邮箱".into());
    }
    let text = value(p, "text", "");
    let attachments = choice(p, "attachmentMode", "none", &["none", "selected"])?;
    if text.trim().is_empty() && (attachments == "none" || sources.is_empty()) {
        return Err("请输入消息正文或明确选择附件".into());
    }
    if attachments == "selected" && sources.is_empty() {
        return Err("发送附件前请先选择文件".into());
    }
    let mut send = Command::new("/usr/bin/osascript");
    send.args(["-e", MESSAGES_SCRIPT, "--", recipient, text]);
    if attachments == "selected" {
        for source in sources {
            let source = absolute(source, cwd)?;
            if !source.is_file() {
                return Err("Messages 附件必须为文件".into());
            }
            std::fs::File::open(&source).map_err(|e| {
                format!(
                    "Messages 附件不可读，未发送正文或附件：{}：{e}",
                    source.display()
                )
            })?;
            send.arg(source);
        }
    }
    Ok(send)
}

struct ProcessResult {
    success: bool,
    stdout: String,
    stderr: String,
}

/// 临时文件接收输出，避免管道堵塞；所有取消/超时/轮询错误都会 kill + wait 回收子进程。
fn run_status(
    mut command: Command,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<ProcessResult, String> {
    check_cancel(cancel)?;
    let mut stdout = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|e| e.to_string())?)
        .stderr(stderr.try_clone().map_err(|e| e.to_string())?);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire) || started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancel.load(Ordering::Acquire) {
                "已取消；子进程已回收"
            } else {
                "运行超时；子进程已回收"
            }
            .into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    let read = |file: &mut std::fs::File| -> Result<String, String> {
        file.rewind().map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    Ok(ProcessResult {
        success: status.success(),
        stdout: read(&mut stdout)?,
        stderr: read(&mut stderr)?,
    })
}

fn ping(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    choice(p, "protocol", "icmp", &["icmp"])?;
    let host = value(p, "host", "");
    if host.is_empty()
        || host.len() > 253
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".:-".contains(&b))
    {
        return Err("请输入有效主机名或 IP 地址；不接受命令参数".into());
    }
    let count = integer(p, "count", 4, 1, 100)?;
    let timeout = integer(p, "timeoutSeconds", 3, 1, 60)?;
    let ipv6 = host.parse::<std::net::Ipv6Addr>().is_ok();
    let mut ping = Command::new(if cfg!(target_os = "macos") {
        if ipv6 { "/sbin/ping6" } else { "/sbin/ping" }
    } else {
        "ping"
    });
    ping.env("LC_ALL", "C").args(["-c", &count.to_string()]);
    if cfg!(target_os = "macos") {
        // ping6 lacks ping's millisecond -W. Both are bounded by the parent deadline.
        if !ipv6 {
            ping.args(["-W", &(timeout * 1000).to_string()]);
        }
    } else {
        ping.args(["-W", &timeout.to_string()]);
    }
    ping.arg(host);
    let result = run_status(ping, cancel, Duration::from_secs(count * (timeout + 1) + 2))?;
    if !result.success {
        return Err(format!(
            "ICMP 探测失败或有丢包；未替换为其他协议。\n{}\n{}",
            result.stdout.trim(),
            result.stderr.trim()
        ));
    }
    Ok(format!(
        "ICMP 探测：{host}；请求 {count} 次，等待上限 {timeout} 秒/次\n{}",
        result.stdout.trim()
    ))
}

fn url(text: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(text).map_err(|_| "HTTP(S) URL 无效")?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        return Err("仅允许 HTTP(S) URL".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL 不应嵌入账号或口令；请使用不含凭据的下载链接".into());
    }
    Ok(url)
}

fn curl(url: &reqwest::Url) -> Command {
    let mut command = Command::new(if cfg!(target_os = "macos") {
        "/usr/bin/curl"
    } else {
        "curl"
    });
    // -q must be first: user curlrc must not inject alternate destinations/protocols.
    command.args([
        "-q",
        "--fail",
        "--location",
        "--max-redirs",
        "5",
        "--proto",
        "=http,https",
        "--proto-redir",
        "=http,https",
        "--connect-timeout",
        "15",
        "--silent",
        "--show-error",
    ]);
    command.arg("--url").arg(url.as_str());
    command
}

fn download(cwd: &Path, p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let source = url(value(p, "url", ""))?;
    let destination = value(p, "destination", "");
    if destination.trim().is_empty() {
        return Err("请指定保存路径".into());
    }
    let collision = choice(p, "collision", "rename", &["rename", "error", "overwrite"])?;
    let requested = cwd.join(destination);
    let name = requested.file_name().ok_or("保存路径必须包含文件名")?;
    let parent = requested
        .parent()
        .ok_or("保存路径缺少父目录")?
        .canonicalize()
        .map_err(|e| format!("保存目录不可用：{e}"))?;
    let mut target = parent.join(name);
    if target.is_dir() {
        return Err("保存路径是目录，请指定文件名".into());
    }
    if target.exists() {
        match collision {
            "error" => return Err("同名文件已存在，未改动原文件".into()),
            "rename" => target = crate::capabilities::unique_destination(&parent, Path::new(name)),
            _ => (),
        }
    }
    // Named temporary file is in the destination filesystem: publish only after verified success.
    let mut temporary = tempfile::Builder::new()
        .prefix(".fleqi-download-")
        .tempfile_in(&parent)
        .map_err(|e| e.to_string())?;
    let mut download = curl(&source);
    download
        .args(["--max-time", "1800", "--output"])
        .arg(temporary.path())
        .args(["--write-out", "%{json}"]);
    let result =
        run_command(download, cancel).map_err(|e| format!("下载失败；未发布临时内容：{e}"))?;
    check_cancel(cancel)?;
    let transfer: Value =
        serde_json::from_str(&result).map_err(|e| format!("无法核对下载结果：{e}"))?;
    let status = transfer["http_code"].as_u64().ok_or("下载缺少 HTTP 状态")?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}；未发布下载文件"));
    }
    temporary.flush().map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    let size = temporary
        .as_file()
        .metadata()
        .map_err(|e| e.to_string())?
        .len();
    check_cancel(cancel)?;
    if collision == "overwrite" {
        temporary.persist(&target).map_err(|e| e.to_string())?;
    } else {
        temporary
            .persist_noclobber(&target)
            .map_err(|e| format!("保存失败，未覆盖已有文件：{e}"))?;
    }
    // 结果中的路径仅用于显示，不能用作后续 PathRef；非 UTF-8 的父目录也不会使 JSON 序列化 panic。
    Ok(json!({"file":target.to_string_lossy(),"pathDisplayLossy":target.to_str().is_none(),"bytes":size,"source":source.as_str(),"resolvedSource":transfer["url_effective"],"httpStatus":status,"status":"completed","progress":1.0,"completedAt":timestamp()}).to_string())
}

fn fetch_json(url: &reqwest::Url, cancel: &AtomicBool) -> Result<Value, String> {
    let mut request = curl(url);
    request.args(["--max-time", "30", "--max-filesize", "2097152"]);
    let result = run_command(request, cancel)?;
    let data: Value =
        serde_json::from_str(&result).map_err(|e| format!("天气服务返回无效 JSON：{e}"))?;
    if data["error"] == true {
        return Err(format!("天气服务失败：{}", data["reason"]));
    }
    Ok(data)
}

fn weather(p: &Parameters, cancel: &AtomicBool) -> Result<String, String> {
    let location = value(p, "location", "").trim();
    if location.is_empty() {
        return Err("请输入地点或纬度,经度".into());
    }
    let unit = choice(p, "unit", "celsius", &["celsius", "fahrenheit"])?;
    let range = choice(p, "range", "current", &["current", "forecast"])?;
    let days = integer(p, "days", 3, 1, 16)?;
    let start = value(p, "startDate", "");
    let end = value(p, "endDate", "");
    if start.is_empty() != end.is_empty() {
        return Err("指定日期范围需要同时填写开始与结束日期".into());
    }
    if !start.is_empty() && (range != "forecast" || valid_date(start)? > valid_date(end)?) {
        return Err("日期范围无效；请使用预报模式且开始不晚于结束".into());
    }
    let (lat, lon, place) = if let Some((lat, lon)) = coordinates(location) {
        (
            lat,
            lon,
            json!({"name":location,"latitude":lat,"longitude":lon,"resolution":"explicitCoordinates"}),
        )
    } else {
        let mut search = url("https://geocoding-api.open-meteo.com/v1/search")?;
        search
            .query_pairs_mut()
            .append_pair("name", location)
            .append_pair("count", "10")
            .append_pair("language", "zh")
            .append_pair("format", "json");
        let data = fetch_json(&search, cancel)?;
        let place = select_place(&data, value(p, "locationId", ""))?;
        let lat = place["latitude"].as_f64().ok_or("地点缺少纬度")?;
        let lon = place["longitude"].as_f64().ok_or("地点缺少经度")?;
        if !lat.is_finite()
            || !lon.is_finite()
            || !(-90.0..=90.0).contains(&lat)
            || !(-180.0..=180.0).contains(&lon)
        {
            return Err("地点坐标无效".into());
        }
        (lat, lon, place)
    };
    let mut forecast = url("https://api.open-meteo.com/v1/forecast")?;
    {
        let mut query = forecast.query_pairs_mut();
        query
            .append_pair("latitude", &lat.to_string())
            .append_pair("longitude", &lon.to_string())
            .append_pair("temperature_unit", unit)
            .append_pair("timezone", "auto");
        if range == "current" {
            query.append_pair(
                "current",
                "temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m",
            );
        } else {
            query.append_pair(
                "daily",
                "temperature_2m_max,temperature_2m_min,weather_code,precipitation_probability_max",
            );
            if start.is_empty() {
                query.append_pair("forecast_days", &days.to_string());
            } else {
                query
                    .append_pair("start_date", start)
                    .append_pair("end_date", end);
            }
        }
    }
    let data = fetch_json(&forecast, cancel)?;
    let section = if range == "current" {
        "current"
    } else {
        "daily"
    };
    if data[section]["time"].is_null() {
        return Err("天气服务缺少观测/预报时间，不能作为有效结果".into());
    }
    Ok(json!({"location":place,"source":"Open-Meteo (https://open-meteo.com/)","sourceUrl":forecast.as_str(),"retrievedAt":timestamp(),"scope":"模型天气分析/预报；不是本地传感器实测","data":data}).to_string())
}

fn coordinates(location: &str) -> Option<(f64, f64)> {
    let (lat, lon) = location.split_once(',')?;
    let (lat, lon) = (
        lat.trim().parse::<f64>().ok()?,
        lon.trim().parse::<f64>().ok()?,
    );
    if lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
    {
        Some((lat, lon))
    } else {
        None
    }
}

fn select_place(data: &Value, id: &str) -> Result<Value, String> {
    let candidates = data["results"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("没有找到该地点；请提供更完整的名称或纬度,经度")?;
    if !id.is_empty() {
        let id: u64 = id.parse().map_err(|_| "地点 ID 无效")?;
        return candidates
            .iter()
            .find(|p| p["id"].as_u64() == Some(id))
            .cloned()
            .ok_or_else(|| "地点 ID 不在搜索候选中，请按当前结果重新选择".into());
    }
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    let summaries = candidates.iter().map(|p| json!({"id":p["id"],"name":p["name"],"country":p["country"],"region":p["admin1"],"latitude":p["latitude"],"longitude":p["longitude"]})).collect::<Vec<_>>();
    Err(format!(
        "地点存在歧义，请填写候选 locationId 或明确纬度,经度后重试：{}",
        json!(summaries)
    ))
}

fn valid_date(value: &str) -> Result<time::Date, String> {
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return Err("日期格式应为 YYYY-MM-DD".into());
    }
    let year = parts[0].parse().map_err(|_| "年份无效")?;
    let month = time::Month::try_from(parts[1].parse::<u8>().map_err(|_| "月份无效")?)
        .map_err(|_| "月份无效")?;
    let day = parts[2].parse().map_err(|_| "日期无效")?;
    time::Date::from_calendar_date(year, month, day).map_err(|_| "日期无效".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_content_and_attachments_remain_separate_argv_values() {
        let temporary = tempfile::tempdir().unwrap();
        let attachment = temporary.path().join("quoted ' 中文 $(not_a_command).txt");
        std::fs::write(&attachment, "test attachment").unwrap();
        let body = "\" & do shell script \"touch forbidden\"\nend run";
        let parameters = BTreeMap::from([
            ("recipient".into(), "+15555550123".into()),
            ("text".into(), body.into()),
            ("attachmentMode".into(), "selected".into()),
        ]);
        let command = messages_command(
            std::slice::from_ref(&attachment),
            temporary.path(),
            &parameters,
        )
        .unwrap();
        let args = command.get_args().collect::<Vec<_>>();
        assert_eq!(args.len(), 6);
        assert_eq!(args[1], MESSAGES_SCRIPT);
        assert_eq!(args[3], "+15555550123");
        assert_eq!(args[4], body);
        assert_eq!(args[5], attachment.canonicalize().unwrap().as_os_str());
        assert!(
            messages_command(
                &[temporary.path().join("missing")],
                temporary.path(),
                &parameters
            )
            .is_err()
        );
        assert!(
            messages_command(
                &[temporary.path().to_path_buf()],
                temporary.path(),
                &parameters
            )
            .is_err()
        );
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 1);
    }

    #[test]
    fn weather_requires_ambiguity_resolution_and_valid_time_range() {
        let candidates = json!({"results":[
            {"id":1,"name":"Springfield","country":"US","admin1":"A","latitude":10.0,"longitude":20.0},
            {"id":2,"name":"Springfield","country":"US","admin1":"B","latitude":30.0,"longitude":40.0}
        ]});
        let error = select_place(&candidates, "").unwrap_err();
        assert!(
            error.contains("歧义") && error.contains("locationId") && error.contains("latitude")
        );
        assert_eq!(select_place(&candidates, "2").unwrap()["admin1"], "B");
        assert!(select_place(&candidates, "999").is_err());
        assert!(select_place(&json!({}), "").is_err());
        assert_eq!(coordinates("-33.86,151.2"), Some((-33.86, 151.2)));
        assert!(coordinates("91,0").is_none());
        assert!(valid_date("2026-02-29").is_err());
        assert!(valid_date("2028-02-29").is_ok());
        assert!(valid_date("2026-2-1").is_err());
    }

    #[test]
    fn missing_ejection_facts_fail_closed() {
        for facts in [
            json!({}),
            json!({"MountPoint":"/","Internal":false,"Ejectable":true}),
            json!({"MountPoint":"/Volumes/External","Internal":true,"Ejectable":true}),
            json!({"MountPoint":"/Volumes/External","Ejectable":true}),
        ] {
            assert!(validate_eject(&facts).is_err());
        }
        assert!(
            validate_eject(
                &json!({"MountPoint":"/Volumes/External","Internal":false,"Ejectable":true})
            )
            .is_ok()
        );
    }

    #[test]
    fn print_options_are_values_and_ranges_must_be_ordered() {
        for pages in ["1", "1,3-5", "10-10"] {
            assert!(validate_pages(pages).is_ok());
        }
        for pages in ["", "0", "3-1", "1-2-3", "1 - 3", "1;touch x"] {
            assert!(validate_pages(pages).is_err());
        }
        assert!(!token("A4 sides=two-sided-long-edge"));
        assert!(!token("--help"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fixed_applescripts_compile_without_executing_system_actions() {
        let temporary = tempfile::tempdir().unwrap();
        for (name, script) in [
            ("terminal", TERMINAL_SCRIPT),
            ("appearance", APPEARANCE_SCRIPT),
            ("messages", MESSAGES_SCRIPT),
        ] {
            let source = temporary.path().join(format!("{name}.applescript"));
            std::fs::write(&source, script).unwrap();
            let output = Command::new("/usr/bin/osacompile")
                .arg("-o")
                .arg(temporary.path().join(format!("{name}.scpt")))
                .arg(source)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
