//! 文件、开发者、工具和计算能力规格；条件依赖由说明明确，执行器重复校验参数。
use crate::capability_service::{CapabilityDescriptor, CapabilityField, field};
fn select(key: &str, label: &str, default: &str, choices: &[&str]) -> CapabilityField {
    field(key, label, "select", default, choices, true)
}
fn text(key: &str, label: &str, default: &str, required: bool) -> CapabilityField {
    field(key, label, "text", default, &[], required)
}
fn number(key: &str, label: &str, default: &str) -> CapabilityField {
    field(key, label, "number", default, &[], true)
}
fn toggle(key: &str, label: &str, default: bool) -> CapabilityField {
    select(
        key,
        label,
        if default { "true" } else { "false" },
        &["true", "false"],
    )
}
fn directory() -> CapabilityField {
    text(
        "directory",
        "搜索/工作目录（相对当前会话或绝对路径）",
        ".",
        true,
    )
}
fn traversal() -> Vec<CapabilityField> {
    vec![
        directory(),
        toggle("recursive", "递归子目录（不跟随符号链接）", true),
        toggle("includeHidden", "包含隐藏项", false),
    ]
}
fn unit() -> CapabilityField {
    select("unit", "大小单位", "bytes", &["bytes", "KiB", "MiB", "GiB"])
}
#[allow(clippy::too_many_arguments)] // Mirrors the fixed capability descriptor fields.
fn descriptor(
    id: &str,
    category: &str,
    title: &str,
    description: &str,
    minimum_inputs: usize,
    changes_files: bool,
    fields: Vec<CapabilityField>,
    dependencies: &[&str],
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: id.into(),
        category: category.into(),
        title: title.into(),
        description: description.into(),
        minimum_inputs,
        changes_files,
        grouped_inputs: true,
        fields,
        dependencies: dependencies.iter().map(|s| (*s).into()).collect(),
    }
}

pub fn descriptors() -> Vec<CapabilityDescriptor> {
    let mut result = vec![
        descriptor(
            "CAP-FILE-005",
            "file",
            "按模板、日期或大小写改名",
            "逐项预览和执行。普通重名使用唯一名称；大小写冲突保留双方并明确报告。日期缺失不使用其他日期代替。",
            1,
            true,
            vec![
                select(
                    "mode",
                    "改名方式",
                    "template",
                    &["template", "affix", "date", "case"],
                ),
                text(
                    "template",
                    "名称模板（{name} / {stem} / {ext}）",
                    "{name}",
                    true,
                ),
                text("prefix", "前缀原文（affix 模式）", "", false),
                text("suffix", "后缀原文（affix 模式）", "", false),
                select(
                    "position",
                    "后缀/日期插入位置",
                    "beforeExtension",
                    &["beforeExtension", "afterName"],
                ),
                select(
                    "extensionRule",
                    "扩展名规则",
                    "last",
                    &["last", "all", "none"],
                ),
                select(
                    "dateSource",
                    "日期来源",
                    "modified",
                    &["created", "modified", "taken", "now"],
                ),
                select(
                    "dateFormat",
                    "日期格式",
                    "YYYY-MM-DD",
                    &["YYYY-MM-DD", "YYYYMMDD", "YYYY-MM-DD_HHmmss"],
                ),
                text("timezone", "输出日期时区（UTC 或 ±HH:MM）", "UTC", true),
                text("takenOffset", "拍摄元数据没有时区时的原始时区", "", false),
                select("letterCase", "大小写转换", "lower", &["lower", "upper"]),
                select(
                    "caseScope",
                    "大小写转换范围",
                    "stem",
                    &["stem", "name", "extension"],
                ),
            ],
            &[],
        ),
        descriptor(
            "CAP-FILE-006",
            "file",
            "按确定顺序批量编号",
            "明确排序、起始值、步长、位数和扩展名规则；重名使用唯一名称，末行结果列出实际名称。",
            1,
            true,
            vec![
                number("start", "起始序号", "1"),
                number("step", "序号步长", "1"),
                number("width", "序号位数（0–16）", "3"),
                select("position", "序号位置", "prefix", &["prefix", "suffix"]),
                select(
                    "sort",
                    "排序规则",
                    "nameAsc",
                    &[
                        "nameAsc",
                        "nameDesc",
                        "modifiedAsc",
                        "modifiedDesc",
                        "selection",
                    ],
                ),
                select(
                    "extensionRule",
                    "扩展名规则",
                    "last",
                    &["last", "all", "none"],
                ),
            ],
            &[],
        ),
        descriptor(
            "CAP-FILE-007",
            "file",
            "按规则整理文件",
            "先列出逐项整理计划再移动；重名使用唯一名称。日期缺失时逐项说明，不使用其他日期代替；拍摄日期需要 exiftool。",
            0,
            true,
            vec![
                select(
                    "scope",
                    "整理输入范围",
                    "selection",
                    &["selection", "directory"],
                ),
                directory(),
                text("destination", "分类目录根路径", ".", true),
                select(
                    "groupBy",
                    "分类规则",
                    "extension",
                    &["extension", "date", "rules"],
                ),
                select(
                    "dateSource",
                    "日期字段",
                    "modified",
                    &["created", "modified", "taken"],
                ),
                text("timezone", "分类时区（UTC 或 ±HH:MM）", "UTC", true),
                text(
                    "takenOffset",
                    "拍摄元数据无时区时的原始时区（±HH:MM）",
                    "",
                    false,
                ),
                select(
                    "directoryFormat",
                    "日期目录格式",
                    "YYYY-MM",
                    &["YYYY", "YYYY-MM", "YYYY-MM-DD"],
                ),
                field(
                    "rules",
                    "自定义规则（扩展名到目录名称的 JSON 对象）",
                    "textarea",
                    "{}",
                    &[],
                    false,
                ),
                toggle("includeHidden", "包含隐藏项", false),
            ],
            &[],
        ),
        descriptor(
            "CAP-FILE-009",
            "file",
            "探测真实文件类型",
            "使用内容 magic 探测 MIME 并同时列出扩展名；不按文件名猜测真实类型。",
            1,
            false,
            vec![],
            &["file"],
        ),
        descriptor(
            "CAP-FILE-010",
            "file",
            "文件大小",
            "分别显示逻辑字节数和文件系统分配的占用字节数。",
            1,
            false,
            vec![unit()],
            &[],
        ),
        descriptor(
            "CAP-FILE-015",
            "file",
            "查看下载来源",
            "读取 WhereFroms 与 quarantine 系统扩展属性；未记录时明确说明，不猜测来源。",
            1,
            false,
            vec![],
            &[],
        ),
        descriptor(
            "CAP-FILE-016",
            "file",
            "检查 Finder 灰显属性",
            "读取权限、隐藏/云状态标志和扩展属性；无法确定灰显原因时明确说明。",
            1,
            false,
            vec![],
            &[],
        ),
        descriptor(
            "CAP-DEV-001",
            "developer",
            "检查 Git 仓库",
            "使用 Git 实际查询工作区根目录，区分普通目录与查询失败。",
            0,
            false,
            vec![directory()],
            &["git"],
        ),
        descriptor(
            "CAP-DEV-002",
            "developer",
            "拉取 Git 远端",
            "按指定策略拉取；冲突保留工作区状态，不强制重置。需要可用远端及认证。",
            0,
            true,
            vec![
                directory(),
                text("remote", "远端名称", "origin", true),
                text("branch", "远端分支（留空按远端默认规则）", "", false),
                select(
                    "strategy",
                    "整合策略",
                    "ff-only",
                    &["ff-only", "merge", "rebase"],
                ),
            ],
            &["git"],
        ),
        descriptor(
            "CAP-DEV-003",
            "developer",
            "切换 Git 分支",
            "仅在明确选择创建时创建新分支；冲突时保留用户改动。",
            0,
            true,
            vec![
                directory(),
                text("branch", "目标分支", "", true),
                toggle("create", "明确创建新分支", false),
            ],
            &["git"],
        ),
        descriptor(
            "CAP-DEV-004",
            "developer",
            "暂存、提交并推送",
            "显示三个阶段；提交包含暂存区已有内容。推送失败保留本地提交，不强推。需要 Git 身份、远端及认证。",
            0,
            true,
            vec![
                directory(),
                select(
                    "scope",
                    "暂存范围（selected 需要 Finder 选区）",
                    "selected",
                    &["selected", "all", "staged"],
                ),
                field("message", "提交消息", "textarea", "", &[], true),
                text("remote", "推送远端", "origin", true),
                text("branch", "推送分支（留空使用当前分支）", "", false),
            ],
            &["git"],
        ),
        descriptor(
            "CAP-DEV-006",
            "developer",
            "计算 SHA-256",
            "流式读取完整文件，输出每个文件的真实摘要。",
            1,
            false,
            vec![
                select("algorithm", "摘要算法", "sha256", &["sha256"]),
                select("format", "输出格式", "standard", &["standard", "json"]),
            ],
            &[],
        ),
        descriptor(
            "CAP-DEV-007",
            "developer",
            "移除下载隔离属性",
            "仅移除选区自身的 com.apple.quarantine，不递归目录；缺少属性时说明未修改。",
            1,
            true,
            vec![],
            &[],
        ),
        descriptor(
            "CAP-TOOLS-001",
            "tools",
            "检查 Homebrew",
            "检测路径、版本和来源；不会自动安装包管理器。",
            0,
            false,
            vec![select("manager", "包管理器", "homebrew", &["homebrew"])],
            &[],
        ),
        descriptor(
            "CAP-TOOLS-002",
            "tools",
            "查看 Homebrew 已安装清单",
            "区分全部、显式请求、依赖和叶子 formula；展示版本及 tap 来源。",
            0,
            false,
            vec![
                select("manager", "包管理器", "homebrew", &["homebrew"]),
                select(
                    "scope",
                    "清单范围",
                    "all",
                    &["all", "requested", "dependencies", "leaves"],
                ),
            ],
            &["brew"],
        ),
        descriptor(
            "CAP-ZIP-004",
            "zip",
            "计算 ZIP 压缩率",
            "比较条目原大小和压缩数据大小；同时列出归档实际大小，零长度分母单独说明。",
            1,
            false,
            vec![
                select("scope", "统计范围", "total", &["total", "entries"]),
                number("precision", "百分比小数位（0–8）", "2"),
            ],
            &[],
        ),
        descriptor(
            "CAP-ZIP-005",
            "zip",
            "整理到目标目录并归档",
            "先复制或移动，再生成 ZIP。第二阶段失败保留已整理文件；输出重名使用唯一名称，不跟随符号链接。",
            1,
            true,
            vec![
                text("destination", "目标目录", "", true),
                text("name", "ZIP 名称", "archive.zip", true),
                select("sourceIntent", "源文件处理", "copy", &["copy", "move"]),
            ],
            &[],
        ),
        descriptor(
            "CAP-CALC-001",
            "calculation",
            "百分比计算",
            "使用十进制定点运算；最多 12 位小数，明确取整方式。",
            0,
            false,
            vec![
                text("percent", "百分比（十进制，可为负）", "", true),
                text("base", "基数（十进制，可为负）", "", true),
                number("precision", "结果小数位（0–12）", "4"),
                select(
                    "rounding",
                    "取整方式",
                    "nearest",
                    &["nearest", "floor", "ceil", "truncate"],
                ),
            ],
            &[],
        ),
        descriptor(
            "CAP-CALC-002",
            "calculation",
            "英尺英寸换算",
            "按 1 英寸 = 2.54 cm 换算；英尺为非负整数，英寸小于 12。",
            0,
            false,
            vec![
                number("feet", "英尺", "0"),
                number("inches", "英寸（0 ≤ x < 12）", "0"),
                select("unit", "目标单位", "cm", &["cm", "m", "mm"]),
                number("precision", "结果小数位（0–12）", "4"),
            ],
            &[],
        ),
        descriptor(
            "CAP-CALC-003",
            "calculation",
            "固定时长换算",
            "一天固定为 86400 秒；不将日历日期、时区或夏令时混为固定时长。",
            0,
            false,
            vec![
                number("amount", "时长数量", "1"),
                select(
                    "from",
                    "源单位",
                    "days",
                    &["seconds", "minutes", "hours", "days", "weeks"],
                ),
                select(
                    "to",
                    "目标单位",
                    "seconds",
                    &["seconds", "minutes", "hours", "days", "weeks"],
                ),
                number("precision", "结果小数位（0–12）", "4"),
            ],
            &[],
        ),
        descriptor(
            "CAP-CALC-004",
            "calculation",
            "平方根",
            "可选择实数或复数范围；负数不产生虚构的实数结果。",
            0,
            false,
            vec![
                text("value", "数值（可为负）", "", true),
                select("domain", "计算范围", "real", &["real", "complex"]),
                number("precision", "结果小数位（0–12）", "4"),
            ],
            &[],
        ),
    ];
    for (id, title, description) in [
        (
            "CAP-FILE-011",
            "统计文件夹大小",
            "列出已读取文件总大小、范围和不可读取项；不跟随符号链接。",
        ),
        (
            "CAP-FILE-013",
            "查找最大文件",
            "在指定范围按逻辑或占用大小排序；不完整范围单独说明。",
        ),
        (
            "CAP-FILE-014",
            "查找内容重复文件",
            "完整读取并按大小与 SHA-256 分组，不自动删除文件。",
        ),
    ] {
        let mut fields = traversal();
        if id != "CAP-FILE-014" {
            fields.push(select(
                "sizeKind",
                "大小口径",
                "logical",
                &["logical", "allocated"],
            ));
            fields.push(unit());
        }
        if id == "CAP-FILE-013" {
            fields.push(number("limit", "最多结果数（1–10000）", "20"));
        }
        result.push(descriptor(
            id,
            "file",
            title,
            description,
            0,
            false,
            fields,
            &[],
        ));
    }
    for (id, title) in [
        ("CAP-FILE-017", "查找并选择当前目录文件"),
        ("CAP-FILE-018", "深层查找文件"),
        ("CAP-FILE-019", "检索当前目录 PDF 正文"),
        ("CAP-FILE-020", "跨目录检索 PDF 正文"),
    ] {
        let pdf = id == "CAP-FILE-019" || id == "CAP-FILE-020";
        let mut fields = vec![
            directory(),
            toggle("includeHidden", "包含隐藏项", false),
            select("action", "结果动作", "reveal", &["list", "reveal"]),
        ];
        if id != "CAP-FILE-017" {
            fields.push(toggle(
                "recursive",
                "递归子目录（不跟随符号链接）",
                id != "CAP-FILE-019",
            ));
        }
        if pdf {
            fields.extend([
                text("keyword", "正文关键词（字面匹配）", "", true),
                toggle("caseSensitive", "正文匹配区分大小写", false),
                select(
                    "ocr",
                    "扫描页识别（auto 需要 Poppler 与 Tesseract）",
                    "auto",
                    &["auto", "never"],
                ),
                text("ocrLanguage", "OCR 语言模型", "chi_sim+eng", true),
            ]);
        } else {
            fields.push(text(
                "extensions",
                "匹配扩展名（逗号分隔，不区分大小写）",
                "mp4",
                true,
            ));
        }
        result.push(descriptor(id,"file",title,"列出匹配文件与不可检索条目；跨目录按组定位，最后一组是 Finder 当前选择。OCR 需已安装工具和语言模型。",0,false,fields,&[]));
    }
    let mut fields = traversal();
    fields.extend([
        text(
            "extensions",
            "统计扩展名（逗号分隔）",
            "js,jsx,mjs,cjs",
            true,
        ),
        text(
            "exclude",
            "排除目录名称（逗号分隔）",
            "node_modules,.git,target,vendor,dist,build",
            false,
        ),
        select(
            "countKind",
            "行数口径",
            "physical",
            &["physical", "nonBlank"],
        ),
    ]);
    result.push(descriptor(
        "CAP-DEV-005",
        "developer",
        "按扩展名统计代码行数",
        "流式统计所选扩展名；默认排除依赖和构建目录，末行无换行仍计一行。",
        0,
        false,
        fields,
        &[],
    ));
    for (id, title) in [
        ("CAP-TOOLS-003", "安装指定 Homebrew 包"),
        ("CAP-TOOLS-004", "卸载指定 Homebrew 包"),
    ] {
        result.push(descriptor(id,"tools",title,"仅处理指定 formula/cask；安装使用 Homebrew 校验，结束后重查状态。卸载不请求 zap/autoremove，已有共享依赖由 Homebrew 保护。",0,true,vec![select("manager","包管理器","homebrew",&["homebrew"]),text("package","包名或 tap/formula（如 ghostscript）","",true),select("source","目录来源类型","formula",&["formula","cask"]),text("version","formula 版本目录后缀（需上游提供 @版本，留空用默认）","",false)],&["brew"]));
    }
    result
}
