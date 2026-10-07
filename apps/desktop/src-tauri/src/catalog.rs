//! 六类 30 项基础能力目录（capabilities.md §2 的参数化数据）。
//! 标题/输入/依赖以台账行为准；用户调用时参数化，不保留示例值。

use crate::commands_m3::CatalogEntry;

pub fn builtin_catalog() -> Vec<CatalogEntry> {
    let entry = |id: &str,
                 category: &str,
                 title: &str,
                 description: &str,
                 inputs: &str,
                 dependencies: &[&str]| CatalogEntry {
        availability: fleqi_domain::platform::CapabilityState::Supported,
        unavailable_reason: None,
        id: id.into(),
        category: category.into(),
        title: title.into(),
        description: description.into(),
        inputs: inputs.into(),
        dependencies: dependencies.iter().map(|d| d.to_string()).collect(),
    };
    let mut entries = vec![
        entry(
            "CAP-FILE-001",
            "file",
            "创建文本文件",
            "按名称/内容/编码/位置创建新文件",
            "有效目录及正文",
            &["DEP-FS"],
        ),
        entry(
            "CAP-FILE-002",
            "file",
            "创建文件夹",
            "单层或显式多层目录；同名返回真实状态",
            "有效父目录",
            &["DEP-FS"],
        ),
        entry(
            "CAP-FILE-003",
            "file",
            "复制",
            "文件与文件夹混合选区，逐项报告",
            "选区 + 目标目录",
            &["DEP-FS"],
        ),
        entry(
            "CAP-FILE-004",
            "file",
            "移动",
            "同卷/跨卷，部分完成可见",
            "选区 + 目标目录",
            &["DEP-FS"],
        ),
        entry(
            "CAP-FILE-005",
            "file",
            "改名",
            "模板/前后缀/日期/大小写规则，预览一致",
            "一项或多项",
            &["DEP-FS"],
        ),
        entry(
            "CAP-FILE-006",
            "file",
            "批量编号",
            "起始/步长/位数/位置/排序",
            "两项及以上",
            &["DEP-FS"],
        ),
        entry(
            "CAP-FILE-007",
            "file",
            "整理",
            "按类型/日期/规则分类移动",
            "目录或选区",
            &["DEP-FS"],
        ),
        entry(
            "CAP-FILE-008",
            "file",
            "移入回收站",
            "可从平台回收站恢复",
            "选区",
            &["DEP-FS", "DEP-PLATFORM"],
        ),
        entry(
            "CAP-ZIP-001",
            "zip",
            "ZIP 打包",
            "保留约定相对结构",
            "文件/文件夹",
            &["DEP-ZIP"],
        ),
        entry(
            "CAP-ZIP-002",
            "zip",
            "查看 ZIP 内容",
            "路径/大小/类型，无需全量解压",
            "一个 ZIP",
            &["DEP-ZIP"],
        ),
        entry(
            "CAP-ZIP-003",
            "zip",
            "ZIP 解压",
            "安全目标目录与逐项结果",
            "普通未加密 ZIP",
            &["DEP-ZIP"],
        ),
        entry(
            "CAP-IMAGE-001",
            "image",
            "图片格式转换",
            "PNG/JPG/WebP 六个方向；质量参数",
            "图像",
            &["DEP-IMAGE"],
        ),
        entry(
            "CAP-IMAGE-002",
            "image",
            "缩放/缩略图",
            "宽/高/边界框；默认不放大",
            "图像",
            &["DEP-IMAGE"],
        ),
        entry(
            "CAP-IMAGE-003",
            "image",
            "旋转图片",
            "90/180/270 度",
            "图像",
            &["DEP-IMAGE"],
        ),
        entry(
            "CAP-IMAGE-004",
            "image",
            "JPG 压缩",
            "质量参数；报告大小变化",
            "JPG",
            &["DEP-IMAGE"],
        ),
        entry(
            "CAP-MEDIA-001",
            "media",
            "音频转换",
            "MP3/M4A/WAV 六方向；质量显式",
            "音频",
            &["DEP-MEDIA"],
        ),
        entry(
            "CAP-MEDIA-002",
            "media",
            "视频格式转换",
            "编码矩阵列明；错误格式明确失败",
            "支持解码的媒体",
            &["DEP-MEDIA"],
        ),
        entry(
            "CAP-MEDIA-003",
            "media",
            "提取音频",
            "音轨选择；无音轨可读失败",
            "带音轨的视频",
            &["DEP-MEDIA"],
        ),
        entry(
            "CAP-MEDIA-004",
            "media",
            "裁剪",
            "精确或无重编码模式",
            "音频/视频",
            &["DEP-MEDIA"],
        ),
        entry(
            "CAP-PDF-001",
            "pdf",
            "合并 PDF",
            "页序与输入顺序一致",
            "两个及以上 PDF",
            &["DEP-PDF"],
        ),
        entry(
            "CAP-PDF-002",
            "pdf",
            "拆分 PDF",
            "逐页或页组",
            "普通 PDF",
            &["DEP-PDF"],
        ),
        entry(
            "CAP-PDF-003",
            "pdf",
            "提取页面",
            "页号/范围；越界拒绝",
            "普通 PDF",
            &["DEP-PDF"],
        ),
        entry(
            "CAP-PDF-004",
            "pdf",
            "旋转页面",
            "页范围 + 角度",
            "普通 PDF",
            &["DEP-PDF"],
        ),
        entry(
            "CAP-PDF-005",
            "pdf",
            "结构压缩",
            "不主动栅格化；可报告无缩小空间",
            "普通 PDF",
            &["DEP-PDF"],
        ),
        entry(
            "CAP-TEXT-001",
            "text",
            "读取 TXT",
            "编码与范围；非法编码报错",
            "TXT",
            &["DEP-FS"],
        ),
        entry(
            "CAP-TEXT-002",
            "text",
            "创建 TXT",
            "正文/编码/换行",
            "用户正文",
            &["DEP-FS"],
        ),
        entry(
            "CAP-TEXT-003",
            "text",
            "读取 Markdown",
            "保留源文",
            "MD/Markdown",
            &["DEP-FS"],
        ),
        entry(
            "CAP-TEXT-004",
            "text",
            "创建 Markdown",
            "原样保留格式标记",
            "用户正文",
            &["DEP-FS"],
        ),
        entry(
            "CAP-TEXT-005",
            "text",
            "创建 DOCX",
            "段落/标题/列表",
            "结构化正文",
            &["DEP-DOCX"],
        ),
        entry(
            "CAP-TEXT-006",
            "text",
            "提取 DOCX 正文",
            "主文档段落与表格；范围明示",
            "DOCX",
            &["DEP-DOCX"],
        ),
    ];
    for (id, title, description) in [
        (
            "CAP-IMAGE-007",
            "移除图片元数据",
            "重新编码并移除 EXIF/XMP；保留原件",
        ),
        ("CAP-IMAGE-008", "裁切图片", "分别指定四边裁切量；越界拒绝"),
        (
            "CAP-IMAGE-009",
            "颜色透明化",
            "指定颜色和容差，输出透明 PNG",
        ),
        (
            "CAP-IMAGE-010",
            "生成 ICNS 图标",
            "生成 macOS 所需的多尺寸图层",
        ),
        ("CAP-IMAGE-011", "生成 ICO 图标", "自选图层尺寸，保留透明度"),
        (
            "CAP-IMAGE-012",
            "图片合成 GIF",
            "按选区顺序、帧时长与循环次数生成动画",
        ),
        ("CAP-IMAGE-013", "模糊图片", "指定半径，保持尺寸与原件"),
        ("CAP-IMAGE-014", "添加边框", "指定边框宽度和颜色"),
        (
            "CAP-IMAGE-015",
            "网格拼图",
            "指定行列、间距、单格尺寸与背景",
        ),
        ("CAP-IMAGE-016", "图片着色", "指定颜色与强度，保持 alpha"),
        (
            "CAP-IMAGE-017",
            "覆盖文字",
            "用户文字、字体、字号、颜色与位置",
        ),
    ] {
        entries.push(entry(
            id,
            "image",
            title,
            description,
            "Finder 图像选区",
            &["DEP-IMAGE-EXT"],
        ));
    }
    for (id, category, title, description) in [
        (
            "CAP-IMAGE-005",
            "image",
            "图像尺寸与方向",
            "区分存储尺寸与应用方向后的显示尺寸",
        ),
        (
            "CAP-IMAGE-006",
            "image",
            "识别图片文字",
            "使用指定的本地 OCR 语言模型",
        ),
        (
            "CAP-MEDIA-005",
            "media",
            "媒体时长",
            "真实容器时长，未知不当作零",
        ),
        (
            "CAP-MEDIA-006",
            "media",
            "视频分辨率",
            "指定轨道的编码尺寸、显示尺寸与方向",
        ),
        (
            "CAP-MEDIA-007",
            "media",
            "媒体码率",
            "区分容器与轨道的声明码率",
        ),
        (
            "CAP-MEDIA-008",
            "media",
            "视频编码",
            "读取指定视频轨道的真实编码信息",
        ),
        (
            "CAP-MEDIA-009",
            "media",
            "文件转写",
            "使用本地 whisper 模型生成正文",
        ),
        (
            "CAP-MEDIA-010",
            "media",
            "生成 SRT 字幕",
            "转写并输出带时间戳的 SRT",
        ),
        (
            "CAP-MEDIA-011",
            "media",
            "生成 WebVTT 字幕",
            "转写并输出标准 WebVTT",
        ),
        (
            "CAP-MEDIA-012",
            "media",
            "旋转视频",
            "保留音轨并按指定角度重新编码",
        ),
        (
            "CAP-MEDIA-013",
            "media",
            "按比例裁剪视频",
            "参数化宽高比与裁切锚点，不拉伸画面",
        ),
    ] {
        entries.push(entry(
            id,
            category,
            title,
            description,
            "Finder 文件选区",
            &["DEP-MEDIA"],
        ));
    }
    for (id, title, description) in [
        (
            "CAP-PDF-006",
            "PDF 页数",
            "读取真实页数，支持显式提供加密口令",
        ),
        (
            "CAP-PDF-007",
            "PDF 作者信息",
            "读取 Info.Author；未设置时明确为空",
        ),
        (
            "CAP-PDF-009",
            "提取 PDF 嵌入图片",
            "按页范围提取原格式或 PNG 图像",
        ),
        (
            "CAP-PDF-010",
            "移除 PDF 元数据",
            "分别移除 Info、XMP 或两者，保留页面",
        ),
        (
            "CAP-PDF-011",
            "解除 PDF 口令",
            "使用用户提供的正确口令生成新文档",
        ),
        (
            "CAP-PDF-012",
            "加密 PDF",
            "使用独立打开/管理口令和 AES-256，设置打印/提取权限",
        ),
    ] {
        entries.push(entry(
            id,
            "pdf",
            title,
            description,
            "PDF 文件",
            &["DEP-PDF"],
        ));
    }
    for (id, category, title, description) in [
        (
            "CAP-TEXT-007",
            "text",
            "文本字词统计",
            "区分空白分词、Unicode 字符和 CJK 字符",
        ),
        (
            "CAP-TEXT-008",
            "text",
            "文档字词统计",
            "提取 DOC/DOCX/DOCM/ODT/RTFD/RTF 正文后统计",
        ),
        (
            "CAP-TEXT-009",
            "text",
            "文本摘要",
            "使用用户配置的模型，基于已读取正文生成摘要",
        ),
        (
            "CAP-TEXT-010",
            "text",
            "文档摘要",
            "先提取真实正文，再请求指定摘要模型",
        ),
        (
            "CAP-TEXT-011",
            "text",
            "提取可读正文",
            "只读展示或生成 TXT；明确内容与截取范围",
        ),
        (
            "CAP-PDF-008",
            "pdf",
            "PDF 摘要",
            "按页范围提取正文；扫描页按需 OCR",
        ),
    ] {
        entries.push(entry(
            id,
            category,
            title,
            description,
            "文本或文档",
            &["DEP-DOC-IMPORT"],
        ));
    }
    for spec in fleqi_application::capability_service::extended_descriptors() {
        entries.retain(|entry| entry.id != spec.id);
        entries.push(CatalogEntry {
            availability: fleqi_domain::platform::CapabilityState::Supported,
            unavailable_reason: None,
            id: spec.id,
            category: spec.category,
            title: spec.title,
            description: spec.description,
            inputs: if spec.minimum_inputs == 0 {
                "当前目录或可选选区".into()
            } else {
                format!("至少 {} 项", spec.minimum_inputs)
            },
            dependencies: spec.dependencies,
        });
    }
    for entry in &mut entries {
        if !fleqi_application::capability_service::platform_available(&entry.id) {
            entry.availability = fleqi_domain::platform::CapabilityState::Unsupported;
            entry.unavailable_reason = Some("Windows 暂未开放此能力".into());
        }
    }
    entries
}
