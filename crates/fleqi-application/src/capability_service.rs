//! 本地能力表单与计划：不依赖模型；字段校验在宿主重复执行。
use crate::{
    dto::{AppError, AppResult},
    run_service::RunSubmit,
};
use fleqi_domain::{
    context::ContextSnapshot,
    execution::{
        Effect, EffectKind, ExecutionPlan, ExecutionStep, PlanPreviewCompleteness, StepKind,
    },
    revision::Revision,
    settings::AiPolicy,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CapabilityField {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub default_value: String,
    pub required: bool,
    pub choices: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CapabilityForm {
    pub capability_id: String,
    pub context: ContextSnapshot,
    pub minimum_inputs: usize,
    pub fields: Vec<CapabilityField>,
    pub changes_files: bool,
}

/// 能力注册信息；平台执行器与 UI 表单共用此规格，字段类型从 Rust 导出。
pub struct CapabilityDescriptor {
    pub id: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub minimum_inputs: usize,
    pub changes_files: bool,
    pub grouped_inputs: bool,
    pub fields: Vec<CapabilityField>,
    pub dependencies: Vec<String>,
}

pub(crate) fn field(
    key: &str,
    label: &str,
    kind: &str,
    default: &str,
    choices: &[&str],
    required: bool,
) -> CapabilityField {
    CapabilityField {
        key: key.into(),
        label: label.into(),
        kind: kind.into(),
        default_value: default.into(),
        required,
        choices: choices.iter().map(|s| (*s).into()).collect(),
    }
}

pub fn extended_descriptors() -> Vec<CapabilityDescriptor> {
    let mut entries = crate::file_capability_specs::descriptors();
    entries.extend(crate::system_capability_specs::descriptors());
    entries.push(CapabilityDescriptor {
        id: "CAP-FILE-012".into(),
        category: "file".into(),
        title: "查看当前工作目录".into(),
        description: "分别显示 Finder 目标与可见会话的实际终端目录、同步状态".into(),
        minimum_inputs: 0,
        changes_files: false,
        grouped_inputs: true,
        fields: vec![],
        dependencies: vec!["DEP-PLATFORM".into()],
    });
    entries
}

pub fn form(capability_id: &str, context: ContextSnapshot) -> AppResult<CapabilityForm> {
    if let Some(spec) = extended_descriptors()
        .into_iter()
        .find(|spec| spec.id == capability_id)
    {
        return Ok(CapabilityForm {
            capability_id: spec.id,
            context,
            minimum_inputs: spec.minimum_inputs,
            fields: spec.fields,
            changes_files: spec.changes_files,
        });
    }
    let name = || field("name", "输出名称", "text", "", &[], true);
    let content = || field("content", "正文", "textarea", "", &[], false);
    let number = |key, label, default| field(key, label, "number", default, &[], true);
    let output = || field("name", "输出名称（留空自动生成）", "text", "", &[], false);
    let encoding = || {
        field(
            "encoding",
            "文字编码",
            "select",
            "utf-8",
            &["utf-8", "utf-16le", "utf-16be"],
            true,
        )
    };
    let (minimum_inputs, mut fields, changes_files) = match capability_id {
        "CAP-FILE-001" | "CAP-TEXT-002" => (
            0,
            vec![
                name(),
                content(),
                encoding(),
                field("newline", "换行", "select", "lf", &["lf", "crlf"], true),
            ],
            true,
        ),
        "CAP-FILE-002" => (0, vec![name()], true),
        "CAP-TEXT-004" | "CAP-TEXT-005" => (0, vec![name(), content()], true),
        "CAP-FILE-003" | "CAP-FILE-004" => (
            1,
            vec![field("destination", "目标文件夹", "text", "", &[], true)],
            true,
        ),
        "CAP-FILE-005" => (
            1,
            vec![field(
                "template",
                "名称模板（{name} / {stem} / {ext}）",
                "text",
                "{name}",
                &[],
                true,
            )],
            true,
        ),
        "CAP-FILE-006" => (
            2,
            vec![
                number("start", "起始序号", "1"),
                number("step", "序号步长", "1"),
                number("width", "序号位数", "3"),
                field(
                    "position",
                    "序号位置",
                    "select",
                    "prefix",
                    &["prefix", "suffix"],
                    true,
                ),
            ],
            true,
        ),
        "CAP-FILE-007" => (
            1,
            vec![field(
                "groupBy",
                "分类方式",
                "select",
                "extension",
                &["extension", "date"],
                true,
            )],
            true,
        ),
        "CAP-FILE-008" => (1, vec![], true),
        "CAP-ZIP-001" => (1, vec![name()], true),
        "CAP-ZIP-002" => (1, vec![], false),
        "CAP-ZIP-003" => (1, vec![name()], true),
        "CAP-IMAGE-001" => (
            1,
            vec![
                field(
                    "format",
                    "目标格式",
                    "select",
                    "png",
                    &["png", "jpg", "webp"],
                    true,
                ),
                number("quality", "JPG 质量（1–100）", "85"),
            ],
            true,
        ),
        "CAP-IMAGE-002" => (
            1,
            vec![
                field(
                    "mode",
                    "缩放约束",
                    "select",
                    "box",
                    &["box", "width", "height"],
                    true,
                ),
                field(
                    "upscale",
                    "允许放大",
                    "select",
                    "false",
                    &["false", "true"],
                    true,
                ),
                number("width", "最大宽度（像素）", "1920"),
                number("height", "最大高度（像素）", "1080"),
            ],
            true,
        ),
        "CAP-IMAGE-005" => (1, vec![], false),
        "CAP-IMAGE-006" => (
            1,
            vec![field(
                "language",
                "OCR 语言模型",
                "text",
                "chi_sim+eng",
                &[],
                true,
            )],
            false,
        ),
        "CAP-IMAGE-007" => (1, vec![], true),
        "CAP-IMAGE-008" => (
            1,
            vec![
                number("left", "左侧裁切（像素）", "0"),
                number("right", "右侧裁切（像素）", "0"),
                number("top", "顶部裁切（像素）", "0"),
                number("bottom", "底部裁切（像素）", "0"),
            ],
            true,
        ),
        "CAP-IMAGE-009" => (
            1,
            vec![
                field("color", "目标颜色（#RRGGBB）", "text", "#ffffff", &[], true),
                number("tolerance", "颜色容差（0–255）", "0"),
            ],
            true,
        ),
        "CAP-IMAGE-010" => (1, vec![], true),
        "CAP-IMAGE-011" => (
            1,
            vec![field(
                "sizes",
                "图层尺寸（逗号分隔，1–256）",
                "text",
                "16,32,48,64,128,256",
                &[],
                true,
            )],
            true,
        ),
        "CAP-IMAGE-012" => (
            2,
            vec![
                number("durationMs", "每帧时长（毫秒）", "100"),
                number("loops", "循环次数（0 表示无限）", "0"),
            ],
            true,
        ),
        "CAP-IMAGE-013" => (1, vec![number("radius", "模糊半径（0–100）", "2")], true),
        "CAP-IMAGE-014" => (
            1,
            vec![
                number("width", "边框宽度（像素）", "10"),
                field("color", "边框颜色", "text", "#000000", &[], true),
            ],
            true,
        ),
        "CAP-IMAGE-015" => (
            1,
            vec![
                number("columns", "网格列数", "3"),
                number("rows", "网格行数", "3"),
                number("cellSize", "单格尺寸（像素）", "256"),
                number("gap", "间距（像素）", "0"),
                field("color", "背景颜色", "text", "#ffffff", &[], true),
            ],
            true,
        ),
        "CAP-IMAGE-016" => (
            1,
            vec![
                field("color", "着色颜色", "text", "#0000ff", &[], true),
                number("strength", "强度（0–1）", "0.3"),
            ],
            true,
        ),
        "CAP-IMAGE-017" => (
            1,
            vec![
                field("text", "覆盖文字", "textarea", "", &[], true),
                field(
                    "font",
                    "字体名称或文件",
                    "text",
                    "/System/Library/Fonts/Hiragino Sans GB.ttc",
                    &[],
                    true,
                ),
                number("size", "字号", "32"),
                field("color", "文字颜色", "text", "#000000", &[], true),
                number("x", "横向位置（像素）", "0"),
                number("y", "纵向位置（像素）", "0"),
            ],
            true,
        ),
        "CAP-IMAGE-003" => (
            1,
            vec![field(
                "angle",
                "顺时针旋转角度",
                "select",
                "90",
                &["90", "180", "270"],
                true,
            )],
            true,
        ),
        "CAP-IMAGE-004" => (1, vec![number("quality", "JPG 质量（1–100）", "80")], true),
        "CAP-MEDIA-001" | "CAP-MEDIA-003" => (
            1,
            vec![
                field(
                    "format",
                    "音频格式",
                    "select",
                    "mp3",
                    &["mp3", "m4a", "wav"],
                    true,
                ),
                number("bitrate", "音频码率（kbps）", "192"),
            ],
            true,
        ),
        "CAP-MEDIA-002" => (
            1,
            vec![
                field(
                    "format",
                    "视频格式",
                    "select",
                    "mp4",
                    &["mp4", "mov", "mkv"],
                    true,
                ),
                number("quality", "视频 CRF（0–51，越小越清晰）", "23"),
            ],
            true,
        ),
        "CAP-MEDIA-004" => (
            1,
            vec![
                number("start", "起始时间（秒）", "0"),
                number("duration", "持续时间（秒）", "10"),
                field(
                    "mode",
                    "裁剪模式",
                    "select",
                    "precise",
                    &["precise", "copy"],
                    true,
                ),
            ],
            true,
        ),
        "CAP-MEDIA-005" => (1, vec![], false),
        "CAP-MEDIA-006" | "CAP-MEDIA-008" => (
            1,
            vec![number("track", "视频轨道序号（从 0 开始）", "0")],
            false,
        ),
        "CAP-MEDIA-007" => (
            1,
            vec![
                field(
                    "scope",
                    "码率来源",
                    "select",
                    "container",
                    &["container", "stream"],
                    true,
                ),
                number("track", "视频轨道序号（从 0 开始）", "0"),
            ],
            false,
        ),
        "CAP-MEDIA-009" | "CAP-MEDIA-010" | "CAP-MEDIA-011" => (
            1,
            vec![
                field("language", "转写语言", "text", "auto", &[], true),
                field(
                    "model",
                    "本地 ggml 模型路径（留空自动查找）",
                    "text",
                    "",
                    &[],
                    false,
                ),
            ],
            true,
        ),
        "CAP-MEDIA-012" => (
            1,
            vec![
                field(
                    "angle",
                    "旋转角度",
                    "select",
                    "90",
                    &["90", "180", "270"],
                    true,
                ),
                number("track", "视频轨道序号（从 0 开始）", "0"),
            ],
            true,
        ),
        "CAP-MEDIA-013" => (
            1,
            vec![
                number("ratioWidth", "目标宽高比：宽", "16"),
                number("ratioHeight", "目标宽高比：高", "10"),
                field(
                    "anchor",
                    "裁切锚点",
                    "select",
                    "center",
                    &["center", "topLeft", "bottomRight"],
                    true,
                ),
                number("track", "视频轨道序号（从 0 开始）", "0"),
            ],
            true,
        ),
        "CAP-PDF-001" => (2, vec![name()], true),
        "CAP-PDF-002" => (1, vec![number("pagesPerFile", "每份页数", "1")], true),
        "CAP-PDF-003" => (
            1,
            vec![
                field("pages", "页码或范围（如 1,3-5）", "text", "", &[], true),
                output(),
            ],
            true,
        ),
        "CAP-PDF-004" => (
            1,
            vec![
                field("pages", "页码或范围", "text", "", &[], true),
                field(
                    "angle",
                    "旋转角度",
                    "select",
                    "90",
                    &["90", "180", "270"],
                    true,
                ),
                output(),
            ],
            true,
        ),
        "CAP-PDF-005" => (1, vec![output()], true),
        "CAP-PDF-006" | "CAP-PDF-007" => (
            1,
            vec![field(
                "password",
                "打开口令（仅加密文件需要）",
                "password",
                "",
                &[],
                false,
            )],
            false,
        ),
        "CAP-PDF-009" => (
            1,
            vec![
                field(
                    "password",
                    "打开口令（仅加密文件需要）",
                    "password",
                    "",
                    &[],
                    false,
                ),
                number("firstPage", "起始页", "1"),
                field("lastPage", "结束页（留空到末页）", "number", "", &[], false),
                field(
                    "format",
                    "图像输出格式",
                    "select",
                    "png",
                    &["png", "original"],
                    true,
                ),
            ],
            true,
        ),
        "CAP-PDF-010" => (
            1,
            vec![
                field(
                    "password",
                    "打开口令（仅加密文件需要）",
                    "password",
                    "",
                    &[],
                    false,
                ),
                field(
                    "scope",
                    "移除范围",
                    "select",
                    "all",
                    &["all", "info", "xmp"],
                    true,
                ),
            ],
            true,
        ),
        "CAP-PDF-011" => (
            1,
            vec![field("password", "当前打开口令", "password", "", &[], true)],
            true,
        ),
        "CAP-PDF-012" => (
            1,
            vec![
                field("password", "新的打开口令", "password", "", &[], true),
                field(
                    "ownerPassword",
                    "管理口令（须与打开口令不同）",
                    "password",
                    "",
                    &[],
                    true,
                ),
                field(
                    "printing",
                    "允许打印",
                    "select",
                    "full",
                    &["full", "low", "none"],
                    true,
                ),
                field(
                    "allowExtract",
                    "允许复制内容",
                    "select",
                    "true",
                    &["true", "false"],
                    true,
                ),
            ],
            true,
        ),
        "CAP-TEXT-001" | "CAP-TEXT-003" => (
            1,
            vec![
                field(
                    "encoding",
                    "读取编码",
                    "select",
                    "auto",
                    &["auto", "utf-8", "utf-16le", "utf-16be"],
                    true,
                ),
                number("offset", "起始位置（字节）", "0"),
                number("maxBytes", "最多读取字节数", "65536"),
            ],
            false,
        ),
        "CAP-TEXT-006" | "CAP-TEXT-011" => (
            1,
            vec![field(
                "output",
                "输出方式",
                "select",
                "view",
                &["view", "file"],
                true,
            )],
            false,
        ),
        "CAP-TEXT-007" | "CAP-TEXT-008" => (
            1,
            vec![field(
                "unit",
                "统计口径",
                "select",
                "words",
                &["words", "characters", "cjk"],
                true,
            )],
            false,
        ),
        "CAP-TEXT-009" | "CAP-TEXT-010" => (
            1,
            vec![
                field("language", "摘要语言", "text", "中文", &[], true),
                number("length", "摘要目标字数", "300"),
            ],
            false,
        ),
        "CAP-PDF-008" => (
            1,
            vec![
                field("pages", "页码/范围（留空全文）", "text", "", &[], false),
                field(
                    "ocr",
                    "扫描页文字识别",
                    "select",
                    "auto",
                    &["auto", "never"],
                    true,
                ),
                field(
                    "ocrLanguage",
                    "OCR 语言模型",
                    "text",
                    "chi_sim+eng",
                    &[],
                    true,
                ),
                field(
                    "password",
                    "打开口令（加密时需要）",
                    "password",
                    "",
                    &[],
                    false,
                ),
                field("language", "摘要语言", "text", "中文", &[], true),
                number("length", "摘要目标字数", "300"),
            ],
            false,
        ),
        _ => return Err(AppError::not_found("未登记的本地能力")),
    };
    if capability_id == "CAP-IMAGE-002" {
        fields.push(field(
            "formats",
            "目录输入扩展名（逗号分隔，留空全部）",
            "text",
            "",
            &[],
            false,
        ));
        fields.push(field(
            "recursive",
            "目录输入包含子目录",
            "select",
            "false",
            &["false", "true"],
            true,
        ));
    }
    if matches!(capability_id, "CAP-IMAGE-001" | "CAP-IMAGE-004") {
        fields.push(field(
            "alphaPolicy",
            "透明图片转 JPG",
            "select",
            "reject",
            &["reject", "flatten"],
            true,
        ));
        fields.push(field(
            "background",
            "JPG 合成背景",
            "text",
            "#ffffff",
            &[],
            true,
        ));
    }
    if capability_id == "CAP-IMAGE-009" {
        fields.extend([
            field(
                "format",
                "输出格式",
                "select",
                "png",
                &["png", "webp", "jpg"],
                true,
            ),
            field(
                "alphaPolicy",
                "JPEG 透明度替代策略",
                "select",
                "reject",
                &["reject", "flatten"],
                true,
            ),
            field("background", "JPEG 合成背景", "text", "#ffffff", &[], true),
        ]);
    }
    if supports_conversion_cleanup(capability_id) {
        fields.push(field(
            "sourceHandling",
            "转换成功后",
            "select",
            "keep",
            &["keep", "trashAfterSuccess"],
            true,
        ));
    }
    Ok(CapabilityForm {
        capability_id: capability_id.into(),
        context,
        minimum_inputs,
        fields,
        changes_files,
    })
}

pub fn supports_conversion_cleanup(id: &str) -> bool {
    matches!(id, "CAP-IMAGE-001" | "CAP-MEDIA-001" | "CAP-MEDIA-002")
}

pub fn form_with_settings(
    id: &str,
    context: ContextSnapshot,
    settings: &fleqi_domain::settings::Settings,
) -> AppResult<CapabilityForm> {
    let mut form = form(id, context)?;
    if let Some(field) = form
        .fields
        .iter_mut()
        .find(|field| field.key == "sourceHandling")
    {
        field.default_value = match settings.conversion_source_handling {
            fleqi_domain::settings::ConversionSourceHandling::Keep => "keep",
            fleqi_domain::settings::ConversionSourceHandling::TrashAfterSuccess => {
                "trashAfterSuccess"
            }
        }
        .into();
    }
    Ok(form)
}

pub fn plan(
    form: &CapabilityForm,
    parameters: &BTreeMap<String, String>,
    session_id: String,
    directory: PathBuf,
    policy: AiPolicy,
) -> AppResult<RunSubmit> {
    if form.context.selected_items.len() < form.minimum_inputs {
        return Err(AppError::unavailable(format!(
            "此操作至少需要在 Finder 选择 {} 项",
            form.minimum_inputs
        )));
    }
    let mut values = BTreeMap::new();
    for field in &form.fields {
        let value = parameters.get(&field.key).unwrap_or(&field.default_value);
        if field.required && value.trim().is_empty() {
            return Err(AppError::unavailable(format!("请填写{}", field.label)));
        }
        if !field.choices.is_empty() && !field.choices.contains(value) {
            return Err(AppError::unavailable(format!(
                "{}不是有效选项",
                field.label
            )));
        }
        if field.kind == "number" && !value.trim().is_empty() {
            let parsed = value
                .parse::<f64>()
                .map_err(|_| AppError::unavailable(format!("{}需要有效数值", field.label)))?;
            if !parsed.is_finite() || parsed < 0.0 || parsed > 1_000_000.0 {
                return Err(AppError::unavailable(format!("{}超出范围", field.label)));
            }
        }
        values.insert(field.key.clone(), value.clone());
    }
    if parameters
        .keys()
        .any(|key| !form.fields.iter().any(|field| &field.key == key))
    {
        return Err(AppError::unavailable("提交包含未登记的参数"));
    }
    if !form.context.selection_complete {
        return Err(AppError::unavailable("选区不完整，请缩小范围后重新选择"));
    }
    let inputs = if matches!(
        form.capability_id.as_str(),
        "CAP-FILE-001"
            | "CAP-FILE-002"
            | "CAP-TEXT-002"
            | "CAP-TEXT-004"
            | "CAP-TEXT-005"
            | "CAP-FILE-012"
    ) {
        vec![]
    } else {
        form.context
            .selected_items
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
    };
    let grouped = extended_descriptors()
        .iter()
        .any(|spec| spec.id == form.capability_id && spec.grouped_inputs)
        || matches!(
            form.capability_id.as_str(),
            "CAP-FILE-006"
                | "CAP-FILE-007"
                | "CAP-ZIP-001"
                | "CAP-PDF-001"
                | "CAP-IMAGE-012"
                | "CAP-IMAGE-015"
                | "CAP-TEXT-007"
                | "CAP-TEXT-008"
        );
    let groups = if inputs.is_empty() || grouped {
        vec![inputs.clone()]
    } else {
        inputs.iter().map(|id| vec![id.clone()]).collect()
    };
    let args = serde_json::to_string(&values).map_err(|e| AppError::internal(e.to_string()))?;
    let steps = groups
        .into_iter()
        .map(|input_refs| ExecutionStep {
            kind: StepKind::Native,
            operation: form.capability_id.clone(),
            executable_ref: None,
            script: None,
            args: vec![args.clone()],
            cwd_ref: None,
            env_refs: vec![],
            input_refs,
            expected_outputs: vec![],
        })
        .collect();
    let mut effects = vec![Effect {
        kind: if form.changes_files || parameters.get("output").is_some_and(|mode| mode == "file") {
            EffectKind::Modify
        } else {
            EffectKind::Read
        },
        source_ref: inputs.first().cloned(),
        destination_ref: form.context.directory_ref.as_ref().map(|p| p.id.clone()),
        explanation: if form.changes_files {
            format!(
                "按以上参数修改或生成文件。输入：{}",
                if form.context.selected_items.is_empty() {
                    "无文件输入".into()
                } else {
                    form.context
                        .selected_items
                        .iter()
                        .map(|input| input.display_path.clone())
                        .collect::<Vec<_>>()
                        .join("；")
                }
            )
        } else {
            "读取所选文件，不修改原件".into()
        },
    }];
    if supports_conversion_cleanup(&form.capability_id)
        && values
            .get("sourceHandling")
            .is_some_and(|value| value == "trashAfterSuccess")
    {
        effects.extend(inputs.iter().map(|id| Effect {
            kind: EffectKind::Delete,
            source_ref: Some(id.clone()),
            destination_ref: None,
            explanation:
                "仅在对应新文件生成并校验成功后，将原文件移入回收站；失败或取消保留原文件。".into(),
        }));
    }
    Ok(RunSubmit {
        working_directory: directory,
        session_id,
        prompt: format!("{} · {} 项输入", form.capability_id, inputs.len()),
        policy,
        plan: ExecutionPlan {
            id: format!("plan-{}", form.capability_id),
            revision: Revision::new(1),
            capability_id: Some(form.capability_id.clone()),
            context_id: form.context.id.clone(),
            steps,
            required_tools: vec![],
            effects,
            preview_completeness: PlanPreviewCompleteness::Complete,
            source_fingerprint: crate::fingerprint::fingerprint(&(parameters, &form.context)),
        },
    })
}

/// Bind output-producing steps to the persisted file preference using native PathRefs.
/// Grouped outputs require one explicit destination, so besideSource uses the captured cwd.
pub fn bind_output_locations(
    submit: &mut RunSubmit,
    location: &fleqi_domain::settings::OutputLocation,
    paths: &crate::paths::PathRegistry,
) -> AppResult<()> {
    use fleqi_domain::{context::PathKind, settings::OutputLocation};
    let id = submit.plan.capability_id.as_deref().unwrap_or("");
    let generates = writes_output_files(id);
    if !generates {
        return Ok(());
    }
    for step in &mut submit.plan.steps {
        if matches!(id, "CAP-TEXT-006" | "CAP-TEXT-011") {
            let params: BTreeMap<String, String> = serde_json::from_str(&step.args[0])
                .map_err(|e| AppError::internal(e.to_string()))?;
            if params.get("output").is_none_or(|value| value != "file") {
                continue;
            }
        }
        let target = match location {
            OutputLocation::Directory { display_path } => {
                let path = PathBuf::from(display_path);
                if !path.is_absolute() {
                    return Err(AppError::unavailable("输出目录必须为绝对路径"));
                }
                path
            }
            OutputLocation::BesideSource if step.input_refs.len() == 1 => {
                let path = paths
                    .resolve(&step.input_refs[0])
                    .ok_or_else(|| AppError::not_found("输入引用已失效"))?;
                if path.is_dir() {
                    path
                } else {
                    path.parent()
                        .ok_or_else(|| AppError::unavailable("输入没有父目录"))?
                        .to_owned()
                }
            }
            OutputLocation::BesideSource => submit.working_directory.clone(),
        };
        if !target.is_dir() {
            return Err(AppError::unavailable(
                "输出目录不存在，请在文件设置中重新选择",
            ));
        }
        let reference = paths.register(&target, PathKind::Directory);
        step.cwd_ref = Some(reference.id);
        step.expected_outputs = vec![format!("输出目录：{}", reference.display_path)];
        if id == "CAP-IMAGE-002" {
            let mut params: BTreeMap<String, String> = serde_json::from_str(&step.args[0])
                .map_err(|e| AppError::internal(e.to_string()))?;
            params.insert(
                "_besideSource".into(),
                matches!(location, OutputLocation::BesideSource).to_string(),
            );
            step.args[0] =
                serde_json::to_string(&params).map_err(|e| AppError::internal(e.to_string()))?;
            if matches!(location, OutputLocation::BesideSource) {
                step.expected_outputs
                    .push("目录批处理：每个输入旁生成，子目录范围以参数为准".into());
            }
        }
    }
    Ok(())
}

pub fn writes_output_files(id: &str) -> bool {
    matches!(
        id,
        "CAP-FILE-001"
            | "CAP-TEXT-002"
            | "CAP-TEXT-004"
            | "CAP-TEXT-005"
            | "CAP-ZIP-001"
            | "CAP-IMAGE-001"
            | "CAP-IMAGE-002"
            | "CAP-IMAGE-003"
            | "CAP-IMAGE-004"
            | "CAP-IMAGE-007"
            | "CAP-IMAGE-008"
            | "CAP-IMAGE-009"
            | "CAP-IMAGE-010"
            | "CAP-IMAGE-011"
            | "CAP-IMAGE-012"
            | "CAP-IMAGE-013"
            | "CAP-IMAGE-014"
            | "CAP-IMAGE-015"
            | "CAP-IMAGE-016"
            | "CAP-IMAGE-017"
            | "CAP-MEDIA-001"
            | "CAP-MEDIA-002"
            | "CAP-MEDIA-003"
            | "CAP-MEDIA-004"
            | "CAP-MEDIA-009"
            | "CAP-MEDIA-010"
            | "CAP-MEDIA-011"
            | "CAP-MEDIA-012"
            | "CAP-MEDIA-013"
            | "CAP-PDF-001"
            | "CAP-PDF-002"
            | "CAP-PDF-003"
            | "CAP-PDF-004"
            | "CAP-PDF-005"
            | "CAP-PDF-009"
            | "CAP-PDF-010"
            | "CAP-PDF-011"
            | "CAP-PDF-012"
            | "CAP-TEXT-006"
            | "CAP-TEXT-011"
    )
}

pub fn bind_output_settings(
    submit: &mut RunSubmit,
    settings: &fleqi_domain::settings::Settings,
    paths: &crate::paths::PathRegistry,
) -> AppResult<()> {
    bind_output_locations(submit, &settings.output_location, paths)?;
    let mut replaces = false;
    for step in &mut submit.plan.steps {
        if !writes_output_files(&step.operation) {
            continue;
        }
        let mut params: BTreeMap<String, String> =
            serde_json::from_str(&step.args[0]).map_err(|e| AppError::internal(e.to_string()))?;
        if matches!(step.operation.as_str(), "CAP-TEXT-006" | "CAP-TEXT-011")
            && params.get("output").is_none_or(|value| value != "file")
        {
            continue;
        }
        let overwrite = settings.name_conflict == fleqi_domain::settings::NameConflict::Overwrite;
        params.insert(
            "_nameConflict".into(),
            if overwrite { "overwrite" } else { "uniqueName" }.into(),
        );
        step.args[0] =
            serde_json::to_string(&params).map_err(|e| AppError::internal(e.to_string()))?;
        replaces |= overwrite;
    }
    if replaces {
        submit.plan.effects.push(Effect { kind: EffectKind::Modify, source_ref: None, destination_ref: None, explanation: "同名文件处理：新文件成功生成后替换输出目录中的同名文件；原始输入文件与同名文件夹保留。".into() });
        submit.plan.preview_completeness = PlanPreviewCompleteness::Unknown;
    }
    Ok(())
}
