//! macOS 权限探测（architecture.md §12.4）：
//! - Finder 自动化：AEDeterminePermissionToAutomateTarget，目标仅 com.apple.finder；
//!   被动 askUserIfNeeded=false，显式 true；区分允许/需同意/拒绝/Finder 未运行/调用失败。
//! - 辅助功能：AXIsProcessTrustedWithOptions，仅显式申请带提示；false 只表示当前未受信任。
//!
//! 阻塞调用由应用层放到专用线程。

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use fleqi_application::ports::{PermissionPort, PermissionProbe};
use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};
use std::ffi::c_void;

const FINDER_BUNDLE_ID: &str = "com.apple.finder";
const TYPE_APPLICATION_BUNDLE_ID: u32 = 0x62756E64; // 'bund'
const TYPE_WILDCARD: u32 = 0x2A2A2A2A; // '****'
const NO_ERR: i32 = 0;
const PROC_NOT_FOUND: i32 = -600;
const ERR_AE_EVENT_NOT_PERMITTED: i32 = -1743;
const ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT: i32 = -1744;

#[repr(C)]
struct AEDesc {
    descriptor_type: u32,
    data_handle: *mut c_void,
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn AECreateDesc(
        type_code: u32,
        data_ptr: *const c_void,
        data_size: isize,
        result: *mut AEDesc,
    ) -> i16;
    fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
    fn AEDeterminePermissionToAutomateTarget(
        target: *const AEDesc,
        event_class: u32,
        event_id: u32,
        ask_user_if_needed: bool,
    ) -> i32;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef)
    -> bool;
    static kAXTrustedCheckOptionPrompt: core_foundation::string::CFStringRef;
}

/// 原始 OSStatus → 权限状态（可测试的纯映射）。
pub fn map_automation_status(status: i32) -> (PermissionStatus, Option<String>) {
    match status {
        NO_ERR => (PermissionStatus::Allowed, None),
        ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT => (PermissionStatus::NeedsConsent, None),
        ERR_AE_EVENT_NOT_PERMITTED => (PermissionStatus::Denied, None),
        PROC_NOT_FOUND => (PermissionStatus::TargetNotRunning, None),
        other => (
            PermissionStatus::Failed,
            Some(format!(
                "AEDeterminePermissionToAutomateTarget 返回 {other}"
            )),
        ),
    }
}

fn finder_automation(ask: bool) -> PermissionProbe {
    let bundle = FINDER_BUNDLE_ID.as_bytes();
    let mut desc = AEDesc {
        descriptor_type: 0,
        data_handle: std::ptr::null_mut(),
    };
    // SAFETY: 传入有效缓冲区与输出描述符；成功后由 AEDisposeDesc 释放。
    let created = unsafe {
        AECreateDesc(
            TYPE_APPLICATION_BUNDLE_ID,
            bundle.as_ptr().cast(),
            bundle.len() as isize,
            &mut desc,
        )
    };
    if created != 0 {
        return PermissionProbe {
            status: PermissionStatus::Failed,
            error: Some(format!("AECreateDesc 失败：{created}")),
        };
    }
    // SAFETY: desc 已由 AECreateDesc 初始化。
    let status =
        unsafe { AEDeterminePermissionToAutomateTarget(&desc, TYPE_WILDCARD, TYPE_WILDCARD, ask) };
    // SAFETY: 释放上面创建的描述符。
    unsafe {
        AEDisposeDesc(&mut desc);
    }
    let (status, error) = map_automation_status(status);
    PermissionProbe { status, error }
}

fn accessibility(ask: bool) -> PermissionProbe {
    // SAFETY: kAXTrustedCheckOptionPrompt 是框架导出的常量字符串。
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let value = if ask {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
    // SAFETY: 字典在调用期间存活。
    let trusted = unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) };
    let status = if trusted {
        PermissionStatus::Allowed
    } else if ask {
        // 显式申请后仍未受信任：系统只提供设置入口，按拒绝处理并提供打开设置。
        PermissionStatus::Denied
    } else {
        // AX 不区分"未询问"与"已拒绝"；被动检查按需要显式申请处理。
        PermissionStatus::NeedsConsent
    };
    PermissionProbe {
        status,
        error: None,
    }
}

pub fn system_settings_url(permission: Permission) -> &'static str {
    match permission {
        Permission::FinderAutomation => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
        }
        Permission::Accessibility => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
    }
}

#[derive(Default)]
pub struct MacPermissions {
    reset_directory: Option<std::path::PathBuf>,
    reset_error: std::sync::Mutex<Option<String>>,
}

impl MacPermissions {
    /// Called before any permission/context checks on ordinary installed builds.
    /// Test/development hosts use Default and cannot reset the installed app.
    pub fn for_installed_build(data_dir: &std::path::Path) -> Self {
        let error = super::update_permissions::reset_for_installed_build(data_dir).err();
        Self {
            reset_directory: Some(data_dir.to_owned()),
            reset_error: std::sync::Mutex::new(error),
        }
    }

    pub fn reset_error(&self) -> Option<String> {
        self.reset_error.lock().expect("permission reset").clone()
    }
}

impl PermissionPort for MacPermissions {
    fn check(&self, permission: Permission, procedure: PermissionProcedure) -> PermissionProbe {
        let mut error = self.reset_error.lock().expect("permission reset");
        if error.is_some()
            && procedure == PermissionProcedure::Explicit
            && let Some(directory) = &self.reset_directory
        {
            *error = super::update_permissions::reset_for_installed_build(directory).err();
        }
        if let Some(message) = error.as_ref() {
            return PermissionProbe {
                status: PermissionStatus::Failed,
                error: Some(format!(
                    "更新后的权限重置未完成：{message}。点击显式申请可重试。"
                )),
            };
        }
        drop(error);
        let ask = procedure == PermissionProcedure::Explicit;
        match permission {
            Permission::FinderAutomation => finder_automation(ask),
            Permission::Accessibility => accessibility(ask),
        }
    }

    fn open_system_settings(&self, permission: Permission) -> Result<(), String> {
        // 明确 argv，不经 shell；URL 为固定常量。
        let status = std::process::Command::new("/usr/bin/open")
            .arg(system_settings_url(permission))
            .status()
            .map_err(|e| format!("无法打开系统设置：{e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "open 退出码 {status}；请手动打开 系统设置 → 隐私与安全性"
            ))
        }
    }
}
