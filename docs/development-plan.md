# Fleqi 开发顺序与验证计划

版本：1.4 · 日期：2026-09-16。当前状态见[实施状态](status.md)。

范围由[需求](requirements.md)和[能力台账](capabilities.md)确定；[UI](ui-design.md)和[架构](architecture.md)提供合同。本文件是唯一排期入口。先定稿文档，再复用当前 Git 文档基线，获准后建立新工程；后续验证只回填事实，产品行为变更先同步合同。

## 1. 里程碑与交付顺序

实际进度以[实施状态](status.md)为准（P0 于 2026-09-17、M1 于 2026-09-18 完成，M2 于 2026-09-18 进行中）；旧候选版代码和验收记录已归档，不计入新一轮完成度。保留产品范围和阶段出口条件，后续按 M2 → M3 → M4 → M5 实施。

本次仅回退到文档状态。用户明确开始下一轮实施后，再建立独立工程并逐阶段验证；不继承旧执行记录中的自动连续开发指令。

| 阶段 | 输入/依赖 | 必须交付 | 出口条件 |
|---|---|---|---|
| M0 · 文档基线 | 用户决定、原始参考与意图 | 需求、能力、UI、架构、计划及统一入口 | ID、引用和行为一致 |
| P0 · 工程准备 | M0、已批准实施计划 | 完整路线、状态/追踪、AGPL/构建/贡献说明、本地 Git、Rust/pnpm workspace、最小 App、生成类型和检查脚本 | 构建、真实只读 IPC、原生启动/退出与证据通过 |
| M1 · 宿主与权限底座 | P0 | 单实例、菜单窗口、设置/SQLite、权限、Finder、目录选择、Keychain、自检 UI | 真实持久化和跨窗口一致；拒绝/撤销/恢复、损坏数据降级可验证 |
| M2 · 入口、会话与终端 | M1 | 输入条、热键、自启动、显隐、会话、PTY、安全目录同步与屏幕恢复 | AC-FLOW-001–007、009–011、014–015；无孤儿进程 |
| M3 · AI、命令和工具 | M1、M2 的 Session/Context 合同 | 模型、执行/策略、工具、全部能力、规则收藏和历史 | AC-FLOW-008、012、013 及 AC-CAP-001–135 真实通过 |
| M4 · 完整界面与异常闭环 | M2、M3 | 输入条/浮层、控制台、七类设置、主题、错误态和无障碍 | 每个入口有真实动作；视觉、键盘、IME、reduced-motion 通过 |
| M5 · macOS 发布 | M1–M4 | 双架构回归、性能、升级、签名/公证、更新验签、安装包和源码材料 | 全部首版 AC 与实际制品、发行配置齐备 |

新工程使用架构约定的独立目录。`Web APP/` 保留为参考，其 localStorage、旧 DTO、账号占位和测试不作为新业务。文件转写、字幕、OCR 均在 M3，实时语音与 Windows/Linux 产品在后续版本。

## 2. 工作包与验收门槛

### P0 · 待开始的工程准备

| 顺序/验收 ID | 交付内容 | 检查与完成条件 |
|---|---|---|
| 1 · P0-DOC-001 | 同步六份合同及项目入口，写明全程依赖、M1 合同、状态和追踪；补 AGPL、第三方说明、构建与贡献指南 | `pnpm check:docs`、`pnpm check:traceability`；135 能力 AC、105 legacy_id、15 FLOW 全覆盖 |
| 2 · P0-REPO-001 | 复用当前 Git 文档基线、忽略规则、参考资产哈希清单 | 正确仓库根与文档基线；视频、缓存和工具记录被忽略；参考源哈希不变 |
| 3 · P0-ENV-001 | Rust 2024、Tauri 2、React/TS/Vite、pnpm workspace、锁文件与环境检查 | Rust 1.98.0、Node 26.8.1、pnpm 10.33.4；`pnpm run doctor`、`pnpm check:rust` 与 frozen/locked 安装通过 |
| 4 · P0-CONTRACT-001 | Rust BuildInfo/AppError、只读 app_build_info、显式 ts-rs 导出 | `pnpm check:contracts` 无差异；普通测试不改生成文件 |
| 5 · P0-BOUNDARY-001 | crate 依赖、ViewModel/适配器、窗口 ACL、CSP、导航边界 | `pnpm check:boundaries`；不注册未实现业务命令，测试驱动仅显式测试构建启用 |
| 6 · P0-UI-001 | 单窗口工程状态页、加载/成功/失败/重试、独立预览标识、视觉 token | `pnpm typecheck`、`pnpm build`、`pnpm test:ui`；深浅外观、键盘和减少动态基本可用 |
| 7 · P0-DESKTOP-001 | 加载打包资源的原生测试构建，真实只读 IPC | `pnpm test:desktop`；WKWebView 展示真实版本/平台，截图及启动/退出证据 |
| 8 · P0-PACKAGE-001 | macOS 14 最低版本、app.fleqi.desktop、开发签名 App | `pnpm tauri build --bundles app`；核对 Info.plist、签名、架构与普通包启动；无测试驱动 |

P0 只开放工程状态窗口。菜单栏常驻、关窗口保留宿主、设置保存和授权交互由 M1 交付。P0 启动不创建数据库、shell 或模型连接，不申请系统权限。初始项目版本为 **0.0.1 BETA1**，技术形式（Cargo/npm 等需要 semver 的位置）写作 `0.0.1-beta.1`，阶段标识仍为 P0。版本号不表示已有正式制品：P0 与 M1–M5 的实现与验收进度不因它改变，后续阶段沿用该版本直到新的发布版本决定。该版本与 `Web APP/` 参考工程自身 package.json 的版本无关。macOS 制品按平台约束拆字段：`CFBundleShortVersionString` 只放数字形式的 `0.0.1`，beta 标识由构建号与 BuildInfo 的 `stage` 承载，不写进只接受数字段的字段。

参考视频按用户决定仅本地保留；[资产清单](reference-assets.json)登记路径、大小与 SHA-256。缺少该视频不阻塞构建；后续视频对照验收仍需真实参考。当前 Git 仓库和远程保留；新工程的提交以本次文档基线为起点。

#### P0 前置条件

开始 P0 前先满足下表。工具链按归档工程已验证的组合锁定，不以“本机可用版本”代替。

| 前置 | 要求与原因 |
|---|---|
| Rust 工具链 | 安装 rustup，并以 `rust-toolchain.toml` 锁定 1.98.0 与 rustfmt、clippy。Homebrew 单体 rustc 不读取该文件，且只有 `aarch64-apple-darwin` 标准库，无法满足 M5 的 Intel 目标。 |
| Node 与 pnpm | Node 26.8.1；根 package.json 用 `packageManager: pnpm@10.33.4` 与同值 `engines` 固定包管理器，首次安装经 corepack 获取该版本，不让他版 pnpm 改写锁文件。 |
| Xcode 与 SDK | 命令行工具与 macOS SDK 可用；App 部署目标 macOS 14。 |
| 构建空间 | 预留多目标 Rust 与 Tauri 构建空间；性能记录按最低 8GB Apple Silicon 基线登记实际设备。 |
| 网络 | 首次依赖安装需要 npm registry 与 crates.io；联网取得的内容不记作本地已验证。 |

#### P0 执行细化

按上表 ID 顺序执行；每个工作包的最小步骤与证据如下。

| ID | 执行步骤 | 产出与证据 |
|---|---|---|
| P0-DOC-001 | 核对六份合同互链、ID 与枚举一致性；建立输入为当前六份合同的 `pnpm check:docs` 与 `pnpm check:traceability` | 两项检查可用；135 个 AC-CAP、105 个 legacy_id、15 个 AC-FLOW 全覆盖；AGPL、第三方、构建与贡献说明就位 |
| P0-REPO-001 | 校正忽略边界（视频、依赖、构建产物、缓存、工具记录）；`pnpm check:references` 校验资产清单；`pnpm check:repo` 校验仓库根与远程 | 参考资产哈希全部一致；待决漂移项处理后门禁才通过 |
| P0-ENV-001 | 建立根 workspace（package.json、pnpm-workspace.yaml、Cargo.toml、rust-toolchain.toml）；运行 `pnpm run doctor`、`pnpm check:rust`、frozen/locked 安装 | 工具链与依赖锁定可复现；doctor 报告版本、组件与 Xcode |
| P0-CONTRACT-001 | BuildInfo/AppError DTO 落在 application，serde camelCase，ts-rs 显式导出；生成与校验分离 | `pnpm check:contracts` 临时目录比对无差异；普通测试不改生成文件 |
| P0-BOUNDARY-001 | 建立 crate 依赖方向、ViewModel/适配器边界、窗口 ACL/capability、CSP 与导航限制 | `pnpm check:boundaries` 通过；宿主只注册 `app_build_info`；测试驱动仅显式测试构建启用 |
| P0-UI-001 | 单窗口工程状态页、加载/成功/失败/重试、独立预览标识与视觉 token | `pnpm typecheck`、`pnpm build`、`pnpm test:ui`；深浅外观、键盘与减少动态基本可用 |
| P0-DESKTOP-001 | 原生测试构建加载打包本地 UI，走真实只读 IPC | `pnpm test:desktop`；真实版本/平台截图与启动、退出证据 |
| P0-PACKAGE-001 | `pnpm tauri build --bundles app`，macOS 14 最低版本、`app.fleqi.desktop`、开发签名 | 核对 Info.plist、签名与架构；普通包启动证据，无测试驱动 |

#### P0 开始前需要确认的事项

| 事项 | 现状与影响 |
|---|---|
| 参考资产漂移 | 2026-09-16 复核显示 `Icon/macos/Fleqi.icon/icon.json` 的工作区内容、HEAD 与[资产清单](reference-assets.json)一致；此前记录的漂移当前未重现。P0-REPO-001 仍需在实施时自动核验清单与 Git 分发边界，本次复核不代表图标设计批准或 P0 资产门禁已完成。 |
| 检查脚本来源 | 归档工程已实现 docs、traceability、references、repo、contracts、boundaries、rust、package 等检查入口，但其文档引用针对旧文档集。按当前六份合同重写，或移植后逐项核对，需在 P0-DOC-001 开始前定下。 |
| 依赖版本矩阵 | 归档 `Cargo.toml` 留有一组已验证 pin（Tauri 2.11.5、tauri-build 2.6.3、rusqlite 0.40.2 bundled+backup、ts-rs 12.0.1、reqwest 0.13.5、objc2 0.6.4 系列）。锁定配置时确认沿用或升级，不逐 crate 各自取值。 |
| macOS 27 构建约束 | 归档记录：macOS 27 与当时工具链下精简符号会破坏 proc-macro 的 LINKEDIT，release 需保持 `strip = "none"`，直到 M5 的工具链矩阵验证替换。 |

### M1 · 宿主与权限底座（下一批）

| 工作包 | 依赖 | 交付与验收 |
|---|---|---|
| M1.1 · 应用合同 | P0 | 需求默认值、Settings/Permission/Context DTO、ports、AppError、requestId 幂等与 expectedRevision；同 ID 不同载荷冲突，并发重复合并 |
| M1.2 · 存储与凭据 | M1.1 | rusqlite bundled、单写线程、WAL/事务、迁移/backup API、设置/请求回执表、脱敏日志、Keychain 内部端口；真实保存/重启恢复，损坏不覆盖原数据 |
| M1.3 · 宿主生命周期 | M1.1、M1.2 | 单实例先于初始化；菜单、按需控制台/设置、退出停止接收变更并清理；关窗口保留宿主，迟到结果不复活服务 |
| M1.4 · macOS 自检 | M1.1、M1.3 | Apple Events/AX 权限、Finder 结构化快照、NSOpenPanel、观察清理；无提示检测、显式申请、拒绝/撤销/恢复与超时分类 |
| M1.5 · 最小真实 UI | M1.2–M1.4 | 构建/启动/存储、自检与上下文页；只开放能生效的 theme/transparency/motionMode；冲突和保存失败保留草稿 |
| M1.6 · 原生验收 | M1.1–M1.5 | 多窗口 revision、幂等、权限各路径、虚拟目录/快速切窗/选区超限、特殊原生路径、目录取消、Keychain 自有测试项及资源回收证据 |

权限成功从系统重新检测取得，不因点击申请变绿；普通测试不重置用户 TCC。授权/撤销的系统操作需显式原生验收模式。测试用独立数据目录与凭据命名空间，结束清理。详细合同见架构第 12 节。

### M2 · 入口与持续终端

| 工作包 | 依赖 | 交付与验收 |
|---|---|---|
| M2.1 · 产品入口 | M1 | 输入条、热键、自启动、manual/followFinder、主动隐藏抑制、窗口定位；热键失败保留旧值，自启动回读系统，自动显示不抢焦点 |
| M2.2 · 会话 | M2.1、M1 存储 | Registry、分页历史、创建/选择/继续、置顶/结束/删除/重启中断；16 会话上限，手动唤起新建，删除不删用户文件 |
| M2.3 · 持续终端 | M2.2 | portable-pty、系统 zsh integration、输入租约、串行写入/resize/取消和回收；纯 AI 会话不创建 PTY，真实 Ctrl+C/IME/交互程序 |
| M2.4 · 目录同步与恢复 | M2.3、M1 Context | 前台进程组与空编辑行验证、pending/queuedLine、vt100 快照、ACK/重连；后台不注入，主屏/备用屏均可恢复，相关 FLOW 全通过 |

选区变化和目录目标变化独立；只有目标目录改变才撤回等待发送的命令。最小纵向路径为 Finder → Session → !pwd → 交互程序 → 隐藏继续 → 重新选择 → 结束。

### M3 · 全部能力与 AI

| 工作包 | 依赖 | 交付与验收 |
|---|---|---|
| M3.1 · 执行与策略 | M1、M2 Session/Context | ProcessRunner、Run、取消/队列、计划版本、可信效果分类与 `readOnlyAutoConfirmChanges` / `yolo` 两种策略；手动输入来源不可伪造，结果以文件/进程证据判定 |
| M3.2 · 模型与计划 | M3.1 | 多 OpenAI 兼容端点、本地模型、流式、测试连接、参数补齐、摘要回退；错误密钥/断流/限流/取消/非法计划均有闭环 |
| M3.3 · 工具设施 | M3.1 | ToolManifest、检测、staging 下载、来源与完整性、安全解压、原子安装、所有权卸载；取消/坏包保留旧版本，安装后重评计划 |
| M3.4 · 基础能力 | M3.1–M3.3 | 文件、ZIP、图片、媒体、PDF、文本/文档的复用执行器；AC-CAP-001–030，含重名/部分失败/取消/特殊路径 |
| M3.5 · 扩展能力 | M3.4 | 图像扩展、富文本、OCR、转写字幕、系统、Git、网络、计算、消息；AC-CAP-031–135，条件满足成功与条件不足均覆盖 |
| M3.6 · 规则收藏与历史 | M3.1–M3.5 | 四种规则作用域、快照、收藏复用、输入历史、导入导出和清理；重新绑定上下文，不形成永久批准旁路，AC-FLOW-012 |

每项工单引用台账行的参数、格式、依赖与 AC，不用类目名称代替验收。工具具体版本、来源、许可证与校验材料在接入时锁定，不虚构下载站或账号。

### M4 · 完整 UI；M5 · 发布

M4 在各阶段已能操作的真实 UI 上完成输入条、任务浮层、结果、终端、会话、完整控制台与七类设置；验证视频/截图、焦点、IME、读屏、reduced-motion、透明度降级与异常闭环。

M5 以 macOS 14+、Apple Silicon/Intel 为发布矩阵，分别取得最低系统和发布时稳定系统的原生记录；完成性能、安装/升级、签名公证、更新验签、图标和源码材料。跨编译不替代实际运行；缺签名身份/更新公钥/发布地址时标明发行环境阻塞，不宣称已发布。

## 3. 需求到实现/测试的追踪

| 需求组 | 主要模块 | UI | 阶段与验证 |
|---|---|---|---|
| FR-ENTRY、FR-PLATFORM | application、platform | UI-COMPOSER、UI-SETTINGS、UI-PERMISSIONS | M2/M5；原生显示、权限、热键与退出 |
| FR-CTX | application、terminal、platform | UI-COMPOSER、UI-TERMINAL | M2；AC-FLOW-005/006/007 和目录同步扩展矩阵 |
| FR-SESSION | application、terminal、process、storage | UI-SESSION-SELECTOR、UI-WORKSPACE | M2；AC-FLOW-003/004/011，真实进程生命周期 |
| FR-TERM | terminal、platform | UI-COMPOSER、UI-TERMINAL | M2；AC-FLOW-009，交互程序与大量输出 |
| FR-AI、FR-POLICY | planner、policy、model | UI-RUN-DETAIL、UI-MODELS | M3；AC-FLOW-008，API/策略/计划修订 |
| FR-RUN、FR-DATA | application、process、storage | UI-RUN-LIST、UI-RUN-DETAIL | M3；取消、部分成功、重试、损坏数据 |
| FR-CAP、FR-TOOLS | catalog、tools、process、platform | UI-CAPABILITY-LIBRARY、UI-TOOLS | M3；全部 AC-CAP 与 AC-FLOW-013 |
| FR-RULE、FR-FAV | planner、application、storage | UI-RULES、UI-FAVORITES | M3；AC-FLOW-012 |
| FR-SET、NFR-UX | application、storage、platform、UI | UI-SETTINGS 及全部首版页面 | M1/M4；真实保存、冲突、键盘与视觉 |
| NFR-SEC/REL/PERF/DATA/TEST | 所有模块 | 关联界面 | 对应阶段先验证，M5 汇总实际结果 |

能力台账的每个 CAP 行本身包含输入输出、工具、平台和 AC；开发工单直接引用该行，不能用“支持图片”“支持终端”替代具体验收。

## 4. 必测场景与判定

### 4.1 Finder、显隐和目录同步

| 场景 | 通过条件 |
|---|---|
| 全新安装/manual/无 hotkey | 栏隐藏，菜单引导注册；注册成功后可显式新建 |
| followFinder/无 hotkey | 有效上下文可自动显示，与热键配置无关 |
| 自动显示后主动 hide | 不被下一次窗口/目录/选区事件立刻唤回；显式 show/改 activation 才解除抑制 |
| endAll 下拖动 Finder | 暂隐再恢复同一会话，进程不结束 |
| A 前台/B 后台，Finder 换 C | 只当前 A 收到同步，B 保留原终端与目录 |
| 当前 shell 空提示符 | 自动进入新目录并验证 cwd 后显示成功 |
| vim/REPL/密码提示/前台长命令 | 不向程序注入 cd，pending 目标可见，原始键仍可用 |
| shell 有半行未提交输入 | 不拼接 cd；原输入保留，清空/执行后再同步 |
| Finder 连续 B→C→D | 只最新目标 D 应用；过期回执不能覆盖当前状态 |
| 等待同步的新 ! 行遇下一次目录变化 | 撤销自动投递、保留草稿并提示重新提交，不能换到新目录暗中执行 |
| 目录重命名/删除/无权限 | 保留真实 cwd、显示同步失败；未投递命令不执行 |
| 空格、引号、换行、Unicode、命令符号路径 | 同步到正确原生路径，不执行名称中的内容 |
| 搜索/智能目录/多目录结果 | 不猜 cwd；可明确选择目录；结果按真实目录定位 |
| 休眠、屏幕热插拔、缩放/空间切换 | 恢复有效位置、上下文和显隐策略；不新建或结束错误会话 |

### 4.2 会话与终端

- 三个活跃会话：两个持续终端、一个 AI 工作并行；切换、隐藏与重新选择期间输入/输出不串线。
- keepAll 隐藏后继续产出输出，再唤起新建；endAll 结束全部；单会话结束只影响指定会话。
- 置顶后原会话历史保持、排序更新；删除活跃会话结束其进程并移除记录，保留用户文件。
- 历史只读、继续生成新 ID；重启恢复历史，不自动重跑命令、安装或模型请求。
- 终端真实处理输入、Tab、方向键、Ctrl+C、Esc、粘贴和中文 IME；AI 输入区域不抢终端控制键。
- 终端程序进入/退出 alternate screen，隐藏重连、宽字符、颜色、光标、resize 后画面与交互正确。
- 连续输出累计十万行后仍能输入和取消；确认缓冲上限、日志段、截断标识与历史读取。
- 结束/退出后检查所属进程树、PTY、线程、Channel、输入租约和事件订阅全部释放。

### 4.3 AI、文件、依赖与数据

- `readOnlyAutoConfirmChanges` 与 `yolo` 分别执行相同的只读、改名、未知脚本、安装、Git 推送和消息发送计划；手动终端均直通。
- 模型返回非法结构、错误能力 ID、虚构只读或改变策略的文本时，不绕过 Rust 校验。
- 确认绑定计划版本；编辑、替换参数、源文件变化或依赖安装后重建计划，不复用失效确认。
- 固定能力离线执行；本地模型可用；云服务错误、断流、取消、摘要失败均保留真实执行结果。
- 批量文件部分成功、跨卷失败、同名输出、只读/云端占位、空间不足、执行中源文件改变都有逐项结果。
- 内置/结构化写计划同文件冲突有确定处理；自由脚本无法推导的影响明确 unknown，不声称全量预知。
- 下载损坏、签名不匹配、越界归档、取消安装和卸载外部管理工具均按工具合同处理。
- 规则作用域与顺序正确；收藏复用新的上下文；JSON 导入失败不破坏原数据。
- 检查应用管理凭据、密码参数和禁用回显的 PTY 输入不会作为日志/历史明文保存；原始终端按键不落盘。

### 4.4 UI、性能与原生发布

工作区/设置对照原始图片，输入条/浮层对照 UI 文档列出的时间点；截图记录主题、逻辑尺寸、缩放、系统版本和状态数据。新增页面按设计合同验收。

UI 需覆盖空/加载/失败/成功、长路径/长中文、焦点返回、菜单关闭、输入法、reduced-motion 与透明度降级。不得为了截图让不可用入口表现为真实功能。

性能验收以 60Hz 显示、最低 8GB 内存的 Apple Silicon 设备为基线，并记录具体系统、CPU、内存、构建模式与工具版本；Intel 补充兼容性验收。按 NFR-PERF 测量可见反馈 p95、十万行输出和至少三会话场景，记录实测值，不把目标写成已达成结果。

真实 App 必须验证 Finder 权限、全局键、窗口层级、多屏和进程生命周期。浏览器测试可以验证页面逻辑，但不能代替 WKWebView/原生行为。

## 5. 检查命令与测试边界

所有本节命令在新工程根目录执行。旧 `Web APP/` 的测试会重写参考截图，本批不运行；参考工程检查不计入新 App 验收。

下列命令由 P0 建立，位于仓库根 workspace；真实运行结果见[实施状态](status.md)。`pnpm check` 串行组织必要检查（docs、traceability、references、repo、rust、contracts、boundaries、typecheck），原生测试另行显式运行。

| 命令 | 用途 |
|---|---|
| `cargo fmt --all --check` | Rust 格式 |
| `pnpm check:rust` | 实际工具链的格式、Clippy（workspace/all-targets/-D warnings）、workspace 测试 |
| `cargo test --workspace --locked` | 已实现 Rust 行为；空模块不编造业务测试 |
| `pnpm run doctor` | 版本、工具链组件、Xcode 与目标环境 |
| `pnpm check:docs` / `pnpm check:traceability` | 引用、阶段、ID 与追踪一致性 |
| `pnpm check:references` / `pnpm check:repo` | 参考哈希、本地视频和 Git 忽略边界 |
| `pnpm check:contracts` | DTO 生成到临时目录后与提交产物比对，避免测试静默改类型 |
| `pnpm check:boundaries` | 模块依赖与 UI 宿主边界 |
| `pnpm typecheck` / `pnpm build` | 新 UI 类型检查和构建 |
| `pnpm test:ui` | 新 UI 行为/截图，基线写入需要显式更新命令 |
| `pnpm test:desktop` | macOS 原生场景驱动与记录（embedded WebDriver 测试构建，证据在 `tests/.artifacts/desktop/`） |
| `pnpm tauri build` | 新 App 的实际安装包构建 |
| `pnpm verify:package` | 核对普通包 Info.plist、签名、架构、无测试驱动并记录启动/退出证据 |

状态机、策略、路径处理和并发是有行为风险的逻辑，需有对应测试。纯文案/文档更新采用引用、ID、格式和一致性检查；不为每条文字复制一份断言。必要检查通过后，仅因新改动、失败或未解决疑点扩大测试。

每个测试记录至少包含 requirementId、capabilityId（如适用）、caseId、环境、输入、预期、实际、日志/截图路径和结果。对外账号、打印机、磁盘等条件能力须在满足条件时验证成功路径；条件不足的错误测试不能替代成功路径。

## 6. 文档完整性门槛

- 六份正式文档互链可访问；项目规则只指向当前入口。
- 105 个历史来源 ID 与迁移前集合相等、唯一；30 个基础能力和全部映射均有 AC，不保留未决来源项。
- 首版能力没有被标成“以后再做”；仅实时语音和后续平台处于明确后续里程碑。
- manual/followFinder、keepAll/endAll、两种 AI 策略在所有文档使用同一枚举与含义。
- 最新目录同步合同覆盖自动 cd、busy pending、原始终端直通、前后台隔离及过期事件。
- 旧核心文档已移除，其文件名不再出现在有效引用中；原始图像、视频、UI 源码和图标工程保持。
- 各文档明确区分“已经审查/写入”和“后续待实现/待验收”。

## 7. 后续平台与下一版本

Windows/Linux 在 macOS 完整版本后独立推进：复用 domain/application/catalog/model/storage/UI，实现平台上下文、终端进程、热键、凭据、窗口与发布适配。支持矩阵逐环境验证；Linux X11/Wayland 的定位和文件管理器差异不得用同一个“支持 Linux”勾选掩盖。

2026-10-06 修订：先推进 Windows 11 x64 核心修复，范围为需求与能力台账登记的核心流程及 21 项基础能力；不等待 macOS M5 收尾，也不扩展本轮 Linux 范围。构建、原生 API、ConPTY/进程树、目录/选区、文件结果与 NSIS 安装分别验收；桌面焦点、设备或外部模型条件未满足时明确列为未验证，不能用交叉编译或纯函数测试替代。

实时语音作为下一版本独立模块：麦克风权限、音频采集/播放、实时模型会话、打断和 transcript 与 Session 的关联。它不改变首版已经包含的文件转写、字幕和 OCR 的交付义务。

移动端、普通浏览器执行产品、云端账号同步和商业化服务不在当前路线图中。后续新增时先更新需求和接口合同，再进入实现。

M5 更新安装追加要求（2026-09-23）：macOS 新构建在使用相关能力之前须重置本应用隐私授权，并由用户重新授权。当前普通安装包已接入按构建签名去重的公共启动流程，后续自动更新器不得绕过或自动补回授权，详见[重新授权流程](permission-update-policy.md)。

## 2026-09-25 发布版本决定

首个公开测试版定为 0.0.2（Beta）。Cargo/npm/Tauri 使用同一三段数字版本，stage=BETA，GitHub release 标记 prerelease；此前 0.0.1-beta.1 记录作为历史事实保留。PDF 工具路径、官方品牌图标、精简界面与签名更新链路纳入本次修订。公开测试版不代表 33 条条件验收、正式签名公证、Intel 和 M5 完整验收完成。
