# Fleqi 完成度分析与收尾开发计划

## 一、当前完成度分析（HEAD 11458b7，工作树干净）

| 里程碑 | 状态 | 剩余缺口 |
|---|---|---|
| P0 工程 / M1 宿主 | **完成** | — |
| M2 入口/会话/终端 | 接近收口 | 仅 Finder 窗口移动/几何观察（拖动暂隐/恢复未接线；`system_hide/system_restore` 服务端已就绪但无调用方） |
| M3 AI/工具/能力 | 后端+UI 完成 | 受管工具清单无注入通道（`register_manifests` 无人调用→安装正路径断裂）；安装/规划无进度与取消 UI（FR-TOOLS-002 硬要求）；模型页无法更新已有端点、无"清除密钥"、顶层 defaultModel 无 UI；目录仅含 30 项基础能力；135 AC 矩阵未跑 |
| M4 界面与交互验收 | 进行中 | AC-FLOW 002/003/004/006/007/008/010/011/014/015 已原生通过；**001/005/012/013 未跑**（009 也缺显式编号断言）；验证记录未成册 |
| M5 发布与验收 | 部分进行 | 包构建+verify:package 通过；签名/公证/更新验签（无 Apple 凭据）、Intel 工具链、性能 p95、安装/升级、发布材料未做 |

**首次使用的用户视角硬缺口**：composer 三种提示（无热键/无端点/无会话）全是纯文本、无"打开设置"跳转；会话选择器无"新建会话"（`session_create` 后端在位但 UI 未消费）；控制台 Overview"首次配置"卡片的"设置唤起方式/配置模型 API"两步是死徽章（写着"M2/M3 交付"却不可点）；零端点空态无动作按钮。

## 二、执行阶段（每阶段独立 commit + 真实验证）

### 阶段 A：首次可用闭环（M4 UI 收尾，最优先）
1. composer 引导链路：无热键提示加"打开设置"、无端点提示加"配置模型 API"按钮（`app_open_window`，必要时扩展可选 page 参数并 `pnpm contracts:regen`）；符合 ui-design.md:224/425/429。
2. 会话选择器加"新建会话"入口（消费 `session_create` + `session_select`；preview 适配器同步）。
3. Overview 首次配置卡真实化：四步全部真实状态徽章（热键状态来自 settings snapshot、端点来自 provider_list）+ 前往按钮。
4. 模型页：卡片"编辑"载入已有端点、显式"清除密钥"（后端补最小支持，新命令走 5 点注册）、顶层默认模型绑定 `settings.defaultModel`。
5. 进度/取消（FR-TOOLS-002）：`tools_install_cancel` + 安装进度事件接到工具页；`run_plan_cancel`（gateway 已支持块间取消）接到 composer 规划中状态。
6. AC-FLOW-009（`!` 前缀）补显式编号断言进既有 spec。
7. vitest/Playwright 覆盖以上 UI；`pnpm check`、`pnpm test:ui`、`pnpm test:desktop` 全绿。

### 阶段 B：M2 收口 — Finder 窗口移动观察
- `crates/fleqi-platform/src/macos/observer.rs` 增加 Finder 前窗移动观察（AXObserver + AXManualAccessibility，不可行则有界轮询 `finder_window_bounds`）；`state.rs` 装配把"移动开始/结束"路由到 `surface.system_hide()/system_restore()`，复用现有 `surface:changed → hide_composer`。
- Rust 假事件测试 + 原生验证（AppleScript 改窗口 bounds 触发）；status.md 宣告 M2 完成。

### 阶段 C：AC-FLOW-001/005 原生运行（已获准自动驱动 Finder）
- 新串行 spec（并入 test-desktop run 定义）：**001** 全新数据目录默认 manual/keepAll/无热键 → osascript 激活 Finder 切目录断言不自动出现 → 托盘引导路径（System Events 点托盘项，不可脚本化则如实降级为 IPC 拒绝断言+手动清单）→ 设置 UI 真实注册热键 → 显式唤起新会话含有效目录。**005** 真实 vim：租约输入进插入模式敲文本 → Finder A(/tmp/a)→B→C 连续切换 → 断言零目录命令注入、文本完好 → 退出 vim 后同步 C。
- 全部 /tmp 临时目录，结束关闭临时 Finder 窗口；证据 JSON 落 tests/.artifacts/desktop/。

### 阶段 D：AC-FLOW-012 环路
- 补受管清单注入通道（按 AC-COMMON-007"受管目录确定"：宿主装配注册受管条目 url+sha256；原生验证走回环 HTTP——合同明确允许）。
- spec：catalog_query 发现 → tools_list 确认缺失 → tools_install（回环包+sha256，验证进度/取消）→ run_submit 执行依赖该工具的能力 → favorites_create → 新上下文复用（规则重求值）→ AI 摘要段按合同完成"无端点引导"验收；若执行环境经环境变量提供测试端点则真实跑（凭据只从环境变量读取）。

### 阶段 E：135 项能力 AC 全量矩阵（你已选全量）
- 按 docs/capabilities.md 逐行建矩阵测试（按类别分文件：文件/文本 → ZIP/图片 → 媒体/PDF → 扩展），每行真实 fixture + 条件缺失真实反馈（缺工具/权限/无设备/网络失败），补齐约 92 个未覆盖编号；进度区分确定/不确定（AC-COMMON-008）。
- 最后批次做需真实安装的条目（brew 装 Ghostscript、OCR/ASR 模型组合），安装前在会话里明示将要安装的软件；下载量大/网络失败的如实记录。
- 产出 135 项逐项结果表（docs/status.md 或独立核验文档）+ 证据。

### 阶段 F：记录成册 + M5 可做项
- M4 验收记录成册（截图/视频、焦点/IME、reduced-motion、透明度降级）。
- M5：性能 p95 计时证据（composer 显示/终端回显）、本地 .app 安装/升级冒烟、图标与关于页复核、发布材料；尝试 Intel 交叉工具链。
- **如实挂起**（无凭据，不宣称已发布）：Developer ID 签名+公证、更新验签公钥/发布地址。status.md 终版诚实结论。

## 三、纪律
- DTO 变更必 `pnpm contracts:regen`；新命令严格执行 5 点注册（commands.rs / main.rs / allowlist / capability / build.rs）+ check:boundaries。
- 每阶段跑 `pnpm check` + `pnpm test:ui` + 相关 cargo/原生测试后再 commit；原生 spec 并入 scripts/test-desktop.mjs。
- 凭据只从环境变量；测试文件只落 /tmp；Finder 驱动后恢复；不宣称项目安全；不把参考工程/mock 当 App 证据。