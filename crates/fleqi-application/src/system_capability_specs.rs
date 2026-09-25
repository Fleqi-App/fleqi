//! 系统与网络能力的 UI 表单、影响和参数规格。
use crate::capability_service::{CapabilityDescriptor, CapabilityField, field};

fn text(key: &str, label: &str, default: &str, required: bool) -> CapabilityField {
    field(key, label, "text", default, &[], required)
}
fn number(key: &str, label: &str, default: &str) -> CapabilityField {
    field(key, label, "number", default, &[], true)
}
fn select(key: &str, label: &str, default: &str, choices: &[&str]) -> CapabilityField {
    field(key, label, "select", default, choices, true)
}
fn descriptor(
    id: &str,
    title: &str,
    description: &str,
    minimum_inputs: usize,
    changes_files: bool,
    fields: Vec<CapabilityField>,
) -> CapabilityDescriptor {
    let network = id.starts_with("CAP-NETWORK-");
    CapabilityDescriptor {
        id: id.into(),
        category: if network { "network" } else { "system" }.into(),
        title: title.into(),
        description: description.into(),
        minimum_inputs,
        changes_files,
        grouped_inputs: true,
        fields,
        dependencies: match id {
            "CAP-SYSTEM-002" => vec!["DEP-PLATFORM".into(), "DEP-TERM".into()],
            "CAP-SYSTEM-014" => vec!["DEP-PLATFORM".into(), "DEP-FS".into()],
            "CAP-SYSTEM-015" => vec!["DEP-MESSAGES".into(), "DEP-FS".into()],
            "CAP-NETWORK-001" => vec!["DEP-NET".into(), "DEP-PLATFORM".into()],
            "CAP-NETWORK-002" => vec!["DEP-NET".into(), "DEP-FS".into()],
            "CAP-NETWORK-003" => vec!["DEP-NET".into(), "DEP-WEATHER".into()],
            _ => vec!["DEP-PLATFORM".into()],
        },
    }
}

pub fn descriptors() -> Vec<CapabilityDescriptor> {
    vec![
        descriptor(
            "CAP-SYSTEM-001",
            "在文本编辑器中打开",
            "在系统默认文本编辑器或指定已安装应用中打开选中文件。",
            1,
            true,
            vec![text(
                "editor",
                "编辑器名称或应用路径（留空使用默认文本编辑器）",
                "",
                false,
            )],
        ),
        descriptor(
            "CAP-SYSTEM-002",
            "在目录打开系统终端",
            "创建新的交互终端并切换到当前目录；需要应用及自动化权限。",
            0,
            true,
            vec![select(
                "terminal",
                "终端应用",
                "Terminal",
                &["Terminal", "iTerm"],
            )],
        ),
        descriptor(
            "CAP-SYSTEM-003",
            "弹出可移除卷",
            "弹出当前目录所在的外部可移除卷；拒绝系统卷和内部磁盘，繁忙失败如实报告。",
            0,
            true,
            vec![text(
                "volume",
                "卷路径（留空使用当前目录所在卷）",
                "",
                false,
            )],
        ),
        descriptor(
            "CAP-SYSTEM-004",
            "限时保持唤醒",
            "任务运行期间保持指定电源断言，到期或取消后释放；system 类型需要交流电源。",
            0,
            true,
            vec![
                number("seconds", "持续时间（秒，1–86400）", "3600"),
                select("type", "断言类型", "idle", &["idle", "display", "system"]),
            ],
        ),
        descriptor(
            "CAP-SYSTEM-005",
            "设置系统外观",
            "设置或切换 macOS 的真实外观，需要 System Events 自动化权限。",
            0,
            true,
            vec![select(
                "appearance",
                "系统外观",
                "toggle",
                &["toggle", "light", "dark"],
            )],
        ),
        descriptor(
            "CAP-SYSTEM-006",
            "设置 Finder 隐藏项显示",
            "修改 Finder 隐藏项显示设置并重启 Finder；窗口会刷新。",
            0,
            true,
            vec![select("visible", "显示隐藏项", "true", &["true", "false"])],
        ),
        descriptor(
            "CAP-SYSTEM-007",
            "提交打印队列",
            "将选中文件提交至系统打印队列，返回受理任务 ID；受理不表示纸张已打印。",
            1,
            true,
            vec![
                text("printer", "打印机队列名（留空使用系统默认）", "", false),
                number("copies", "份数（1–999）", "1"),
                text("pages", "输出页范围（如 1,3-5；留空为全部）", "", false),
                select(
                    "sides",
                    "单双面",
                    "one-sided",
                    &["one-sided", "two-sided-long-edge", "two-sided-short-edge"],
                ),
                text("media", "纸张（如 A4，留空使用打印机默认）", "", false),
            ],
        ),
        descriptor(
            "CAP-SYSTEM-008",
            "请求系统休眠",
            "在指定延迟后向系统请求休眠；任务参数先写入历史，实际进入和恢复需由系统事件核对。",
            0,
            true,
            vec![number("delaySeconds", "延迟时间（秒，0 为立即）", "0")],
        ),
        descriptor(
            "CAP-SYSTEM-009",
            "查看处理器",
            "读取当前设备 CPU/SoC 名称与架构；缺失字段不猜测。",
            0,
            false,
            vec![select(
                "fields",
                "显示字段",
                "all",
                &["all", "name", "architecture"],
            )],
        ),
        descriptor(
            "CAP-SYSTEM-010",
            "查看内存",
            "读取物理内存总量及系统分页分类快照，区分总量、活动页、缓存及压缩器实际占用。",
            0,
            false,
            vec![
                select("unit", "显示单位", "GiB", &["bytes", "GiB", "GB"]),
                select("scope", "查询范围", "all", &["total", "usage", "all"]),
            ],
        ),
        descriptor(
            "CAP-SYSTEM-011",
            "查看显示器分辨率",
            "逐个显示系统报告的像素与逻辑分辨率，标明比例和缺失字段。",
            0,
            false,
            vec![
                text("display", "显示器 ID（all 表示全部）", "all", true),
                select(
                    "scope",
                    "分辨率口径",
                    "both",
                    &["both", "physical", "logical"],
                ),
            ],
        ),
        descriptor(
            "CAP-SYSTEM-012",
            "查看充电适配器功率",
            "区分适配器额定/协商功率与实时功率；未连接或系统未提供字段时说明未知。",
            0,
            false,
            vec![select("scope", "功率口径", "rated", &["rated", "measured"])],
        ),
        descriptor(
            "CAP-SYSTEM-013",
            "查看电池充满时间",
            "读取系统电池状态及估计，区分充电、已满、未充电和未知。",
            0,
            false,
            vec![],
        ),
        descriptor(
            "CAP-SYSTEM-014",
            "设置文件隐藏属性",
            "修改并回读选中文件的 hidden 属性；Finder 显示隐藏项时仍可见，点名称保持不变。",
            1,
            true,
            vec![select("hidden", "隐藏文件", "true", &["true", "false"])],
        ),
        descriptor(
            "CAP-SYSTEM-015",
            "通过 Messages 发送",
            "向显式收件人发送正文及可选附件，需要已登录的 iMessage 账号和自动化权限；仅报告服务受理。",
            0,
            true,
            vec![
                text(
                    "recipient",
                    "收件人（带国家码的电话或 iMessage 邮箱）",
                    "",
                    true,
                ),
                select("service", "消息服务", "iMessage", &["iMessage"]),
                field("text", "消息正文", "textarea", "", &[], false),
                select("attachmentMode", "附件", "none", &["none", "selected"]),
            ],
        ),
        descriptor(
            "CAP-NETWORK-001",
            "探测网络主机",
            "对指定主机进行 ICMP 探测，显示真实延迟和丢包；不静默替换协议。",
            0,
            false,
            vec![
                text("host", "目标主机或 IP", "", true),
                number("count", "次数（1–100）", "4"),
                number("timeoutSeconds", "等待时间（秒，1–60）", "3"),
                select("protocol", "协议", "icmp", &["icmp"]),
            ],
        ),
        descriptor(
            "CAP-NETWORK-002",
            "下载 HTTP(S) 文件",
            "下载到临时文件，成功后按重名策略发布；HTTP 错误、中断及取消不会留下伪成功文件。",
            0,
            true,
            vec![
                text("url", "下载 URL", "", true),
                text(
                    "destination",
                    "保存路径（相对当前目录或绝对路径）",
                    "",
                    true,
                ),
                select(
                    "collision",
                    "重名策略",
                    "rename",
                    &["rename", "error", "overwrite"],
                ),
            ],
        ),
        descriptor(
            "CAP-NETWORK-003",
            "查询地点天气",
            "通过 Open-Meteo 查询地点天气和预报；歧义地点返回候选 ID，结果标注来源与时间。",
            0,
            false,
            vec![
                text("location", "地点或纬度,经度", "", true),
                text("locationId", "候选地点 ID（出现歧义后填写）", "", false),
                select("unit", "温度单位", "celsius", &["celsius", "fahrenheit"]),
                select("range", "时间范围", "current", &["current", "forecast"]),
                number("days", "预报天数（1–16）", "3"),
                text("startDate", "预报开始日期（YYYY-MM-DD，可选）", "", false),
                text("endDate", "预报结束日期（YYYY-MM-DD，可选）", "", false),
            ],
        ),
    ]
}
