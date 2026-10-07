//! 文件、Git、工具及计算能力。所有路径保留原生表示，子进程参数不经过 shell。
use crate::{
    capabilities::unique_destination,
    image_operations::tool,
    native_steps::{run_command, run_command_with_input},
};
use fleqi_application::run_service::NativeOutput;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

type Params = BTreeMap<String, String>;
fn value<'a>(p: &'a Params, key: &str, default: &'a str) -> &'a str {
    p.get(key).map(String::as_str).unwrap_or(default)
}
fn choice<'a>(
    p: &'a Params,
    key: &str,
    default: &'a str,
    allowed: &[&str],
) -> Result<&'a str, String> {
    let v = value(p, key, default);
    if allowed.contains(&v) {
        Ok(v)
    } else {
        Err(format!("{key} 参数无效：{v}"))
    }
}
fn boolean(p: &Params, key: &str, default: bool) -> Result<bool, String> {
    Ok(choice(
        p,
        key,
        if default { "true" } else { "false" },
        &["true", "false"],
    )? == "true")
}
fn integer(p: &Params, key: &str, default: &str, min: usize, max: usize) -> Result<usize, String> {
    let n = value(p, key, default)
        .parse::<usize>()
        .map_err(|_| format!("{key} 必须是整数"))?;
    if (min..=max).contains(&n) {
        Ok(n)
    } else {
        Err(format!("{key} 范围为 {min}–{max}"))
    }
}
fn number(p: &Params, key: &str, default: &str) -> Result<f64, String> {
    let n = value(p, key, default)
        .parse::<f64>()
        .map_err(|_| format!("{key} 必须为数值"))?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(format!("{key} 必须为有限数值"))
    }
}
fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("已取消".into())
    } else {
        Ok(())
    }
}
fn ok(output: String) -> NativeOutput {
    NativeOutput {
        output,
        partial: false,
    }
}
fn safe_name(name: &str) -> Result<&str, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        Err("名称不能为空或包含路径分隔符".into())
    } else {
        Ok(name)
    }
}
#[derive(Default)]
struct Report {
    lines: Vec<String>,
    failures: Vec<String>,
}
impl Report {
    fn fail(&mut self, path: &Path, error: impl std::fmt::Display) {
        self.failures.push(format!("{}：{error}", path.display()));
    }
    fn finish(mut self) -> NativeOutput {
        let partial = !self.failures.is_empty();
        if partial {
            self.lines.push(format!(
                "结果不完整：未完成/不可读取（{} 项）：\n{}",
                self.failures.len(),
                self.failures.join("\n")
            ));
        }
        NativeOutput {
            output: self.lines.join("\n"),
            partial,
        }
    }
}
struct Walk {
    files: Vec<PathBuf>,
    report: Report,
}
fn walk(
    roots: &[PathBuf],
    recursive: bool,
    hidden: bool,
    excluded: &BTreeSet<String>,
    cancel: &AtomicBool,
) -> Result<Walk, String> {
    let mut result = Walk {
        files: vec![],
        report: Report::default(),
    };
    let mut pending = roots
        .iter()
        .map(|p| (p.clone(), 0usize))
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let mut skipped = 0usize;
    while let Some((path, depth)) = pending.pop() {
        check(cancel)?;
        if !seen.insert(path.clone()) {
            continue;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                result.report.fail(&path, e);
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            skipped += 1;
            continue;
        }
        if meta.is_file() {
            result.files.push(path);
            continue;
        }
        if !meta.is_dir() {
            result.report.fail(&path, "不是普通文件或目录");
            continue;
        }
        if depth > 0 && !recursive {
            continue;
        }
        let entries = match fs::read_dir(&path) {
            Ok(e) => e,
            Err(e) => {
                result.report.fail(&path, e);
                continue;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let name = entry.file_name();
                    let s = name.to_string_lossy();
                    if (!hidden && s.starts_with('.')) || excluded.contains(s.as_ref()) {
                        continue;
                    }
                    pending.push((entry.path(), depth + 1));
                }
                Err(e) => result.report.fail(&path, e),
            }
        }
    }
    result.files.sort();
    if skipped > 0 {
        result
            .report
            .lines
            .push(format!("已按范围规则跳过 {skipped} 个符号链接；不跟随链接"));
    }
    Ok(result)
}
fn roots(sources: &[PathBuf], cwd: &Path, p: &Params) -> Vec<PathBuf> {
    if !sources.is_empty() {
        sources.to_vec()
    } else {
        vec![cwd.join(value(p, "directory", "."))]
    }
}
fn walk_params(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<Walk, String> {
    walk(
        &roots(sources, cwd, p),
        boolean(p, "recursive", true)?,
        boolean(p, "includeHidden", false)?,
        &BTreeSet::new(),
        cancel,
    )
}
fn hash(path: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        check(cancel)?;
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn sizes(path: &Path) -> Result<(u64, u64), String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let allocated = {
        use std::os::unix::fs::MetadataExt;
        m.blocks().checked_mul(512).ok_or("占用大小溢出")?
    };
    #[cfg(not(unix))]
    let allocated = m.len();
    Ok((m.len(), allocated))
}
fn display_size(bytes: u64, unit: &str) -> Result<String, String> {
    let factor = match unit {
        "bytes" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1048576.0,
        "GiB" => 1073741824.0,
        _ => return Err("单位必须为 bytes/KiB/MiB/GiB".into()),
    };
    Ok(format!(
        "{:.3} {unit} ({bytes} bytes)",
        bytes as f64 / factor
    ))
}

pub fn execute(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    parameters: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    check(cancel)?;
    if operation.starts_with("CAP-CALC-") {
        return calculate(operation, parameters).map(ok);
    }
    if operation.starts_with("CAP-DEV-")
        && matches!(
            operation,
            "CAP-DEV-001" | "CAP-DEV-002" | "CAP-DEV-003" | "CAP-DEV-004"
        )
    {
        return git(operation, sources, cwd, parameters, cancel);
    }
    if operation.starts_with("CAP-TOOLS-") {
        return brew(operation, parameters, cancel);
    }
    match operation {
        "CAP-FILE-005" | "CAP-FILE-006" => rename(operation, sources, parameters, cancel),
        "CAP-FILE-007" => organize(sources, cwd, parameters, cancel),
        "CAP-FILE-009" => {
            require_sources(sources)?;
            let mut r = Report::default();
            for source in sources {
                check(cancel)?;
                let mut command = Command::new(tool("file")?);
                command.args(["-b", "--mime-type", "--"]).arg(source);
                match run_command(command, cancel) {
                    Ok(kind) => r.lines.push(format!(
                        "{}：内容探测 MIME={}；扩展名={}；{}",
                        source.display(),
                        kind.trim(),
                        source.extension().unwrap_or_default().to_string_lossy(),
                        compare_extension(source, kind.trim())
                    )),
                    Err(e) => r.fail(source, e),
                }
            }
            Ok(r.finish())
        }
        "CAP-FILE-010" => {
            require_sources(sources)?;
            let unit = value(parameters, "unit", "bytes");
            let mut r = Report::default();
            for path in sources {
                match sizes(path) {
                    Ok((logical, allocated)) => r.lines.push(format!(
                        "{}：逻辑大小 {}；占用空间 {}",
                        path.display(),
                        display_size(logical, unit)?,
                        display_size(allocated, unit)?
                    )),
                    Err(e) => r.fail(path, e),
                }
            }
            Ok(r.finish())
        }
        "CAP-FILE-011" | "CAP-FILE-013" | "CAP-FILE-014" => {
            inspect_tree(operation, sources, cwd, parameters, cancel)
        }
        "CAP-FILE-015" => download_source(sources, cancel),
        "CAP-FILE-016" => properties(sources, cancel),
        "CAP-FILE-017" | "CAP-FILE-018" | "CAP-FILE-019" | "CAP-FILE-020" => {
            find(operation, sources, cwd, parameters, cancel)
        }
        "CAP-DEV-005" => count_lines(sources, cwd, parameters, cancel),
        "CAP-DEV-006" => {
            require_sources(sources)?;
            choice(parameters, "algorithm", "sha256", &["sha256"])?;
            let style = choice(parameters, "format", "standard", &["standard", "json"])?;
            let mut r = Report::default();
            for path in sources {
                match hash(path, cancel) {
                    Ok(digest) => r.lines.push(if style == "json" {
                        serde_json::json!({"path":path,"algorithm":"SHA-256","digest":digest})
                            .to_string()
                    } else {
                        format!("{digest}  {}", path.display())
                    }),
                    Err(e) => r.fail(path, e),
                }
            }
            Ok(r.finish())
        }
        "CAP-DEV-007" => remove_quarantine(sources, cancel),
        "CAP-ZIP-004" => zip_ratio(sources, parameters, cancel),
        "CAP-ZIP-005" => transfer_archive(sources, cwd, parameters, cancel),
        _ => Err(format!("未登记的文件操作 {operation}")),
    }
}
fn require_sources(sources: &[PathBuf]) -> Result<(), String> {
    if sources.is_empty() {
        Err("请先选择文件".into())
    } else {
        Ok(())
    }
}
fn inspect_tree(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    let mut w = walk_params(sources, cwd, p, cancel)?;
    let scope = choice(p, "sizeKind", "logical", &["logical", "allocated"])?;
    let unit = value(p, "unit", "bytes");
    display_size(0, unit)?;
    if operation == "CAP-FILE-014" {
        let mut groups: BTreeMap<(u64, String), Vec<PathBuf>> = BTreeMap::new();
        for path in &w.files {
            match sizes(path).and_then(|(size, _)| hash(path, cancel).map(|digest| (size, digest)))
            {
                Ok(key) => groups.entry(key).or_default().push(path.clone()),
                Err(e) => w.report.fail(path, e),
            }
        }
        let mut count = 0;
        for ((size, digest), paths) in groups.into_iter().filter(|(_, p)| p.len() > 1) {
            count += 1;
            w.report.lines.push(format!(
                "重复组 {count}：{size} bytes；SHA-256 {digest}\n{}",
                paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        w.report
            .lines
            .push(format!("内容重复组共 {count} 组；未删除任何文件"));
    } else {
        let mut entries = vec![];
        let mut total = 0u64;
        for path in &w.files {
            match sizes(path) {
                Ok((logical, allocated)) => {
                    let size = if scope == "allocated" {
                        allocated
                    } else {
                        logical
                    };
                    total = total.checked_add(size).ok_or("总大小溢出")?;
                    entries.push((size, path));
                }
                Err(e) => w.report.fail(path, e),
            }
        }
        if operation == "CAP-FILE-013" {
            let limit = integer(p, "limit", "20", 1, 10000)?;
            entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
            for (size, path) in entries.iter().take(limit) {
                w.report.lines.push(format!(
                    "{}\t{}",
                    display_size(*size, unit)?,
                    path.display()
                ));
            }
        }
        w.report.lines.push(format!(
            "已读取 {} 个文件；{scope} 合计 {}；递归={}；包含隐藏项={}；符号链接不递归",
            entries.len(),
            display_size(total, unit)?,
            boolean(p, "recursive", true)?,
            boolean(p, "includeHidden", false)?
        ));
    }
    Ok(w.report.finish())
}
fn extensions(p: &Params, default: &str) -> Result<BTreeSet<String>, String> {
    let set = value(p, "extensions", default)
        .split(',')
        .map(|s| s.trim().trim_start_matches('.').to_lowercase())
        .collect::<BTreeSet<_>>();
    if set.is_empty()
        || set
            .iter()
            .any(|s| s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return Err("扩展名使用逗号分隔，不能包含路径或通配符".into());
    }
    Ok(set)
}
fn matches_extension(path: &Path, extensions: &BTreeSet<String>) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| extensions.contains(&s.to_lowercase()))
}
fn find(
    operation: &str,
    _sources: &[PathBuf],
    cwd: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    let pdf = matches!(operation, "CAP-FILE-019" | "CAP-FILE-020");
    let extensions = if pdf {
        BTreeSet::from(["pdf".into()])
    } else {
        extensions(p, "mp4")?
    };
    let recursive =
        operation != "CAP-FILE-017" && boolean(p, "recursive", operation != "CAP-FILE-019")?;
    let mut w = walk(
        &[cwd.join(value(p, "directory", "."))],
        recursive,
        boolean(p, "includeHidden", false)?,
        &BTreeSet::new(),
        cancel,
    )?;
    let action = choice(p, "action", "list", &["list", "reveal"])?;
    let sensitive = boolean(p, "caseSensitive", false)?;
    let keyword = value(p, "keyword", "");
    if pdf && keyword.trim().is_empty() {
        return Err("请输入正文关键词".into());
    }
    let keyword = if sensitive {
        keyword.to_owned()
    } else {
        keyword.to_lowercase()
    };
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in &w.files {
        if !matches_extension(path, &extensions) {
            continue;
        }
        check(cancel)?;
        if pdf {
            let ocr = choice(p, "ocr", "auto", &["auto", "never"])? == "auto";
            match crate::pdf_operations::text_for_summary(
                path,
                "",
                "",
                ocr,
                value(p, "ocrLanguage", "chi_sim+eng"),
                cancel,
            ) {
                Ok(content) => {
                    if content.partial {
                        w.report
                            .fail(path, format!("正文检索范围不完整：{}", content.scope));
                    }
                    let text = if sensitive {
                        content.text
                    } else {
                        content.text.to_lowercase()
                    };
                    if !text.contains(&keyword) {
                        continue;
                    }
                }
                Err(e) => {
                    w.report.fail(path, e);
                    continue;
                }
            }
        }
        groups
            .entry(path.parent().unwrap_or(cwd).to_path_buf())
            .or_default()
            .push(path.clone());
    }
    let count = groups.values().map(Vec::len).sum::<usize>();
    for (directory, paths) in groups {
        w.report.lines.push(format!(
            "目录 {}：\n{}",
            directory.display(),
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        ));
        if action == "reveal" {
            match reveal(&paths, cancel) {
                Ok(()) => w
                    .report
                    .lines
                    .push("已在 Finder 定位本组；跨目录分组依次定位，最后一组为当前选择".into()),
                Err(e) => w.report.fail(&directory, e),
            }
        }
    }
    w.report.lines.push(format!(
        "命中 {count} 个文件；范围为指定目录；递归={recursive}；后缀不区分大小写；符号链接不跟随"
    ));
    Ok(w.report.finish())
}
fn reveal(paths: &[PathBuf], cancel: &AtomicBool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let directory = paths
            .first()
            .and_then(|path| path.parent())
            .ok_or("没有可在文件管理器中打开的目录")?;
        let mut cmd = Command::new("xdg-open");
        cmd.arg(directory);
        run_command(cmd, cancel).map(|_| ())
    }
    #[cfg(target_os = "windows")]
    {
        let target = paths.first().ok_or("没有可在资源管理器中打开的路径")?;
        let mut cmd = Command::new("explorer.exe");
        cmd.arg(target);
        run_command(cmd, cancel).map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        let script = "on run argv\nset targets to {}\nrepeat with p in argv\nset end of targets to POSIX file (contents of p) as alias\nend repeat\ntell application \"Finder\"\nreveal targets\nactivate\nend tell\nend run";
        let mut cmd = Command::new("/usr/bin/osascript");
        cmd.args(["-e", script, "--"]).args(paths);
        run_command(cmd, cancel).map(|_| ())
    }
}
fn count_lines(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    let extensions = extensions(p, "js,jsx,mjs,cjs")?;
    choice(p, "countKind", "physical", &["physical", "nonBlank"])?;
    let excluded = value(p, "exclude", "node_modules,.git,target,vendor,dist,build")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut w = walk(
        &roots(sources, cwd, p),
        boolean(p, "recursive", true)?,
        boolean(p, "includeHidden", false)?,
        &excluded,
        cancel,
    )?;
    let mut total = 0u64;
    let mut files = 0usize;
    for path in &w.files {
        if !matches_extension(path, &extensions) {
            continue;
        }
        let result = (|| {
            let file = fs::File::open(path).map_err(|e| e.to_string())?;
            let mut count = 0u64;
            for line in BufReader::new(file).lines() {
                check(cancel)?;
                let line = line.map_err(|e| e.to_string())?;
                if value(p, "countKind", "physical") == "physical" || !line.trim().is_empty() {
                    count = count.checked_add(1).ok_or("行数溢出")?;
                }
            }
            Ok::<_, String>(count)
        })();
        match result {
            Ok(count) => {
                total = total.checked_add(count).ok_or("行数溢出")?;
                files += 1;
                w.report
                    .lines
                    .push(format!("{}：{count} 行", path.display()));
            }
            Err(e) => w.report.fail(path, e),
        }
    }
    w.report.lines.push(format!(
        "共 {files} 文件，{total} 行；口径={}；扩展名={}；排除目录={}；不跟随符号链接",
        value(p, "countKind", "physical"),
        extensions.into_iter().collect::<Vec<_>>().join(","),
        excluded.into_iter().collect::<Vec<_>>().join(",")
    ));
    Ok(w.report.finish())
}
fn xattr_names(path: &Path, cancel: &AtomicBool) -> Result<BTreeSet<String>, String> {
    let mut cmd = Command::new(tool("xattr")?);
    cmd.arg(path);
    run_command(cmd, cancel).map(|s| s.lines().map(str::to_owned).collect())
}
fn xattr_hex(path: &Path, key: &str, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(tool("xattr")?);
    cmd.args(["-px", key]).arg(path);
    let hex = run_command(cmd, cancel)?
        .split_whitespace()
        .collect::<String>();
    if hex.len() % 2 != 0 {
        return Err("扩展属性十六进制数据不完整".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| "扩展属性数据无效".into()))
        .collect()
}
fn download_source(sources: &[PathBuf], cancel: &AtomicBool) -> Result<NativeOutput, String> {
    require_sources(sources)?;
    let mut report = Report::default();
    for path in sources {
        check(cancel)?;
        let result = (|| {
            let names = xattr_names(path, cancel)?;
            let mut lines = vec![];
            let key = "com.apple.metadata:kMDItemWhereFroms";
            if names.contains(key) {
                let bytes = xattr_hex(path, key, cancel)?;
                let mut cmd = Command::new(tool("plutil")?);
                cmd.args(["-convert", "json", "-o", "-", "-"]);
                let json = run_command_with_input(cmd, Some(&bytes), cancel)?;
                let urls: serde_json::Value =
                    serde_json::from_str(&json).map_err(|e| e.to_string())?;
                lines.push(format!("系统保存的来源 URL：{urls}"));
            } else {
                lines.push("来源 URL：未记录".into());
            }
            if names.contains("com.apple.quarantine") {
                let bytes = xattr_hex(path, "com.apple.quarantine", cancel)?;
                let value = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                if let Some(timestamp) = value
                    .split(';')
                    .nth(1)
                    .and_then(|s| i64::from_str_radix(s, 16).ok())
                {
                    let date = time::OffsetDateTime::from_unix_timestamp(timestamp)
                        .map_err(|e| e.to_string())?;
                    lines.push(format!("隔离记录时间（UTC；不代表拍摄时间）：{date}"));
                } else {
                    lines.push("隔离记录存在，时间字段无法解析".into());
                }
            } else {
                lines.push("下载时间：未记录".into());
            }
            Ok::<_, String>(lines.join("\n"))
        })();
        match result {
            Ok(s) => report.lines.push(format!("{}\n{s}", path.display())),
            Err(e) => report.fail(path, e),
        }
    }
    Ok(report.finish())
}
fn properties(sources: &[PathBuf], cancel: &AtomicBool) -> Result<NativeOutput, String> {
    require_sources(sources)?;
    let mut report = Report::default();
    for path in sources {
        check(cancel)?;
        let result = (|| {
            let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            let mut facts = vec![
                format!("只读权限标记={}", metadata.permissions().readonly()),
                format!("符号链接={}", metadata.file_type().is_symlink()),
                format!(
                    "点文件={}",
                    path.file_name()
                        .is_some_and(|s| s.to_string_lossy().starts_with('.'))
                ),
            ];
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                facts.push(format!(
                    "权限 mode={:o}；uid={}；gid={}",
                    metadata.mode() & 0o7777,
                    metadata.uid(),
                    metadata.gid()
                ));
            }
            #[cfg(target_os = "macos")]
            {
                use std::os::macos::fs::MetadataExt;
                let flags = metadata.st_flags();
                facts.push(format!(
                    "系统 flags=0x{flags:x}；hidden={}；dataless={}",
                    flags & 0x8000 != 0,
                    flags & 0x40000000 != 0
                ));
            }
            let names = xattr_names(path, cancel)?;
            facts.push(format!(
                "扩展属性：{}",
                names.into_iter().collect::<Vec<_>>().join(", ")
            ));
            facts.push("以上为可读取属性。Finder 灰显原因无法仅据这些字段确定；请结合权限、隐藏设置及云文件下载状态核对。".into());
            Ok::<_, String>(facts.join("\n"))
        })();
        match result {
            Ok(s) => report.lines.push(format!("{}\n{s}", path.display())),
            Err(e) => report.fail(path, e),
        }
    }
    Ok(report.finish())
}
fn remove_quarantine(sources: &[PathBuf], cancel: &AtomicBool) -> Result<NativeOutput, String> {
    require_sources(sources)?;
    let mut r = Report::default();
    for path in sources {
        check(cancel)?;
        let result = (|| {
            if fs::symlink_metadata(path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("符号链接不在修改范围内".into());
            }
            if !xattr_names(path, cancel)?.contains("com.apple.quarantine") {
                return Ok("隔离属性不存在，未修改".to_owned());
            }
            let mut cmd = Command::new(tool("xattr")?);
            cmd.args(["-d", "com.apple.quarantine"]).arg(path);
            run_command(cmd, cancel)?;
            if xattr_names(path, cancel)?.contains("com.apple.quarantine") {
                return Err("属性仍存在，未确认移除成功".into());
            }
            Ok::<_, String>("已移除 com.apple.quarantine；未递归修改目录内容或其他属性".into())
        })();
        match result {
            Ok(s) => r.lines.push(format!("{}：{s}", path.display())),
            Err(e) => r.fail(path, e),
        }
    }
    Ok(r.finish())
}
fn git_command(cwd: &Path) -> Result<Command, String> {
    let mut cmd = Command::new(tool("git")?);
    cmd.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR");
    Ok(cmd)
}
fn git_run(cwd: &Path, args: &[&str], cancel: &AtomicBool) -> Result<String, String> {
    let mut cmd = git_command(cwd)?;
    cmd.args(args);
    run_command(cmd, cancel)
}
fn git_name<'a>(name: &'a str, label: &str) -> Result<&'a str, String> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_control) {
        Err(format!("{label} 不能为空、以 - 开头或包含控制字符"))
    } else {
        Ok(name)
    }
}
fn git(
    operation: &str,
    sources: &[PathBuf],
    cwd: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    let directory = cwd.join(value(p, "directory", "."));
    if !directory.is_dir() {
        return Err("Git 工作目录不存在或不可访问".into());
    }
    if operation == "CAP-DEV-001" {
        return match git_run(&directory, &["rev-parse", "--is-bare-repository"], cancel) {
            Ok(is_bare) if is_bare.trim() == "true" => Ok(ok(format!(
                "是 Git 裸仓库；Git 目录：{}",
                git_run(&directory, &["rev-parse", "--absolute-git-dir"], cancel)?.trim()
            ))),
            Ok(_) => Ok(ok(format!(
                "是 Git 工作区；仓库根：{}",
                git_run(&directory, &["rev-parse", "--show-toplevel"], cancel)?.trim()
            ))),
            Err(e) if e.contains("not a git repository") => Ok(ok("不是 Git 仓库".into())),
            Err(e) => Err(format!("无法判断仓库状态：{e}")),
        };
    }
    git_run(&directory, &["rev-parse", "--show-toplevel"], cancel)?;
    let actual_directory = directory.canonicalize().map_err(|e| e.to_string())?;
    // Preserve native path bytes: Git stdout is display text, never a path restoration source.
    let root = actual_directory
        .ancestors()
        .find(|p| p.join(".git").exists())
        .ok_or("无法定位经过 Git 确认的工作区根目录")?
        .to_path_buf();
    match operation {
        "CAP-DEV-002" => {
            let remote = git_name(value(p, "remote", "origin"), "远端")?;
            let branch = value(p, "branch", "");
            if !branch.is_empty() {
                git_name(branch, "分支")?;
            }
            let strategy = choice(p, "strategy", "ff-only", &["ff-only", "merge", "rebase"])?;
            let flag = match strategy {
                "merge" => "--no-rebase",
                "rebase" => "--rebase",
                _ => "--ff-only",
            };
            let mut args = vec!["pull", flag, "--", remote];
            if !branch.is_empty() {
                args.push(branch);
            }
            let before = git_run(&directory, &["rev-parse", "HEAD"], cancel)?;
            match git_run(&directory, &args, cancel) {
                Ok(output) => Ok(ok(format!(
                    "拉取完成（{strategy}）\n原提交：{}\n当前提交：{}\n{output}",
                    before.trim(),
                    git_run(&directory, &["rev-parse", "HEAD"], cancel)?.trim()
                ))),
                Err(e) => Err(format!(
                    "Git 拉取失败；已获取的远端对象或冲突工作区会保留，未自动重置：{e}"
                )),
            }
        }
        "CAP-DEV-003" => {
            let branch = git_name(value(p, "branch", ""), "分支")?;
            git_run(
                &directory,
                &["check-ref-format", "--branch", branch],
                cancel,
            )?;
            let args = if boolean(p, "create", false)? {
                vec!["switch", "-c", branch]
            } else {
                vec!["switch", "--", branch]
            };
            let output = git_run(&directory, &args, cancel)?;
            let current = git_run(&directory, &["branch", "--show-current"], cancel)?;
            Ok(ok(format!(
                "切换完成，当前分支：{}\n{output}",
                current.trim()
            )))
        }
        "CAP-DEV-004" => {
            let scope = choice(p, "scope", "selected", &["selected", "all", "staged"])?;
            let message = value(p, "message", "");
            if message.trim().is_empty() || message.contains('\0') {
                return Err("请提供有效提交消息".into());
            }
            let remote = git_name(value(p, "remote", "origin"), "远端")?;
            let branch = value(p, "branch", "");
            let branch = if branch.is_empty() {
                git_run(&directory, &["branch", "--show-current"], cancel)?
                    .trim()
                    .to_owned()
            } else {
                branch.to_owned()
            };
            git_name(&branch, "推送分支")?;
            git_run(
                &directory,
                &["check-ref-format", "--branch", &branch],
                cancel,
            )?;
            let mut stages = vec![];
            if scope == "selected" {
                require_sources(sources)?;
                let root = root.canonicalize().map_err(|e| e.to_string())?;
                let mut cmd = git_command(&root)?;
                cmd.args(["add", "--"]);
                for path in sources {
                    let source = path.canonicalize().map_err(|e| e.to_string())?;
                    let relative = source
                        .strip_prefix(&root)
                        .map_err(|_| "暂存选区必须位于仓库内")?;
                    cmd.arg(relative);
                }
                run_command(cmd, cancel)?;
                stages.push(format!(
                    "阶段 1：已暂存 {} 项选区（已有暂存内容也会包含在提交中）",
                    sources.len()
                ));
            } else if scope == "all" {
                git_run(&root, &["add", "--all", "--", "."], cancel)?;
                stages.push("阶段 1：已暂存整个仓库新增、修改和删除".into());
            } else {
                stages.push("阶段 1：使用现有暂存区，未额外暂存".into());
            }
            match git_run(&root, &["commit", "-m", message], cancel) {
                Ok(s) => stages.push(format!("阶段 2：提交成功\n{s}")),
                Err(e) => {
                    return Err(format!(
                        "{}\n阶段 2：提交失败：{e}；已暂存状态保留",
                        stages.join("\n")
                    ));
                }
            }
            match git_run(
                &root,
                &["push", "--", remote, &format!("HEAD:refs/heads/{branch}")],
                cancel,
            ) {
                Ok(s) => {
                    stages.push(format!("阶段 3：推送成功至 {remote}/{branch}\n{s}"));
                    Ok(ok(stages.join("\n")))
                }
                Err(e) => Err(format!(
                    "{}\n阶段 3：推送失败：{e}；本地提交已保留，未回滚或强推",
                    stages.join("\n")
                )),
            }
        }
        _ => Err("未登记的 Git 操作".into()),
    }
}
fn brew_command() -> Result<Command, String> {
    let mut cmd = Command::new(tool("brew")?);
    cmd.env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_INSTALL_CLEANUP", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1");
    Ok(cmd)
}
fn brew_run(args: &[&str], cancel: &AtomicBool) -> Result<String, String> {
    let mut cmd = brew_command()?;
    cmd.args(args);
    run_command(cmd, cancel)
}
fn package_name(p: &Params) -> Result<String, String> {
    let name = value(p, "package", "");
    let version = value(p, "version", "");
    let valid = |s: &str| {
        !s.is_empty()
            && !s.starts_with('-')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._+-@/".contains(c))
    };
    if !valid(name)
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err("请输入有效 Homebrew 包名或 tap/formula，不接受选项或 URL".into());
    }
    if !version.is_empty()
        && (!version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
            || name.contains('@'))
    {
        return Err("版本必须是有效目录版本后缀，且不能与包名中的 @ 重复".into());
    }
    Ok(if version.is_empty() {
        name.to_owned()
    } else {
        format!("{name}@{version}")
    })
}
fn brew(operation: &str, p: &Params, cancel: &AtomicBool) -> Result<NativeOutput, String> {
    choice(p, "manager", "homebrew", &["homebrew"])?;
    if operation == "CAP-TOOLS-001" {
        return match tool("brew"){Ok(path)=>{let version=brew_run(&["--version"],cancel)?;Ok(ok(format!("已检测到 Homebrew\n路径：{}\n识别来源：固定系统/Homebrew 工具目录\n{version}",path.display())))},Err(_)=>Ok(ok("未在 /opt/homebrew/bin、/usr/local/bin、/usr/bin 或 /bin 检测到 Homebrew；未执行安装".into()))};
    }
    if operation == "CAP-TOOLS-002" {
        let scope = choice(
            p,
            "scope",
            "all",
            &["all", "requested", "dependencies", "leaves"],
        )?;
        let json = brew_run(&["info", "--json=v2", "--installed"], cancel)?;
        let data: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("Homebrew 清单解析失败：{e}"))?;
        let leaves = if scope == "leaves" {
            brew_run(&["leaves"], cancel)?
                .lines()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        let mut lines = vec![format!(
            "Homebrew 已安装清单；范围={scope}（all 包含 formula/cask，其余范围仅 formula；requested=曾显式请求；dependencies=仅作为依赖安装；leaves=当前无其他 formula 依赖）"
        )];
        for formula in data["formulae"]
            .as_array()
            .ok_or("Homebrew formulae 字段缺失")?
        {
            let name = formula["full_name"]
                .as_str()
                .or_else(|| formula["name"].as_str())
                .unwrap_or("未知名称");
            let installed = formula["installed"]
                .as_array()
                .ok_or("Homebrew installed 字段缺失")?;
            for install in installed {
                let requested = install["installed_on_request"].as_bool();
                let dependency = install["installed_as_dependency"].as_bool();
                let include = match scope {
                    "requested" => requested == Some(true),
                    "dependencies" => dependency == Some(true) && requested != Some(true),
                    "leaves" => {
                        leaves.contains(name)
                            || formula["name"].as_str().is_some_and(|n| leaves.contains(n))
                    }
                    _ => true,
                };
                if include {
                    lines.push(format!("formula {name}\t版本={}\t来源={}\t显式请求={requested:?} 依赖={dependency:?}",install["version"].as_str().unwrap_or("未知"),formula["tap"].as_str().unwrap_or("未记录")));
                }
            }
        }
        if scope == "all" {
            for cask in data["casks"].as_array().ok_or("Homebrew casks 字段缺失")? {
                lines.push(format!(
                    "cask {}\t已装版本={}\t来源={}",
                    cask["token"].as_str().unwrap_or("未知"),
                    cask["installed"],
                    cask["tap"].as_str().unwrap_or("未记录")
                ));
            }
        }
        return Ok(ok(lines.join("\n")));
    }
    let name = package_name(p)?;
    let source = choice(p, "source", "formula", &["formula", "cask"])?;
    let kind = if source == "formula" {
        "--formula"
    } else {
        "--cask"
    };
    if source == "cask" && !value(p, "version", "").is_empty() {
        return Err("cask 不支持按 formula 的 @版本目录安装；请提供完整 cask 名称".into());
    }
    let before = brew_run(&["list", kind, "--versions"], cancel)?;
    let installed = |list: &str| {
        list.lines().any(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|n| n == name || n == name.rsplit('/').next().unwrap_or(&name))
        })
    };
    match operation {
        "CAP-TOOLS-003" => {
            if installed(&before) {
                return Ok(ok(format!("{name} 已安装；未重复安装\n{before}")));
            }
            let output = brew_run(&["install", kind, "--", &name], cancel).map_err(|e| {
                format!("安装失败：{e}；Homebrew 可能已完成下载或安装依赖，未自动撤销")
            })?;
            let after = brew_run(&["list", kind, "--versions"], cancel)?;
            if !installed(&after) {
                return Err(format!("安装命令结束但未检测到目标包 {name}\n{output}"));
            }
            Ok(ok(format!(
                "已通过 Homebrew {source} 安装 {name}；归档完整性由 Homebrew formula/cask 校验\n{output}\n实际安装清单：\n{after}"
            )))
        }
        "CAP-TOOLS-004" => {
            if !installed(&before) {
                return Err(format!(
                    "Homebrew {source} 未管理已安装包 {name}，未执行卸载"
                ));
            }
            let output = brew_run(&["uninstall", kind, "--", &name], cancel)?;
            let after = brew_run(&["list", kind, "--versions"], cancel)?;
            if installed(&after) {
                return Err(format!("卸载命令结束但目标包仍存在：{name}\n{after}"));
            }
            Ok(ok(format!(
                "已卸载指定 {source} {name}；未请求 autoremove、zap 或删除共享依赖/用户数据\n{output}\n剩余包：\n{after}"
            )))
        }
        _ => Err("未登记的 Homebrew 操作".into()),
    }
}
fn zip_ratio(sources: &[PathBuf], p: &Params, cancel: &AtomicBool) -> Result<NativeOutput, String> {
    require_sources(sources)?;
    let precision = integer(p, "precision", "2", 0, 8)?;
    let scope = choice(p, "scope", "total", &["total", "entries"])?;
    let mut report = Report::default();
    for source in sources {
        check(cancel)?;
        let result = (|| {
            let file = fs::File::open(source).map_err(|e| e.to_string())?;
            let archive_bytes = file.metadata().map_err(|e| e.to_string())?.len();
            let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            let mut original = 0u64;
            let mut compressed = 0u64;
            let mut lines = vec![];
            let format = |o: u64, c: u64| {
                if o == 0 {
                    format!("原大小=0 bytes；压缩数据={c} bytes；压缩率不可计算（分母为 0）")
                } else {
                    format!(
                        "原大小={o} bytes；压缩数据={c} bytes；压缩后/原大小={:.precision$}%；节省比例=(1−压缩后/原大小)×100={:.precision$}%",
                        c as f64 / o as f64 * 100.0,
                        (1.0 - c as f64 / o as f64) * 100.0
                    )
                }
            };
            for i in 0..archive.len() {
                check(cancel)?;
                let entry = archive.by_index(i).map_err(|e| e.to_string())?;
                original = original.checked_add(entry.size()).ok_or("归档大小溢出")?;
                compressed = compressed
                    .checked_add(entry.compressed_size())
                    .ok_or("归档大小溢出")?;
                if scope == "entries" {
                    lines.push(format!(
                        "{}：{}",
                        entry.name(),
                        format(entry.size(), entry.compressed_size())
                    ));
                }
            }
            lines.push(format!(
                "条目数据总计：{}；ZIP 文件实际大小={archive_bytes} bytes（另含目录/头部）",
                format(original, compressed)
            ));
            Ok::<_, String>(lines.join("\n"))
        })();
        match result {
            Ok(s) => report.lines.push(format!("{}\n{s}", source.display())),
            Err(e) => report.fail(source, e),
        }
    }
    Ok(report.finish())
}
fn canonical_target(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path.canonicalize().map_err(|e| e.to_string());
    }
    let parent = path.parent().ok_or("目标缺少父目录")?;
    let name = path.file_name().ok_or("目标缺少名称")?;
    if name == ".." || name == "." {
        return Err("目标目录无效".into());
    }
    Ok(canonical_target(parent)?.join(name))
}
fn validate_transfer(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("不移动/复制符号链接；请明确选择原始文件".into());
    }
    let origin = source.canonicalize().map_err(|e| e.to_string())?;
    let destination = canonical_target(target)?;
    if metadata.is_dir() && destination.starts_with(&origin) {
        return Err("目标不能位于源目录内部".into());
    }
    Ok(())
}
fn copy_entry(source: &Path, target: &Path, cancel: &AtomicBool) -> Result<(), String> {
    check(cancel)?;
    let metadata = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("不跟随符号链接：{}", source.display()));
    }
    if metadata.is_dir() {
        fs::create_dir(target).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_entry(&entry.path(), &target.join(entry.file_name()), cancel)?;
        }
    } else if metadata.is_file() {
        let mut input = fs::File::open(source).map_err(|e| e.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        let mut buf = [0u8; 65536];
        loop {
            check(cancel)?;
            let n = input.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        output.sync_all().map_err(|e| e.to_string())?;
        fs::set_permissions(target, metadata.permissions()).map_err(|e| e.to_string())?;
    } else {
        return Err("仅支持普通文件和目录".into());
    }
    Ok(())
}
fn transfer(
    source: &Path,
    directory: &Path,
    move_source: bool,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    validate_transfer(source, directory)?;
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let target = unique_destination(
        directory,
        Path::new(source.file_name().ok_or("源缺少名称")?),
    );
    if move_source {
        match fs::rename(source, &target) {
            Ok(()) => return Ok(target),
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    copy_entry(source, &target, cancel).map_err(|e| {
        format!(
            "复制未完成：{e}；源保留，可能已生成部分目标 {}",
            target.display()
        )
    })?;
    if move_source {
        let result = if source.is_dir() {
            fs::remove_dir_all(source)
        } else {
            fs::remove_file(source)
        };
        result.map_err(|e| {
            format!(
                "已复制到 {}，源删除失败：{e}；两份数据均保留",
                target.display()
            )
        })?;
    }
    Ok(target)
}
fn write_archive(sources: &[PathBuf], target: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| e.to_string())?;
    let mut writer = zip::ZipWriter::new(file);
    for source in sources {
        append_archive(
            &mut writer,
            source,
            Path::new(source.file_name().ok_or("源缺少名称")?),
            cancel,
        )?;
    }
    writer
        .finish()
        .map_err(|e| e.to_string())?
        .sync_all()
        .map_err(|e| e.to_string())
}
fn append_archive(
    writer: &mut zip::ZipWriter<fs::File>,
    source: &Path,
    relative: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    check(cancel)?;
    let meta = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return Err(format!("归档不跟随符号链接：{}", source.display()));
    }
    let options: zip::write::SimpleFileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let name = relative
        .to_str()
        .ok_or("ZIP 文件名需要有效 UTF-8，原文件保留")?;
    if meta.is_dir() {
        writer
            .add_directory(name, options)
            .map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            append_archive(
                writer,
                &entry.path(),
                &relative.join(entry.file_name()),
                cancel,
            )?;
        }
    } else if meta.is_file() {
        writer
            .start_file(name, options)
            .map_err(|e| e.to_string())?;
        let mut file = fs::File::open(source).map_err(|e| e.to_string())?;
        let mut buf = [0u8; 65536];
        loop {
            check(cancel)?;
            let n = file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
    } else {
        return Err("归档仅支持普通文件和目录".into());
    }
    Ok(())
}
fn transfer_archive(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    require_sources(sources)?;
    let destination = value(p, "destination", "");
    if destination.trim().is_empty() {
        return Err("请选择目标目录".into());
    }
    let directory = cwd.join(destination);
    let name = safe_name(value(p, "name", "archive.zip"))?;
    if !name.to_ascii_lowercase().ends_with(".zip") {
        return Err("归档名称必须以 .zip 结尾".into());
    }
    let move_source = choice(p, "sourceIntent", "copy", &["copy", "move"])? == "move";
    for source in sources {
        validate_transfer(source, &directory)?;
    }
    let mut report = Report::default();
    let mut transferred = vec![];
    for source in sources {
        if let Err(e) = check(cancel) {
            return Err(format!(
                "{}\n取消：{e}；已完成的第一阶段文件保留",
                report.finish().output
            ));
        }
        match transfer(source, &directory, move_source, cancel) {
            Ok(target) => {
                report.lines.push(format!(
                    "阶段 1：{} {} → {}",
                    if move_source {
                        "已移动"
                    } else {
                        "已复制"
                    },
                    source.display(),
                    target.display()
                ));
                transferred.push(target);
            }
            Err(e) => report.fail(source, e),
        }
    }
    if transferred.is_empty() {
        return Err(format!(
            "阶段 1 没有完整传输的文件，未压缩\n{}",
            report.finish().output
        ));
    }
    let target = unique_destination(&directory, Path::new(name));
    match write_archive(&transferred, &target, cancel) {
        Ok(()) => report.lines.push(format!(
            "阶段 2：ZIP 已生成 {}；实际包含 {} 项已传输输入",
            target.display(),
            transferred.len()
        )),
        Err(e) => {
            let cleanup = fs::remove_file(&target)
                .err()
                .map(|e| format!("；未完成归档清理失败：{e}"))
                .unwrap_or_default();
            return Err(format!(
                "{}\n阶段 2：压缩失败：{e}{cleanup}；第一阶段已移动/复制的文件仍在 {}，未回滚或删除唯一源文件",
                report.finish().output,
                directory.display()
            ));
        }
    }
    Ok(report.finish())
}
fn offset(value: &str) -> Result<time::UtcOffset, String> {
    if value == "UTC" || value == "Z" {
        return Ok(time::UtcOffset::UTC);
    }
    let bytes = value.as_bytes();
    if bytes.len() != 6 || !matches!(bytes[0], b'+' | b'-') || bytes[3] != b':' {
        return Err("时区使用 UTC 或 ±HH:MM".into());
    }
    let hour = value[1..3].parse::<i8>().map_err(|_| "时区小时无效")?;
    let minute = value[4..6].parse::<i8>().map_err(|_| "时区分钟无效")?;
    if hour > 23 || minute > 59 {
        return Err("时区范围无效".into());
    }
    let sign = if bytes[0] == b'-' { -1 } else { 1 };
    time::UtcOffset::from_hms(hour * sign, minute * sign, 0).map_err(|e| e.to_string())
}
fn taken_date(
    source: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<time::OffsetDateTime, String> {
    let mut cmd = Command::new(tool("exiftool")?);
    cmd.args(["-json", "-DateTimeOriginal", "-OffsetTimeOriginal"])
        .arg(source);
    let text = run_command(cmd, cancel)?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let row = json.get(0).ok_or("没有 EXIF 日期记录")?;
    let date = row["DateTimeOriginal"]
        .as_str()
        .ok_or("缺少拍摄日期 DateTimeOriginal；未改用文件时间")?;
    let parts = date.split([':', ' ']).collect::<Vec<_>>();
    if parts.len() < 6 {
        return Err("拍摄日期格式无法识别".into());
    }
    let year = parts[0].parse::<i32>().map_err(|_| "拍摄年份无效")?;
    let parse = |i: usize| {
        parts[i]
            .parse::<u8>()
            .map_err(|_| "拍摄日期无效".to_owned())
    };
    let date = time::Date::from_calendar_date(
        year,
        time::Month::try_from(parse(1)?).map_err(|e| e.to_string())?,
        parse(2)?,
    )
    .map_err(|e| e.to_string())?;
    let time = time::Time::from_hms(parse(3)?, parse(4)?, parse(5)?).map_err(|e| e.to_string())?;
    let tz = row["OffsetTimeOriginal"]
        .as_str()
        .unwrap_or(value(p, "takenOffset", ""));
    if tz.is_empty() {
        return Err("拍摄日期缺少时区，请填写拍摄时间原始时区 takenOffset；未猜测本地时区".into());
    }
    Ok(date.with_time(time).assume_offset(offset(tz)?))
}
fn organize(
    sources: &[PathBuf],
    cwd: &Path,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    let group = choice(p, "groupBy", "extension", &["extension", "date", "rules"])?;
    let scope = choice(p, "scope", "selection", &["selection", "directory"])?;
    let owned = if scope == "directory" {
        fs::read_dir(cwd.join(value(p, "directory", ".")))
            .map_err(|e| e.to_string())?
            .map(|entry| entry.map(|e| e.path()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        require_sources(sources)?;
        sources.to_vec()
    };
    let date_source = choice(
        p,
        "dateSource",
        "modified",
        &["created", "modified", "taken"],
    )?;
    let tz = offset(value(p, "timezone", "UTC"))?;
    let format = choice(
        p,
        "directoryFormat",
        "YYYY-MM",
        &["YYYY", "YYYY-MM", "YYYY-MM-DD"],
    )?;
    let rules: BTreeMap<String, String> = if group == "rules" {
        serde_json::from_str(value(p, "rules", "{}"))
            .map_err(|e| format!("分类规则需要扩展名到目录名称的 JSON 对象：{e}"))?
    } else {
        BTreeMap::new()
    };
    for name in rules.values() {
        safe_name(name)?;
    }
    let mut report = Report::default();
    let target_root = cwd.join(value(p, "destination", "."));
    let mut plan = vec![];
    for source in owned {
        check(cancel)?;
        if source
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            && !boolean(p, "includeHidden", false)?
        {
            continue;
        }
        let category = (|| {
            if fs::symlink_metadata(&source)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("符号链接不在整理范围内".into());
            }
            let extension = source
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("无扩展名")
                .to_lowercase();
            match group {
                "date" => {
                    let date = match date_source {
                        "taken" => taken_date(&source, p, cancel)?,
                        "created" => fs::metadata(&source)
                            .and_then(|m| m.created())
                            .map(time::OffsetDateTime::from)
                            .map_err(|e| format!("创建日期不可用：{e}；未改用修改日期"))?,
                        _ => fs::metadata(&source)
                            .and_then(|m| m.modified())
                            .map(time::OffsetDateTime::from)
                            .map_err(|e| e.to_string())?,
                    }
                    .to_offset(tz);
                    Ok(match format {
                        "YYYY" => format!("{:04}", date.year()),
                        "YYYY-MM-DD" => format!(
                            "{:04}-{:02}-{:02}",
                            date.year(),
                            u8::from(date.month()),
                            date.day()
                        ),
                        _ => format!("{:04}-{:02}", date.year(), u8::from(date.month())),
                    })
                }
                "rules" => rules
                    .get(&extension)
                    .cloned()
                    .ok_or_else(|| format!("扩展名 {extension} 没有分类规则，保留原位置")),
                _ => Ok(extension),
            }
        })();
        match category {
            Ok(category) => {
                safe_name(&category)?;
                let directory = target_root.join(category);
                match validate_transfer(&source, &directory) {
                    Ok(()) => {
                        report.lines.push(format!(
                            "整理计划：{} → {}（重名使用唯一名称）",
                            source.display(),
                            directory.display()
                        ));
                        plan.push((source, directory));
                    }
                    Err(e) => report.fail(&source, e),
                }
            }
            Err(e) => report.fail(&source, e),
        }
    }
    for (source, directory) in plan {
        if let Err(e) = check(cancel) {
            return Err(format!("{}\n{e}；已移动项保留", report.finish().output));
        }
        match transfer(&source, &directory, true, cancel) {
            Ok(target) => report.lines.push(format!(
                "已移动：{} → {}",
                source.display(),
                target.display()
            )),
            Err(e) => report.fail(&source, e),
        }
    }
    Ok(report.finish())
}
fn decimal(value: &str) -> Result<(i128, u32), String> {
    let value = value.trim();
    let (negative, s) = if let Some(v) = value.strip_prefix('-') {
        (true, v)
    } else {
        (false, value.strip_prefix('+').unwrap_or(value))
    };
    let parts = s.split('.').collect::<Vec<_>>();
    if parts.len() > 2
        || parts.iter().any(|p| !p.chars().all(|c| c.is_ascii_digit()))
        || s.is_empty()
        || s == "."
    {
        return Err("百分比参数需要十进制数（不使用指数表示）".into());
    }
    let decimals = parts.get(1).map_or(0, |s| s.len());
    if decimals > 12 {
        return Err("百分比输入最多保留 12 位小数".into());
    }
    let digits = parts
        .concat()
        .parse::<i128>()
        .map_err(|_| "数值超出计算范围")?;
    Ok((if negative { -digits } else { digits }, decimals as u32))
}
fn percent(p: &Params, precision: usize) -> Result<String, String> {
    let (a, sa) = decimal(value(p, "percent", ""))?;
    let (b, sb) = decimal(value(p, "base", ""))?;
    let scale = 10i128.checked_pow(precision as u32).ok_or("精度超出范围")?;
    let numerator = a
        .checked_mul(b)
        .and_then(|n| n.checked_mul(scale))
        .ok_or("数值超出高精度计算范围")?;
    let denominator = 10i128.checked_pow(sa + sb + 2).ok_or("数值精度超出范围")?;
    let q = numerator / denominator;
    let rem = numerator % denominator;
    let mode = choice(
        p,
        "rounding",
        "nearest",
        &["nearest", "floor", "ceil", "truncate"],
    )?;
    let adjust = match mode {
        "floor" if rem < 0 => -1,
        "ceil" if rem > 0 => 1,
        "nearest" if rem.abs() >= denominator / 2 => numerator.signum(),
        _ => 0,
    };
    let rounded = q.checked_add(adjust).ok_or("取整超出范围")?;
    let sign = if rounded < 0 { "-" } else { "" };
    let abs = rounded.checked_abs().ok_or("数值超出范围")?;
    let result = if precision == 0 {
        format!("{sign}{abs}")
    } else {
        format!("{sign}{}.{:0precision$}", abs / scale, abs % scale)
    };
    Ok(format!(
        "{} × {} ÷ 100 = {result}；小数位={precision}；取整={mode}",
        value(p, "base", ""),
        value(p, "percent", "")
    ))
}
fn calculate(operation: &str, p: &Params) -> Result<String, String> {
    let precision = integer(p, "precision", "4", 0, 12)?;
    if operation == "CAP-CALC-001" {
        return percent(p, precision);
    }
    let (result, meaning) = match operation {
        "CAP-CALC-002" => {
            let feet = number(p, "feet", "")?;
            let inches = number(p, "inches", "")?;
            if feet < 0.0 || feet.fract() != 0.0 || !(0.0..12.0).contains(&inches) {
                return Err("英尺必须为非负整数，英寸范围为 0（含）至 12（不含）".into());
            }
            let unit = choice(p, "unit", "cm", &["cm", "m", "mm"])?;
            let multiplier = match unit {
                "m" => 0.01,
                "mm" => 10.0,
                _ => 1.0,
            };
            (
                (feet * 30.48 + inches * 2.54) * multiplier,
                format!("{feet} ft + {inches} in，按 1 in = 2.54 cm 换算为 {unit}"),
            )
        }
        "CAP-CALC-003" => {
            let amount = number(p, "amount", "")?;
            if amount < 0.0 {
                return Err("固定时长不能为负数".into());
            }
            let factor = |unit: &str| match unit {
                "seconds" => Ok(1.0),
                "minutes" => Ok(60.0),
                "hours" => Ok(3600.0),
                "days" => Ok(86400.0),
                "weeks" => Ok(604800.0),
                _ => Err("仅支持 seconds/minutes/hours/days/weeks 固定时长单位"),
            };
            let from = value(p, "from", "days");
            let to = value(p, "to", "seconds");
            (
                amount * factor(from)? / factor(to)?,
                format!("{amount} {from} → {to}；1 day = 86400 seconds，不用于日历日期/DST"),
            )
        }
        "CAP-CALC-004" => {
            let input = number(p, "value", "")?;
            let domain = choice(p, "domain", "real", &["real", "complex"])?;
            if input < 0.0 {
                if domain == "real" {
                    return Err("负数没有实数平方根；可选择 complex 范围".into());
                }
                return Ok(format!(
                    "√({input}) = {:.precision$}i（主平方根）；另一平方根为 −{:.precision$}i",
                    (-input).sqrt(),
                    (-input).sqrt()
                ));
            }
            (input.sqrt(), format!("√({input})，主平方根"))
        }
        _ => return Err("未登记的计算操作".into()),
    };
    if !result.is_finite() {
        return Err("计算结果超出有限数值范围".into());
    }
    Ok(format!("{meaning} = {result:.precision$}"))
}

fn name_parts<'a>(name: &'a str, is_dir: bool, rule: &str) -> (&'a str, &'a str) {
    if is_dir || rule == "none" {
        return (name, "");
    }
    let split = if rule == "all" {
        name.char_indices()
            .find(|(i, c)| *i > 0 && *c == '.')
            .map(|(i, _)| i)
    } else {
        name.rfind('.').filter(|i| *i > 0)
    };
    split.map_or((name, ""), |i| (&name[..i], &name[i..]))
}
fn file_date(
    source: &Path,
    field: &str,
    p: &Params,
    cancel: &AtomicBool,
) -> Result<time::OffsetDateTime, String> {
    match field {
        "now" => Ok(time::OffsetDateTime::now_utc()),
        "taken" => taken_date(source, p, cancel),
        "created" => fs::metadata(source)
            .and_then(|m| m.created())
            .map(time::OffsetDateTime::from)
            .map_err(|e| format!("创建日期不可用：{e}；未使用其他日期")),
        "modified" => fs::metadata(source)
            .and_then(|m| m.modified())
            .map(time::OffsetDateTime::from)
            .map_err(|e| e.to_string()),
        _ => Err("日期字段无效".into()),
    }
}
fn unique_case_name(
    parent: &Path,
    desired: &str,
    existing: &BTreeSet<String>,
    reserved: &BTreeSet<String>,
) -> String {
    let (stem, extension) = name_parts(desired, false, "last");
    let mut candidate = desired.to_owned();
    let mut counter = 1u64;
    while existing.contains(&candidate.to_lowercase())
        || reserved.contains(&candidate.to_lowercase())
        || fs::symlink_metadata(parent.join(&candidate)).is_ok()
    {
        candidate = format!("{stem} ({counter}){extension}");
        counter += 1;
    }
    candidate
}
fn rename(
    operation: &str,
    sources: &[PathBuf],
    p: &Params,
    cancel: &AtomicBool,
) -> Result<NativeOutput, String> {
    require_sources(sources)?;
    let extension_rule = choice(p, "extensionRule", "last", &["last", "all", "none"])?;
    let mode = choice(
        p,
        "mode",
        "template",
        &["template", "affix", "date", "case"],
    )?;
    let case_scope = choice(p, "caseScope", "stem", &["stem", "name", "extension"])?;
    let letter_case = choice(p, "letterCase", "lower", &["lower", "upper"])?;
    let insertion = choice(
        p,
        "position",
        if operation == "CAP-FILE-006" {
            "prefix"
        } else {
            "beforeExtension"
        },
        if operation == "CAP-FILE-006" {
            &["prefix", "suffix"]
        } else {
            &["beforeExtension", "afterName"]
        },
    )?;
    let width = integer(p, "width", "3", 0, 16)?;
    let start = integer(p, "start", "1", 0, 1_000_000_000)?;
    let step = integer(p, "step", "1", 1, 1_000_000_000)?;
    let date_source = choice(
        p,
        "dateSource",
        "modified",
        &["created", "modified", "taken", "now"],
    )?;
    let date_format = choice(
        p,
        "dateFormat",
        "YYYY-MM-DD",
        &["YYYY-MM-DD", "YYYYMMDD", "YYYY-MM-DD_HHmmss"],
    )?;
    let timezone = offset(value(p, "timezone", "UTC"))?;
    let sort = choice(
        p,
        "sort",
        "nameAsc",
        &[
            "nameAsc",
            "nameDesc",
            "modifiedAsc",
            "modifiedDesc",
            "selection",
        ],
    )?;
    let mut report = Report::default();
    let mut paths = Vec::new();
    for source in sources {
        check(cancel)?;
        let metadata = match fs::symlink_metadata(source) {
            Ok(m) => m,
            Err(e) => {
                report.fail(source, e);
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            report.fail(source, "符号链接不在改名范围内");
            continue;
        }
        let modified = if operation == "CAP-FILE-006" && sort.starts_with("modified") {
            match metadata.modified() {
                Ok(t) => Some(t),
                Err(e) => {
                    report.fail(source, e);
                    continue;
                }
            }
        } else {
            None
        };
        paths.push((source, metadata.is_dir(), modified));
    }
    if operation == "CAP-FILE-006" {
        match sort {
            "nameAsc" => {
                paths.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()).then(a.0.cmp(b.0)))
            }
            "nameDesc" => {
                paths.sort_by(|a, b| b.0.file_name().cmp(&a.0.file_name()).then(a.0.cmp(b.0)))
            }
            "modifiedAsc" => paths.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(b.0))),
            "modifiedDesc" => paths.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0))),
            _ => {}
        }
    }
    let mut existing: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut reserved: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut planned = Vec::new();
    for (index, (source, is_dir, _)) in paths.iter().enumerate() {
        check(cancel)?;
        let result = (|| {
            let name = source
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("改名模板需要有效 UTF-8 名称；源保留")?;
            let (stem, extension) = name_parts(name, *is_dir, extension_rule);
            let desired = if operation == "CAP-FILE-006" {
                let sequence = start
                    .checked_add(step.checked_mul(index).ok_or("序号溢出")?)
                    .ok_or("序号溢出")?;
                let numbered = format!("{sequence:0width$}");
                if insertion == "prefix" {
                    format!("{numbered}-{stem}{extension}")
                } else {
                    format!("{stem}-{numbered}{extension}")
                }
            } else {
                match mode {
                    "affix" => {
                        let prefix = value(p, "prefix", "");
                        let suffix = value(p, "suffix", "");
                        if insertion == "afterName" {
                            format!("{prefix}{name}{suffix}")
                        } else {
                            format!("{prefix}{stem}{suffix}{extension}")
                        }
                    }
                    "date" => {
                        let date = file_date(source, date_source, p, cancel)?.to_offset(timezone);
                        let date_text = match date_format {
                            "YYYYMMDD" => format!(
                                "{:04}{:02}{:02}",
                                date.year(),
                                u8::from(date.month()),
                                date.day()
                            ),
                            "YYYY-MM-DD_HHmmss" => format!(
                                "{:04}-{:02}-{:02}_{:02}{:02}{:02}",
                                date.year(),
                                u8::from(date.month()),
                                date.day(),
                                date.hour(),
                                date.minute(),
                                date.second()
                            ),
                            _ => format!(
                                "{:04}-{:02}-{:02}",
                                date.year(),
                                u8::from(date.month()),
                                date.day()
                            ),
                        };
                        if insertion == "afterName" {
                            format!("{name}-{date_text}")
                        } else {
                            format!("{stem}-{date_text}{extension}")
                        }
                    }
                    "case" => {
                        let convert = |s: &str| {
                            if letter_case == "lower" {
                                s.to_lowercase()
                            } else {
                                s.to_uppercase()
                            }
                        };
                        match case_scope {
                            "name" => convert(name),
                            "extension" => format!("{stem}{}", convert(extension)),
                            _ => format!("{}{extension}", convert(stem)),
                        }
                    }
                    _ => value(p, "template", "{name}")
                        .replace("{name}", name)
                        .replace("{stem}", stem)
                        .replace("{ext}", extension.trim_start_matches('.')),
                }
            };
            safe_name(&desired)?;
            let parent = source.parent().ok_or("输入缺少父目录")?.to_path_buf();
            if !existing.contains_key(&parent) {
                let names = fs::read_dir(&parent)
                    .map_err(|e| e.to_string())?
                    .map(|entry| {
                        entry
                            .map(|e| e.file_name().to_string_lossy().to_lowercase())
                            .map_err(|e| e.to_string())
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?;
                existing.insert(parent.clone(), names);
            }
            let current = existing.get(&parent).ok_or("父目录状态不存在")?;
            let used = reserved.entry(parent.clone()).or_default();
            let lower = desired.to_lowercase();
            let same_name = lower == name.to_lowercase();
            let collision = (!same_name && current.contains(&lower)) || used.contains(&lower);
            let desired = if collision {
                if mode == "case" && operation == "CAP-FILE-005" {
                    return Err(format!("大小写名称冲突：{desired}；原文件保留，未覆盖"));
                }
                unique_case_name(&parent, &desired, current, used)
            } else {
                desired
            };
            used.insert(desired.to_lowercase());
            Ok::<_, String>((parent.join(desired), name.to_owned()))
        })();
        match result {
            Ok((destination, _)) => {
                report.lines.push(format!(
                    "改名预览：{} → {}",
                    source.display(),
                    destination.display()
                ));
                planned.push((source.to_path_buf(), destination));
            }
            Err(e) => report.fail(source, e),
        }
    }
    // A generated case change can collide with a distinct input differing only in case on
    // a case-sensitive volume. Detect from directory entries, before touching either file.
    if operation == "CAP-FILE-005" && mode == "case" {
        let mut protected = BTreeSet::new();
        for (source, destination) in &planned {
            if let Some(parent) = source.parent() {
                for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.path() != *source
                        && entry.file_name().to_string_lossy().to_lowercase()
                            == destination
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_lowercase()
                    {
                        protected.insert(source.clone());
                    }
                }
            }
        }
        planned.retain(|(source, _)| {
            if protected.contains(source) {
                report.fail(source, "大小写名称冲突；原文件保留，未覆盖");
                false
            } else {
                true
            }
        });
    }
    for (source, destination) in planned {
        if let Err(e) = check(cancel) {
            return Err(format!("{}\n{e}；已完成改名保留", report.finish().output));
        }
        if source == destination {
            report
                .lines
                .push(format!("名称未变化：{}", source.display()));
            continue;
        }
        match fs::rename(&source, &destination) {
            Ok(()) => report.lines.push(format!(
                "已改名：{} → {}",
                source.display(),
                destination.display()
            )),
            Err(e) => report.fail(&source, e),
        }
    }
    Ok(report.finish())
}

fn compare_extension(path: &Path, mime: &str) -> &'static str {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let expected = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" | "md" => "text/plain",
        _ => return "无固定后缀映射；以上类型仅由内容探测得出",
    };
    if mime == expected {
        "内容探测与常见后缀类型一致"
    } else {
        "内容探测与后缀预期不匹配；不据后缀改写实际类型"
    }
}
