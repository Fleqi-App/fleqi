# M4/M5 验证记录成册（2026-09-19）

> 2026-09-20 复核：本文保留历史测试记录，不再代表 M4 整体产品验收通过。修复前问题见[项目核查](review-2026-09-20.md)，后续交付和最新证据见[实现与修复记录](implementation-2026-09-20.md)。


本文件汇总产品界面与交互验收（M4）与发布验收（M5）的证据索引、执行方式与已知边界。所有原生证据由 `pnpm test:desktop`（scripts/test-desktop.mjs）生成于 `tests/.artifacts/desktop/`，UI 单测/组件证据由 `pnpm test:ui` 生成。证据文件为运行产物（.gitignore 忽略），本文记录其文件名与核对方式；重新运行即可再生。

## Windows 11 x64 核心修复验证

2026-10-06–07：本轮开放范围为能力台账的 21 项基础能力，完整 Windows 首版不在本轮通过口径中。

| 层次 | 执行入口 | 核对内容与证据 |
|---|---|---|
| 共享业务 | `cargo test --locked -p fleqi-domain -p fleqi-application` | 状态机、策略、上下文、会话/任务、持久化接口及旧脚本解释器兼容；Windows 自由脚本先确认，不把原 POSIX 白名单当成 Windows 证明 |
| Windows 平台 | `cargo test --locked -p fleqi-platform --test windows_platform` | 真实 Credential Manager 隔离命名空间增删改查，测试项清理，不使用生产凭据键 |
| Windows 执行/文件 | `cargo test --locked -p fleqi-adapters --test windows_core -- --test-threads=1` | 真实 PowerShell、ConPTY 光标握手、编辑行/忙碌保护、中文目录与实际文件、exit 回收、Job 子进程取消、文件/ZIP/图片/PDF、系统回收站往返；本机 C:→E: 跨卷、占用文件保留原件、中文长路径、目录 ZIP 往返及 junction 越界拒绝 |
| 同名输出 | `cargo test --locked -p fleqi-adapters --test name_conflict` | 经生产 NativeSteps 验证同名覆盖、保留原件和失败/取消不截断已有文件 |
| 前端 | `pnpm --filter @fleqi/ui run test:unit`、`pnpm typecheck` | 现有界面行为及类型；Windows 界面由原生用例补充，不将浏览器 preview 作为宿主证据 |
| 桌面 | `pnpm run test:desktop` | WebView2 + embedded WebDriver + 真实 IPC；独立数据目录、自建 Explorer 窗口及双标签页、原生目录选择/取消、回环模型 HTTP、文件结果、输入条跟随与重启恢复；证据 `windows-*.json/png` |
| 普通安装包 | `pnpm run verify:package` | 普通 release 包的 PE、载荷哈希、安装/覆盖、真实 IPC 就绪、正常退出和卸载；证据 `tests/.artifacts/package/windows-package-evidence.json` |

`windows-follow-condition.json` 单独记录 Explorer 前台条件。驱动无法把自建窗口置于前台时，跟随/移动/目录同步用例记为未验证，不计通过，也不阻止其它独立功能验证。测试窗口只操作 `windows-data-*/fixtures/`，不关闭用户窗口；模型服务仅验证 HTTP/规划/审批/执行链路，不代表外部模型质量验收。

最终结果：共享业务 105 项、Windows 凭据 2 项、Windows 核心 8 项、输出名称/冲突 4 项、宿主单测 5 项，共 124 项 Rust 测试通过；UI 47 项、WebView2 原生 10 项通过。最终 `windows-follow-condition.json` 的 `verified=true`，无跳过项。原生目录选择器分别核对真实中文目录返回值和取消后上下文不变。

额外尝试的 `cargo test -p fleqi-desktop` 中，既有 macOS 更新器集成测试 `tests/updater.rs` 在 Windows 加载阶段返回 `0xc0000139`，未计通过；Windows 自动更新已明确关闭，本轮没有改写或绕过该 macOS 发行测试。宿主本身的 5 项单测通过。

剩余人工/设备条件：100%/150% 与混合 DPI、多屏边界、原生 IME/读屏、托盘鼠标交互、UNC 网络共享和外部真实模型；本机已覆盖单屏 200% 缩放。macOS CI 已保留，本机没有运行 macOS 原生回归，新 Windows CI 尚未远程执行。条件项完成前不得声明完整 Windows 验收通过。

## 1. AC-FLOW 原生矩阵（真实 IPC + 真实 zsh + 真实 Finder）

| 验收 | 证据文件 | 覆盖内容 |
|---|---|---|
| AC-FLOW-001 | m10-fresh-defaults.json、m10-after-switch.json、m10-refused.json、m10-registered-shown.json | 全新数据目录默认 manual/keepAll/未绑定；真实切换 Finder 不自动出现；未注册时显式显示被拒并引导快捷键；注册后显式唤起新会话含有效目录 |
| AC-FLOW-002 | m4-flow-002.json | followFinder 自动出现、主动隐藏抑制、模式切换/显式唤起解除 |
| AC-FLOW-003/004 | m4-flows-evidence.json、m4-flow-004.json | keepAll 隐藏保留会话并可切回；endAll（经真实设置 UI）隐藏时按 ID 逐会话落 ended（轮询等待异步 PTY 关闭） |
| AC-FLOW-005 | m11-vim-submitted.json、m11-busy-during-switches.json、m11-final-sync.json | 真实 vim 占用前台时切换 Finder B→C：零 `__cd__` 注入、原始键插入文本到达 vim 缓冲、退出后同步到 C |
| AC-FLOW-006/014 | m4-flow-006.json | 忙碌终端排队命令等待目录同步；取消恢复草稿不执行 |
| AC-FLOW-007/008 | m4-flows-evidence.json | Run 上下文独立；yolo（经设置 UI）未知效果免确认执行 |
| AC-FLOW-009 | m2-ac-flow-009.json | `!` 标记命令经宿主去标记后由真实 zsh 执行，输出可见；接受 sent/queued 两条投递路径 |
| AC-FLOW-010 | m4-flow-010.json | 非法热键候选被拒且旧绑定保留；清除后未绑定 |
| AC-FLOW-011 | m4-flow-011*.json、m12-*（会话部分） | 图钉排序、单独结束不误伤、删除不删用户文件、历史继续关联新会话、选择器 UI 真实点击 |
| AC-FLOW-012 | m12-discovery/missing/installed/executed/favorite/reuse/ai-guidance.json | 目录发现 → 缺依赖 → 回环安装（SHA-256 校验+预检+原子发布）→ 真实执行安装物 → 收藏 → 新会话复用；无端点时 AI 摘要给真实引导/报错 |
| AC-FLOW-014 | （同 006 证据） | 取消排队恢复草稿 |
| AC-FLOW-015 | m4-flow-015.json | 忙碌退后台：撤销在途目录切换与排队命令、pendingDirectory 清空、程序继续、被撤销命令不执行 |

已修复的真实缺陷（均由上述验证暴露）：见 docs/status.md M4 核验表各行"发现并修复真实缺陷"。

## 2. 交互维度记录

- **焦点/键盘**：composer 输入条 Enter 提交、Shift+Enter 换行、IME 组合期（`isComposing`）不提交（vitest composer 套件 + 04/08 原生输入租约路径）。
- **IME**：终端面板经 xterm onData 序列化，组合中的输入不拆分下发（M2 UI 核验 + composer Playwright 基线）。
- **reduced-motion**：设置页 motionMode 真实保存并经 `applyAppearance` 生效（settings-general/App vitest、02-settings 原生）。
- **透明度降级**：transparency 关闭时窗口材质回退（外观设置保存 + 原生设置窗口同步断言）。
- **读屏**：控件均有可访问名称（按钮 aria-label、radiogroup、data-testid 辅助定位）；完整 VoiceOver 走查留人工清单（见 §4）。
- **窗口色（无边框/自绘红绿灯，ui-design.md §12.4）**：三窗口 `decorations=false` + 透明 + CSS 圆角；截图证据 `tests/.artifacts/desktop/m1-console-overview.png`（控制台三灯、无白条）与 `m2-composer-borderless.png`（输入条内联红点 + 历史按钮，参考视频样式）。控制台/设置顶部拖动带 36px；输入条无独立标题区——整条背景是拖动层（z-0，控件在其上），点空白拖动、点控件仍是控件。vitest `window-chrome` 套件断言每窗口拖动区唯一且不在滚动容器上（拖动/滚动互不抢占的回归守卫）、输入条红点走 `surfaceHide` 而非窗口 close；原生 spec 04 断言 composer 拖动区唯一。
- **贴附几何（ui-design.md §4.1"外侧下方 4、与 Finder 等宽、内侧贴底回退"）**：runner 自建 Finder 目标窗并把 bounds 写入标志文件；spec 04 经宿主 `core:window` 只读能力（outer_position/outer_size/scale_factor）取输入条外框逻辑坐标，与 Finder bounds 在 spec 内断言（x 对齐、y=下缘+4、等宽最小 560、高 72），证据 `tests/.artifacts/desktop/m2-attach-bounds.json`（实测 x 395/395、y 659=655+4、宽 920/920、高 72，四项全过）。WKWebView 不向页面暴露真实窗口几何（screenX/outerWidth 为 0），System Events 在频繁重启应用的机器上视图不稳定——故采用宿主 IPC 通道。
- **结果回返（§7 结果气泡）**：composer 订阅 `run:changed`，run 终态（成功/部分成功/失败）经 `run_get` 取真实记录后上短结论气泡（输出末行 + 退出码如实归纳，无摘要不编造）；气泡含复制真实输出、回应回填草稿、详情直达控制台任务页；计时按 `bubbleSeconds`，悬停/焦点暂停；主条不可见时只登记未读徽标不强行唤起。vitest 覆盖事件驱动上泡、复制、回应、隐藏期未读；真实模型端到端的气泡截图留人工清单（需用户端点）。
- **托盘菜单端到端**：菜单为原生 NSMenu，Webdriver 不可触达；其"被拒→引导设置"后端语义已由 AC-FLOW-001/04 原生断言，点击路径留人工验收（打开托盘 → 显示输入条 → 观察自动打开设置）。

## 3. 013 能力 AC 全量矩阵

- 基础 30 项（AC-CAP-001–030）：`cargo test -p fleqi-adapters --test capabilities_matrix_basic -- --nocapture`，**pass=29 场景（覆盖 30 项）fail=0 gap=0**；证据 `tests/.artifacts/ac-matrix/basic30.json`。
- 扩展 105 项（AC-CAP-031–135）：`cargo test -p fleqi-adapters --test capabilities_matrix_extended -- --nocapture`，**pass=39 / gap=50 / cond=16，fail=0**；证据 `tests/.artifacts/ac-matrix/extended.json`。
- verdict 口径：pass=真实通过；gap=合同要求的能力执行器尚不存在（诚实缺口，已逐项记录，是后续开发清单）；cond=需要特殊系统状态/外部服务/凭据，按"真实失败或引导反馈"验收（不伪成功），实现完整执行器后可升级为 pass。
- 矩阵过程中补齐的执行器：pdf_extract_pages / pdf_rotate_pages / pdf_compress、trash 恢复（restore_from_trash + restore_paths）。

## 4. 人工验收清单（自动化不可达项）

1. 托盘菜单点击"显示输入条"（manual 未注册时应打开设置；注册后应显示输入条）。
2. VoiceOver 走查：控制台/设置/输入条主要控件朗读名称与状态。
3. 真机窗口截图/录屏（需屏幕录制权限）：composer 贴附 Finder、设置四态保存、终端 xterm 渲染。
4. 无边框窗口手感复核（人工）：从窗口边缘拖拽缩放控制台/设置（无边框窗口的边缘缩放由系统按 resizable 掩码提供，自动化未覆盖）；拖动条拖移窗口；红绿灯悬停符号显示。
5. brew 安装/卸载（AC-CAP-130/131）：需真实网络与磁盘写入，按用户指令执行。
6. 模型端到端（AI 摘要/规划执行）：需用户自配端点与密钥（只从环境变量或密钥服务读取）。

## 5. M5 发布验收现状

- 已通过：`pnpm tauri build --bundles app` + `pnpm verify:package`（Info.plist、adhoc 签名校验、arm64、启动与干净退出；证据 package-evidence.json）。
- 发行环境阻塞（如实挂起，不宣称已发布）：Developer ID 签名与公证（无 APPLE_ID/APPLE_API_KEY）、更新验签（无公钥/发布地址）、Intel 原生记录（未装 x86_64 工具链）。
- 性能 p95 与安装/升级演练：待发行环境就绪后随 RC 一并执行（当前数据目录冷启动→宿主就绪 ≈30s 的 harness 数字不代表应用性能）。
