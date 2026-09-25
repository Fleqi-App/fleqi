//! 设置领域模型：需求 §2.1 设计默认值是唯一来源；补丁按阶段字段允许表校验。

use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

/// 首版固定限制（需求 §2.1），不开放编辑，UI 以说明呈现。
pub mod limits {
    pub const AI_CONCURRENCY: u32 = 4;
    pub const ACTIVE_SESSIONS: u32 = 16;
    pub const SELECTION_ITEMS: usize = 1000;
    pub const INPUT_HISTORY: u32 = 200;
    pub const HISTORY_RETENTION_DAYS: u32 = 30;
    pub const RUN_OUTPUT_BYTES: u64 = 50 * 1024 * 1024;
    pub const SESSION_OUTPUT_BYTES: u64 = 100 * 1024 * 1024;
    pub const SCROLLBACK_LINES: u32 = 10_000;
}

macro_rules! contract_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[ts(export_to = "packages/contracts/src/bindings/")]
        #[serde(rename_all = "camelCase")]
        pub enum $name { $($variant),+ }
    };
}

contract_enum!(/// 唤起模式：manual 默认；followFinder 独立于快捷键。
    Activation { Manual, FollowFinder });
contract_enum!(/// 主动隐藏策略；系统临时隐藏不适用。
    HideBehavior { KeepAll, EndAll });
contract_enum!(/// 仅两种 AI 策略，只约束 AI；手动终端直通。
    AiPolicy { Yolo, ReadOnlyAutoConfirmChanges });
contract_enum!(Theme {
    Dark,
    Light,
    System
});
contract_enum!(/// 不能用应用设置强制忽略系统减少动态。
    MotionMode { System, Reduce });
contract_enum!(NameConflict {
    UniqueName,
    Overwrite
});
/// Conversion cleanup only runs after a verified new output exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ConversionSourceHandling {
    #[default]
    Keep,
    TrashAfterSuccess,
}
contract_enum!(HotkeyModifier {
    Command,
    Option,
    Control,
    Shift
});
contract_enum!(ProviderProtocol { OpenAiCompatible });

/// 有效快捷键：只有宿主注册成功的候选才落到设置。`key` 为稳定键标识（W3C code）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub key: String,
    pub modifiers: Vec<HotkeyModifier>,
}

/// 用户配置的模型端点；密钥只经 credentialRef 指向系统凭据，不在此结构中。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub display_name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub summary_model: Option<String>,
    pub timeout_ms: u32,
    pub credential_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SummaryModel {
    /// 跟随 defaultModel。
    Default,
    Model {
        model: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum OutputLocation {
    /// 默认在源文件所在目录生成新文件。
    BesideSource,
    Directory {
        display_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub bar_enabled: bool,
    pub activation: Activation,
    pub hotkey: Option<Hotkey>,
    pub hide_behavior: HideBehavior,
    pub ai_policy: AiPolicy,
    pub launch_at_login: bool,
    pub theme: Theme,
    /// 1–30 秒；None 表示常驻。
    pub bubble_seconds: Option<f64>,
    pub inline_suggestions_enabled: bool,
    /// 1–5。
    pub inline_suggestions_limit: u8,
    pub transparency: bool,
    pub motion_mode: MotionMode,
    pub providers: Vec<ProviderConfig>,
    pub default_model: Option<String>,
    pub summary_model: SummaryModel,
    /// 11–20。
    pub terminal_font_size: u8,
    pub output_location: OutputLocation,
    pub name_conflict: NameConflict,
    #[serde(default)]
    pub conversion_source_handling: ConversionSourceHandling,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bar_enabled: true,
            activation: Activation::Manual,
            hotkey: None,
            hide_behavior: HideBehavior::KeepAll,
            ai_policy: AiPolicy::ReadOnlyAutoConfirmChanges,
            launch_at_login: false,
            theme: Theme::Dark,
            bubble_seconds: Some(4.8),
            inline_suggestions_enabled: true,
            inline_suggestions_limit: 3,
            transparency: true,
            motion_mode: MotionMode::System,
            providers: Vec::new(),
            default_model: None,
            summary_model: SummaryModel::Default,
            terminal_font_size: 13,
            output_location: OutputLocation::BesideSource,
            name_conflict: NameConflict::UniqueName,
            conversion_source_handling: ConversionSourceHandling::Keep,
        }
    }
}

/// 缺省 = 未提供；`Some(None)` = 显式 null（清除）。
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// 设置补丁：每个字段可缺省；可空字段区分缺省与显式 null。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bar_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub activation: Option<Activation>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "double_option"
    )]
    #[ts(optional = nullable)]
    pub hotkey: Option<Option<Hotkey>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hide_behavior: Option<HideBehavior>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ai_policy: Option<AiPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub launch_at_login: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub theme: Option<Theme>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "double_option"
    )]
    #[ts(optional = nullable)]
    pub bubble_seconds: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub inline_suggestions_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub inline_suggestions_limit: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub transparency: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub motion_mode: Option<MotionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub providers: Option<Vec<ProviderConfig>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "double_option"
    )]
    #[ts(optional = nullable)]
    pub default_model: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary_model: Option<SummaryModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub terminal_font_size: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub output_location: Option<OutputLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub conversion_source_handling: Option<ConversionSourceHandling>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name_conflict: Option<NameConflict>,
}

/// 字段级错误：`code` 为 `invalid`（越界/非法）或 `notAvailable`（当前阶段未开放）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    pub field: String,
    pub code: String,
    pub message: String,
}

impl FieldError {
    fn invalid(field: &str, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code: "invalid".into(),
            message: message.into(),
        }
    }

    fn not_available(field: &str, stage: &str) -> Self {
        Self {
            field: field.into(),
            code: "notAvailable".into(),
            message: format!("该设置由 {stage} 交付，当前阶段不可修改"),
        }
    }
}

/// 各阶段允许修改的字段集合（architecture.md §12.3：M1 仅 theme/transparency/motionMode）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldAllowlist {
    M1,
    M2,
    All,
}

impl FieldAllowlist {
    /// 返回字段是否开放；未开放时给出交付阶段。
    fn permits(self, field: &str) -> Result<(), &'static str> {
        match self {
            FieldAllowlist::All => Ok(()),
            FieldAllowlist::M2 => match field {
                "theme"
                | "transparency"
                | "motionMode"
                | "barEnabled"
                | "activation"
                | "hotkey"
                | "hideBehavior"
                | "launchAtLogin"
                | "bubbleSeconds"
                | "inlineSuggestionsEnabled"
                | "inlineSuggestionsLimit" => Ok(()),
                _ => Err("M3"),
            },
            FieldAllowlist::M1 => match field {
                "theme" | "transparency" | "motionMode" => Ok(()),
                "barEnabled"
                | "activation"
                | "hotkey"
                | "hideBehavior"
                | "launchAtLogin"
                | "bubbleSeconds"
                | "inlineSuggestionsEnabled"
                | "inlineSuggestionsLimit" => Err("M2"),
                _ => Err("M3"),
            },
        }
    }
}

impl SettingsPatch {
    pub fn is_empty(&self) -> bool {
        self.touched_fields().is_empty()
    }

    /// 补丁涉及的 camelCase 字段名，按声明顺序。
    pub fn touched_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        macro_rules! touched { ($($member:ident => $name:literal),+ $(,)?) => { $( if self.$member.is_some() { fields.push($name); } )+ }; }
        touched!(
            bar_enabled => "barEnabled", activation => "activation", hotkey => "hotkey", hide_behavior => "hideBehavior",
            ai_policy => "aiPolicy", launch_at_login => "launchAtLogin", theme => "theme", bubble_seconds => "bubbleSeconds",
            inline_suggestions_enabled => "inlineSuggestionsEnabled", inline_suggestions_limit => "inlineSuggestionsLimit",
            transparency => "transparency", motion_mode => "motionMode", providers => "providers", default_model => "defaultModel",
            summary_model => "summaryModel", terminal_font_size => "terminalFontSize", output_location => "outputLocation",
            name_conflict => "nameConflict",
            conversion_source_handling => "conversionSourceHandling",
        );
        fields
    }
}

impl Settings {
    /// 应用补丁：先按阶段允许表拒绝未开放字段，再逐字段校验范围；任一错误则整体不应用。
    pub fn apply_patch(
        &self,
        patch: &SettingsPatch,
        allowlist: &FieldAllowlist,
    ) -> Result<Settings, Vec<FieldError>> {
        let mut errors = Vec::new();
        for field in patch.touched_fields() {
            if let Err(stage) = allowlist.permits(field) {
                errors.push(FieldError::not_available(field, stage));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut next = self.clone();
        if let Some(v) = patch.bar_enabled {
            next.bar_enabled = v;
        }
        if let Some(v) = patch.activation {
            next.activation = v;
        }
        if let Some(v) = &patch.hotkey {
            if let Some(hotkey) = v
                && hotkey.key.trim().is_empty()
            {
                errors.push(FieldError::invalid("hotkey", "键标识不能为空"));
            }
            next.hotkey = v.clone();
        }
        if let Some(v) = patch.hide_behavior {
            next.hide_behavior = v;
        }
        if let Some(v) = patch.ai_policy {
            next.ai_policy = v;
        }
        if let Some(v) = patch.launch_at_login {
            next.launch_at_login = v;
        }
        if let Some(v) = patch.theme {
            next.theme = v;
        }
        if let Some(v) = patch.bubble_seconds {
            if let Some(seconds) = v
                && !(1.0..=30.0).contains(&seconds)
            {
                errors.push(FieldError::invalid(
                    "bubbleSeconds",
                    "允许 1–30 秒或 null（常驻）",
                ));
            }
            next.bubble_seconds = v;
        }
        if let Some(v) = patch.inline_suggestions_enabled {
            next.inline_suggestions_enabled = v;
        }
        if let Some(v) = patch.inline_suggestions_limit {
            if !(1..=5).contains(&v) {
                errors.push(FieldError::invalid("inlineSuggestionsLimit", "允许 1–5"));
            }
            next.inline_suggestions_limit = v;
        }
        if let Some(v) = patch.transparency {
            next.transparency = v;
        }
        if let Some(v) = patch.motion_mode {
            next.motion_mode = v;
        }
        if let Some(v) = &patch.providers {
            next.providers = v.clone();
        }
        if let Some(v) = &patch.default_model {
            next.default_model = v.clone();
        }
        if let Some(v) = &patch.summary_model {
            next.summary_model = v.clone();
        }
        if let Some(v) = patch.terminal_font_size {
            if !(11..=20).contains(&v) {
                errors.push(FieldError::invalid("terminalFontSize", "允许 11–20"));
            }
            next.terminal_font_size = v;
        }
        if let Some(v) = &patch.output_location {
            next.output_location = v.clone();
        }
        if let Some(v) = patch.name_conflict {
            next.name_conflict = v;
        }
        if let Some(v) = patch.conversion_source_handling {
            next.conversion_source_handling = v;
        }

        if errors.is_empty() {
            Ok(next)
        } else {
            Err(errors)
        }
    }
}
