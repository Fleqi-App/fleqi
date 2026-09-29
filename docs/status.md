# Fleqi 当前状态

更新：2026-09-29。项目版本 **0.0.2 Beta**。Linux 安装包 `Fleqi_0.0.2_amd64.deb` 已在本机用 dpkg 安装并启动：控制台就绪，Secret Service 自检可用，目录选择可用；PCManFM 窗口能解析到真实目录。权限页写明没有 Finder 自动化。Windows 安装包 `Fleqi_0.0.2_x64-setup.exe` 已由交叉编译生成，可执行文件是 PE32+ GUI 程序并链接 WebView2Loader；本环境没有 Windows 虚拟机，安装器未在 Windows 上执行。**这不是 Windows/Linux 发行，也不能宣称与 macOS 首版等价或完整验收。** macOS 公开测试版事实不变：已修复执行调度、真实上下文、会话结束、历史恢复与本地能力接线等主要阻塞，并补齐扩展能力执行入口。**可作 macOS 公开测试版，尚不能宣称完整首版已验收或正式发行。** 详细交付、验证和残留事项见 [实现与修复记录](implementation-2026-09-20.md)；[原始核查](review-2026-09-20.md) 保留修复前证据。

2026-09-25：修复 GUI 启动的任务和终端找不到 Homebrew 工具的问题，加入 pdftotext/pdfinfo 自动准备；实际 PDF 正文读取及最小 PATH 的真实任务回归通过。侧栏使用官方导出图标，文件设置精简，诊断详情默认折叠。加入启动检查、用户发起安装、下载签名校验、原子替换和升级后重新授权。真实发布归档已在隔离目录验证替换及代码签名；篡改包和降级版本被拒。详情见[发布与更新](releasing.md)及[更新说明](release-notes/0.0.2.md)。

本次验证：260 项 Rust 常规测试、47 项 UI 单测、15 项浏览器回归、17 项原生用例通过，另运行真实发布归档的安装与签名定向验证。类型、契约、Clippy、文档和边界检查通过。发布快照的 82 项 Git 参考资产校验通过；本机已有的 Icon Composer 设计修改保留在工作区，不随本次发布覆盖官方导出或更新历史参考清单。发行证据存于 `tests/.artifacts/release-2026-09-25/`，生产依赖审计未发现已知漏洞。

2026-09-23 已按试用反馈改为 shadcn 官方组件、原生红绿灯和菜单栏常驻，修复 Finder 选区过期、失焦/拖动跟随、快捷键恢复，并补齐任务浮层与平台材质。243 项 Rust、44 项 UI 单测、11 项浏览器回归、36 项原生常规用例通过，另完成 1 项真实 Finder 点击的 A/B 图片改选验收。普通包已构建、启动核验并更新至 `/Applications/Fleqi.app`。详见[试用反馈修订](refinement-2026-09-23.md)；该记录明确了仍未完成的后端与发行合同项。

以下表格与 2026-09-20 的能力矩阵是产品基线，不能把本轮窗口与输入修复视为完整首版验收。

| 范围 | 当前结论 |
|---|---|
| 需求、能力、UI、架构与开发计划 | 有效合同，未缩减产品范围与默认值 |
| P0 与 M1 | 基础工程、宿主、存储与权限已有交付 |
| M2 入口、会话与终端 | 修复浮层、租约、订阅、结束联动与会话回执；完整历史分页/回放和非 UTF-8 同步边界仍需补齐 |
| M3 AI、工具与能力 | 多步骤/队列/重试/上下文/恢复/本地表单已接通；111 个 CAP ID；基础 30 条通过，扩展 105 条为 **72 pass / 33 cond / 0 gap / 0 fail**，cond 不算完整通过 |
| M4 产品界面与交互 | 新增能力操作、任务定位、会话正文、规则编辑与收藏复用；UI 自动化通过，人工辅助功能与细粒度运行进度仍待验收 |
| M5 发布与全量验收 | 部分完成；签名更新链路已有本机证据，Developer ID、公证、Intel、性能 p95 与跨系统升级仍未验收 |

2026-09-20 验证记录：`pnpm check`（238 Rust 通过、1 外网测试默认跳过）、46 UI 单测、11 浏览器回归、36 原生用例通过；最后 UI 目录修补后另重新构建通过 3 项原生定向回归。完整通过范围及未验收条件见实施记录。

以下为此前实施记录；其日期、数量与哈希属于当时的局部结果。若“已完成”措辞与本轮核查冲突，以上当前结论优先。产品完成度不再以旧的 0% 或未经核验的整体完成声明表示。

## M2 实施核验（2026-09-19 完成）

已交付并通过验证的部分：

| 工作包 | 交付 | 实际结果 |
|---|---|---|
| M2 领域状态机 | SurfaceMachine（§5.1 全矩阵：manual 需已注册快捷键、followFinder 独立、主动隐藏抑制与解除、系统暂隐按显示周期恢复、barEnabled 关闭按 hideBehavior 处理一次、选择后台会话触发同步）；SyncMachine（§5.2：安全提示符判定证据、忙/编辑行/前台非 shell 时 pending 只保留最新目标、过期回执忽略、失败保留真实 cwd、queuedLine 每会话一个、目标变化撤销自动投递保留草稿、后台撤销但在途回执仍更新 cwd、手动 cd 不拉回）；Session/ConversationEntry/TerminalSnapshot 类型、16 上限、`!` 解析 | 领域 15 项测试通过（308082d） |
| M2.3 终端适配器 | portable-pty + 系统 /bin/zsh -i + 自带 shell integration（临时 ZDOTDIR 包装，不改用户配置；OSC 7331 报告 prompt+hex cwd/preexec/编辑行长度）；OscExtractor 跨块剥离私有序列（其它 OSC 不作空闲证据）；安全性 = prompt ∧ 编辑行空 ∧ tcgetpgrp==shell ∧ 非投递；目录控制消息仅本模块生成（单引号转义+双连字符+前导空格不进历史）；vt100 屏幕快照、512KiB 环+流游标订阅、8MiB 分段落盘 100MiB 截断；shutdown SIGHUP→SIGKILL tty 残留 | 6 项真实 zsh 集成测试：特殊字符目录（空格/引号/换行/连字符/命令替换/反引号/中文）不执行、忙碌与 Ctrl+C、编辑行、resize+快照+游标重连、无孤儿进程（eb3d14b） |
| M2.2 会话存储与服务 | SQLite v2（sessions JSON+排序列、conversation_entries 级联删除、损坏单条跳过）；SessionService（重启标记 interrupted 不复活进程、16 上限、置顶/活动排序、结束/删除/继续）；TerminalService（按需 PTY、无有效目录要求选择、输入租约、submit_line 立即/排队、目标变化撤销草稿、cd 确认自动投递一次、后台撤销、快照/游标订阅/回收）；SurfaceService（状态机驱动 + SurfaceDeps 注入） | 应用编排 8 项测试通过；修复 submit_line 持锁自死锁（sample 定位）与排队投递错误比较（55173e2） |
| M2.1/2.4 宿主 IPC | AppState 装配三服务；35 命令（surface_*/session_*/terminal_*（含 Channel 流订阅、宿主再验证 `!` 标记）/hotkey_*）；global-shortcut + autostart 插件；composer 窗口角色；托盘"显示输入条"（manual 未注册时引导设置）；白名单/capability 同步 | 原生验收 9 项（两进程）：surface_show 拒绝与接受路径、热键真实注册+持久化+解除、显式显示建会话/隐藏/再显示新建、重启恢复设置版本 4（1107580） |
| 契约 | AppEvent 增加 session/surface/terminal:changed；54 个 TS 绑定（含 Session/TerminalSnapshot/QueuedLine） | `pnpm check` 全绿（71bfc5b、511675b） |
| M2 UI（输入条/会话/终端） | #/composer 路由：两行输入条（`!` 全绿+文字标识、真实目录行、无热键拒显提示、sent/queued 反馈、隐藏按钮）、会话选择器（置顶/活跃/历史、结束/置顶）、终端面板（按需 open+租约+Channel 字节流、原始按键直通含 IME 组合保护）；desktop 适配器补齐 M2 命令与 3 类事件 | vitest 18 项（composer 6 项）+ Playwright 9 项（composer 3 项基线）通过；原生 9 项回归通过（511675b、c6512c8） |

## M3 实施核验（2026-09-18，进行中）

| 工作包 | 交付 | 实际结果 |
|---|---|---|
| M3.1 执行引擎基础 | fleqi-domain::execution（ExecutionStep native/process/script、Effect 13 类、policy_decision 两策略、classify_command_trust 已审定只读命令集、RunOrigin/RunState 11 态状态机）；fleqi-adapters::process ProcessRunner（portable-pty 显式 CommandBuilder 无 shell 拼接、取消杀进程组、Drop 清理、输出流广播 Exited） | 领域 7 项 + ProcessRunner 4 项真实进程测试通过（434adb1）；模型自称只读不参与策略判定、未知命令不冒充只读 |
| M3.2 模型适配 | OpenAiCompatibleAdapter（reqwest 0.13.5 blocking）：流式 chat/completions SSE 解析、密钥直发用户端点（Authorization Bearer，无自营代理）、ModelError 分类（认证/网络/限流/模型/响应）、probe /models 连接测试 | 3 项本地 mock HTTP（真实 TCP）测试通过：SSE 重组+密钥+model、401→AuthFailed、探测列表（9775b87） |
| M3.2 规划闭环 | ProviderService（providers 表 v5；密钥只入 Keychain `provider.<id>`，视图仅"已配置"掩码，不回传明文；HTTPS/回环校验）；PlanningModelGateway（流式聚合 + 块间取消）；PlanningService：模型 → 受控 JSON 计划（scripts/effects/previewComplete）→ ExecutionPlan → RunService 按当前 aiPolicy 提交；非法计划一次修正重试、两次仍非法回退摘要文本（不执行）；认证/网络/限流/取消/非法响应各有错误闭环与 retryable 标记 | provider 2 项 + 规划 6 项应用层测试（真实 RunService/即时进程端口）：无端点引导、合法计划自动执行、修正重试后执行、两次非法回退摘要且不建 Run、认证不可重试/限流可重试、变更效果等待确认；模型网关 3 项真实回环 HTTP（聚合、401→Auth、预置取消） |
| M3.3 工具设施 | 领域 ToolManifest/ToolSource/ToolStatus（受管包必须 HTTPS/回环 + 64 位十六进制 SHA-256；系统工具仅 PATH 名）；ToolManager：staging 下载（流式增量哈希、分块取消）→ 校验 → 安全解压（越界/绝对/反斜杠条目拒绝）→ 预检（FR-TOOLS-005 坏包不发布）→ 原子目录替换（旧版本保留至新版本验证通过）；`fleqi-tool.json` 所有权标记，卸载只删应用拥有目录；installed_tools 表 v4 + ToolService（登记/探测/安装/卸载，已有系统工具复用，缺失项通过固定 Homebrew 映射补装，系统工具不提供 Fleqi 卸载） | 领域 4 项 + 适配 6 项真实文件/进程测试：系统 git 真实探测、回环 HTTP 下载安装+标记+staging 清理、坏校验保留旧版本、zip-slip 拒绝、取消中断保留旧版本、卸载只删受管目录且无标记目录拒绝；应用层 3 项 |
| M3.4 基础能力执行器 | 六类共用 AC-COMMON 合同（uniqueName 不覆盖、批处理逐项报告、原件保留）：文件（创建/目录/复制/移动/改名/编号/整理/回收站 Apple Events）、ZIP（打包/列表/解压含越界拒绝）、图片（image crate 三格式六方向/缩放不放大/旋转）、媒体（ffmpeg 检测缺失报 ToolUnavailable、音频转换显式 argv）、PDF（lopdf 测试生成/页面树移植合并/逐页拆分）、文本（TXT 编码读取/Markdown/DOCX OOXML 生成与正文提取不执行宏） | 15 项真实文件操作测试全过（f361963），含 AC-CAP-001..030 代表路径与 AC-COMMON-003 |
| M3.5 扩展能力 | 元数据（逻辑/占用、目录递归含不可访问清单、前 N、内容哈希去重）、SHA-256、字数三规则、Git 判定、ping/下载（回环真实字节）、系统信息（CPU/内存/显示器/电池 None/功率 None）、计算四类、OCR（tesseract）与转写（whisper-cli+模型检测） | 11 项测试全过（530bb0f）；本机 whisper 模型齐备走真实转写成功路径，工具/模型缺失分别报条件 |
| M3 Run 编排 | RunService：提交/策略判定/确认（runId+planRevision，过期 conflict）/取消（保留输出）/重试（新 Run 关联）/失败保留退出码与输出/并发 4 排队；RunStore + ProcessPort 端口（事件带流标识序号）；run:changed 事件 | 7 项脚本化进程端口测试通过（09d7613） |
| M3.6 规则、收藏与历史 | SQLite v3（runs、rules、favorites、input_history）；CollectionService：Rule 四作用域与 matching_rules 按目录+提示重新求值（每次执行重新绑定，不形成永久批准旁路）、Favorite（ai/manual）创建/更新/删除、输入历史追加/读取/清理 | 4 项应用层测试通过（a989e50）：四作用域 CRUD、作用域匹配语义（目录/前缀不越界）、收藏 ai/manual 区分、历史 200 上限与清理只删记录 |
| M3 宿主 IPC | 26 个 M3 命令（run_submit/approve/cancel/get/list/retry、run_plan_submit、provider_save/list/delete、rules_\*、favorites_\*、history_\*、provider_probe、catalog_query、tools_list/install/remove）；run_submit 载荷固化计划（PlanWire→ExecutionPlan），origin 固定 AI 侧不可伪造 manual；provider_probe 密钥经 Rust 直发用户端点；白名单/capability 同步至 58 命令 | 原生回归 05-m3 5 项 + 06-m3-planning 1 项通过（真实 IPC + 真实 /bin/sh + 真实回环网络）：只读计划 readOnly 下自动执行并落盘输出、变更效果 awaitingApproval+过期计划版本 conflict+正确版本确认后执行、未知效果不自动执行+取消保留记录、目录六类 30 项与输入历史写入读回、工具探测登记与系统工具安装/卸载拒绝、无模型端点引导、端点登记掩码视图、不可达端点如实报错（可重试） |
| M3 UI 页面 | 设置：模型与 API 页（端点列表+编辑器+连接检查 provider_probe、密钥只写不读、超时校验）、任务与诊断页（aiPolicy 分段控件真实保存、上限项以说明呈现）、文件与工具页（nameConflict 真实保存、输出位置/选中项上限说明、输入历史查看与清理）；控制台：任务页（Run 列表/详情/确认/取消/重试）、能力库页（六类目录+规则增删+收藏增删）、工具页（探测状态/来源/安装位置、受管安装/卸载、重新检测）；设置字段白名单升级 All（aiPolicy 等由 Run/Planning 真实消费）；71 个 TS 绑定（新增 Run/Provider/Tool/Rule/Favorite/PlanOutcome） | vitest 25 项（新增模型页/aiPolicy/工具安装卸载/任务空态/规则增删）+ Playwright 9 项（基线随侧栏扩展更新，aiPolicy 真实保存断言）+ `pnpm check` 全绿 + 原生回归 6 specs 全过（设置/控制台既有断言不受影响） |

M3 尚未完成（不计入完成度）：

- 安装/下载/模型调用的进度与取消 UI 接线（后端取消通道已实现并测试，进度呈现属 M4）。
- 135 项能力 AC 的全量矩阵：基础 30 项已全量真实运行（见 M4 E1 行）；扩展 031–135 批次进行中。
- 模型页的密钥"替换/清除"显式控件与模型选择持久化到 settings.defaultModel（当前经 provider 记录保存；UI 收尾随 M4 输入条模型选择面板）。

## M4 实施核验（2026-09-19 完成）

| 工作包 | 交付 | 实际结果 |
|---|---|---|
| 通用设置页收口 | 快捷键录入（§10.3 全流程：录入中→候选→真实注册；单键有效；Esc 取消/显式录入 Esc；失败保留候选与旧绑定；可清除）；launchAtLogin 联动；bubbleSeconds（1–30s/常驻）与条内建议 1–5 真实保存；设置字段白名单升级 All | vitest 快捷键取消/候选/注册成功路径 + Playwright aiPolicy 真实保存（c1110e0） |
| 输入条 AI 路径 | AI 提交经 run_plan_submit（真实规划闭环）；无端点如实引导；执行计划气泡指向控制台任务页、摘要回退标注"未执行命令"；queuedLine 取消按钮经 terminal_cancel_queued 恢复草稿；显示状态警告与提交反馈分离（迟到的显示错误不覆盖用户动作结果） | vitest 27（引导路径、配置端点后 run 气泡）；Playwright 9（84ce593） |
| 终端面板 xterm.js | xterm.js 完整渲染（ANSI/光标/滚动/10000 scrollback）、fit→terminal_resize 同步 PTY 尺寸、onData 原始输入直通（组合键/粘贴由 xterm 序列化，IME 组合保护）、terminal_ack 消费位点回执（TerminalService 记录 acked cursor；有界环 + 分段持久化已约束内存，回执用于流控/诊断，不改变重连保留合同） | vitest 27 + Playwright 9（基线更新）+ `pnpm check` 全绿 + 原生回归 6 specs（本机全绿；一次满载并发运行出现过的超时为负载抖动，复跑稳定通过） |
| composer 贴附 Finder | 固定 AppleScript 读最前 Finder 窗口 bounds（2s 超时、失败回退默认位置）；show_composer_attached：宽度取 Finder 宽（560–1120 截断）、贴标题栏下方、skip_taskbar + always_on_top；全局热键真实切换（可见→隐藏；否则显示抢焦点）；Finder 激活观察自动显示不抢焦点；surface:changed 监听使窗口表现与状态机同步（此前热键只改状态从不开窗） | `pnpm check` 全绿 + 原生回归通过（a66c3c7） |
| AC-FLOW 原生矩阵 | 07-m4-flows 5 项（真实 IPC + 真实设置 UI + 真实 zsh）：003 keepAll 隐藏保留 A/再唤起 B/切回 A 活跃；004 endAll（经设置页分段控件真实保存）主动隐藏结束全部；006/014 真实 sleep 占用提示符下排队命令（等待目录同步）→ 取消恢复草稿且不执行；007 Run 上下文独立（ctx-a 不漂移）；008 yolo（经设置页真实保存）未知效果免确认执行；010 非法热键候选被拒且旧绑定保留、清除后未绑定 | 全部通过（含全 suite 6+1 specs 两进程）；发现并修复真实缺陷：terminal_submit_line 线格式为裸 "sent"/"queued" 字符串，desktop 适配器与输入条此前按 {result} 解析（84ce593 修正）；03-restart 版本断言改为 ≥4（run-a 多 spec 合法推进版本） |
| 会话编排与历史继续 | 新增 session_continue 宿主命令（历史继续创建关联新会话：parentSessionId 关联、继承当前上下文；活跃来源 conflict、不存在 not_found；白名单/capability/build.rs 命令清单同步）；适配层补 sessionDelete/sessionContinue；会话选择器历史行新增"继续/删除"（删除仅移除会话记录，不删用户文件）；08-m4-session 5 项由新增 run-a2 独占串行运行（run-a 的 spec 文件为并行 worker，surface/会话编排断言需要独占状态）；03-restart 新增重启恢复断言 | 原生 5 项全过：002 followFinder 自动出现（无快捷键切换即显示）、主动隐藏后 autoShowSuppressed=true 不弹回、切换模式与显式唤起解除抑制（surface_get 真实状态）；011a 图钉排序（置顶优先、双置顶按最近活动、取消置顶恢复）+ 单独结束 A 后 B 的终端仍真实执行 cat（持输入租约）；011b 删除活跃记录（命令内部先结束）与已结束记录、新会话仍读到 /tmp 标记文件（不删用户文件）、不存在会话 not_found；011c 历史继续 parentSessionId 关联且源保持只读历史、活跃/不存在来源拒绝；011d 选择器 UI 真实点击继续（选择器关闭并切到新会话）；run-b 断言上次进程遗留活跃会话标记 interrupted 且 terminalId 清空（不重跑进程）。修复真实缺陷：capability 缺 allow-session-continue（tauri ACL 拒绝调用）；run-a 遗留 endAll 持久化污染共享数据目录（依赖 keepAll 的用例现显式恢复）；PTY 输入必须持租约（terminal_acquire_lease），无租约写入被拒 |
| M2 收口：Finder 窗口移动观察（2026-09-19） | 宿主新增 Finder 前窗几何观察线程（800ms 节奏；visibility ∈ {visible, temporarilyHidden} 时观察，userHidden 退出）：bounds 变化 → surface.system_hide()（拖动暂隐 + 暂停目录投递）；连续 2 次稳定采样 → surface.system_restore()（恢复可见并重同步，经 surface:changed 重新贴附、已在显示时不重复执行 AppleScript）；服务端 system_hide/system_restore 复用既有状态机（m2_contract 已覆盖），补应用层回归 finder_drag_temporarily_hides_pauses_delivery_and_restores（暂隐期异目标提交排队、恢复重同步把排队命令撤销为草稿）；原生验证 09-m2-geometry.spec（test-desktop runner 内置几何驱动器：标志文件启停、以 osascript 真实移动 runner 自建的 /tmp Finder 窗口——按用户许可的自动驱动，不触碰用户窗口） | 原生 1 项通过：真实移动 Finder 前窗后 surface 进入 temporarilyHidden（可见会话保留），停止移动后恢复 visible 且同一会话保留（证据 m2-geometry-watch.json）；发现并修复真实缺陷：hotkey_commit 内部幂等 requestId 只含加速键，两个窗口先后提交同一加速键（expectedRevision 不同）被判"同 ID 不同载荷"冲突 → 持久化静默失败、热键失效——现携带调用方 requestId（native 套件真实暴露）；runner 同步修正：run-a2 原单 spec 的"串行"假象在多 spec 下变为并行 worker，现拆为三次顺序 wdio 调用 |
| AC-FLOW-015 忙碌退后台 | 新增原生用例：真实 sleep 占住前台 → 对另一目标提交 `!` 命令进入排队 → keepAll 主动隐藏 → 撤销在途目录切换与未投递命令（terminal_withdrawn_line 取回草稿）、pendingDirectory 清空 → 再显式唤起新建会话且程序继续运行 → sleep 结束后被撤销命令未执行。发现并修复真实缺陷：SurfaceService::apply 在状态机转换后才取可见会话，UserHide 已清除会话 ID，隐藏副作用（set_visibility cancel=true）从未执行——排队命令不撤销、草稿丢失；现改为转换前快照会话并传入 execute（Hide/TemporarilyHide 共用），并补应用层回归测试 user_hide_withdraws_queued_line_for_visible_session | 原生 1 项 + Rust 回归 1 项通过；run-a2 6/6 |
| E2 · 013 扩展 105 矩阵（2026-09-19） | 新增 crates/fleqi-adapters/tests/capabilities_matrix_extended.rs：AC-CAP-031–135 逐行落盘。真实场景（pass 39）：OCR（ffmpeg 画字 + 无文字不编造）、字数三规则、PDF 提取顺序/结构压缩/逐页拆分/选页旋转、zip 特殊名往返+坏包反馈、逻辑大小、目录递归、前 N、内容去重、多项回收站恢复、移动不成环、整理仅当前层、下载来源不猜测、改名宽度、大小写碰撞经 unique_destination 检测、计算四类、CPU/内存/显示器/电池真实探测、git 仓库判定、回环下载字节一致。gap 50：执行器缺失逐项记录（媒体信息/字幕/视频操作、图像扩展、文档转换、编辑器、PDF 图片与元数据与加密、内容检索、文件属性、git 拉取分支推送、brew 清单安装卸载、天气、邮件）。cond 16：条件反馈验收（模型摘要走无端点引导、终端 cwd 由真实 zsh 测试覆盖、系统级操作实现时必须真实拒绝）。证据 tests/.artifacts/ac-matrix/extended.json | cargo test：pass=39 fail=0 gap=50 cond=16，105 行全落盘；扩展矩阵与基础矩阵证据目录统一解析到仓库根 |
| F · 验收记录成册（2026-09-19） | docs/verification-m4.md：AC-FLOW 12 条证据索引、交互维度（焦点/IME/reduced-motion/透明度/读屏）、013 矩阵口径与结果、人工验收清单（托盘点击、VoiceOver、录屏、brew、模型端到端）、M5 现状 | 文档随实现同步；check:docs 链接与 ID 校验通过 |
| E1 · 013 基础 30 矩阵（2026-09-19） | 新增 crates/fleqi-adapters/tests/capabilities_matrix_basic.rs：AC-CAP-001–030 逐项真实场景（Unicode 路径创建/多层目录/混合复制哈希/移动与失败不误删/预览改名含无扩展名/双目录编号确定性/按类型整理不递归/真实回收站恢复/打包解压哈希/越界 zip 拒绝/图片六方向+缩放旋转不放大/JPG 质量/音频六方向 ffprobe 容差/MP4 转码双轨编码/音轨提取与无音轨反馈/精确裁剪/PDF 合并拆分重组），结果逐项落盘 tests/.artifacts/ac-matrix/basic30.json。为此补齐三个缺失的 PDF 执行器：pdf_extract_pages（乱序/重复按显式顺序、越界拒绝执行）、pdf_rotate_pages（选中页 /Rotate 累积、未选页不变）、pdf_compress（结构重写不降质，体积不降时如实返回两数）；trash() 现记录回收站内位置（restore_paths）且 restore_from_trash 真实移回原位（唯一化命名，替换原"随 M4 交付"占位） | cargo test 基础30：pass=29（029/030 合并场景）fail=0 gap=0，fleqi-adapters 全部 11 个测试二进制全绿；附带修复：trash() 重构时丢失的 items.push 导致 succeeded() 恒 0 |
| AC-FLOW-012 环路（2026-09-19） | 受管目录注入通道：宿主启动时装载数据目录 `tools/managed-catalog.json`（可选；受管条目的版本/来源/校验和平台映射由该目录确定，AC-COMMON-007；解析/校验失败降级为警告不阻断）。runner 在 012 阶段以进程内 127.0.0.1 静态服务提供真实工具 zip（sha256 现算）并把清单写入共享数据目录。12-ac-flow-012 spec：catalog_query 发现 30 项 → tools_list 显示 fleqi-demo-tool 未安装（缺依赖）→ tools_install 经回环下载+SHA-256 校验+安全解压+预检+原子发布（available + installDir）→ run_submit 调用已安装的真实可执行文件（succeeded，输出 fleqi-demo-tool 1.0）→ favorites_create + 列表读回 → session_create 新会话复用同命令再次成功 → AI 摘要段按条件语义给真实反馈（无端点引导/已配置不可达时如实报错；测试环境无可用凭据，不伪造摘要）。安装进度采样取决于包大小（回环小包在首个轮询前完成），InstallProgress 的确定/不确定进度由 Rust 集成测试与工具页轮询测试覆盖，spec 仅记录采样 | 原生 1 项 + 既有 spec 全过（证据 m12-*.json：执行/收藏/复用/引导全链路落盘） |
| AC-FLOW-001/005 原生运行（2026-09-19） | 001：runner 以独立全新数据目录最先运行 10-ac-flow-001 spec——断言默认 manual/keepAll/hotkey null、输入条未显示；真实切换 Finder（驱动器激活事件）后仍不自动出现；surface_show 被拒返回"快捷键"引导文案（托盘"显示输入条"被拒引导设置的后端语义；托盘点击为原生菜单，端到端留人工清单）；hotkey_commit 真实注册后显式唤起 → 新会话 + 有效目录。005：11-ac-flow-005 spec——真实 vim（-n -u NONE -i NONE）占住前台，驱动器经"Calculator 失焦 → Finder open+activate"产生真实激活事件切换 B→C；断言忙时零 __cd__ 注入、原始键插入文本真实到达 vim 缓冲、会话 targetDirectory 落到 C；退出 vim 到安全提示符后自动同步 C（当前目录=C）。发现并修复真实产品缺陷：context_refresh IPC 只刷新上下文、不走 surface 路由（只有观察器路由）——手动刷新不更新会话目录同步、不触发 followFinder 自动显示；现统一为 AppState::refresh_context_routed 管线（观察器与命令共用）。附带：runner 事件驱动器（标志/标记文件协议）+ FLEQI_ONLY 单阶段过滤 | 原生 2 项 + 既有 9 项 spec 全过（证据 m10-*.json、m11-*.json）；发现并修复真实缺陷：TerminalSnapshot.foregroundProcess 被硬编码为 null（检测方法存在但未接线）——现接入实时 tcgetpgrp+ps 查询（005 首个真实消费方） |
| 首次可用闭环（2026-09-19） | composer 三种提示（无热键/无端点/无会话）带可点动作：`app_open_window` 扩展可选 page（settings/console 白名单路由，窗口内 eval 导航或创建时直达），无端点报 unavailable 时给"配置模型 API"直达设置模型页、无热键拒显时给"打开设置"、无会话时给"新建会话"（消费 session_create + session_select，适配器三层补齐）；会话选择器头部新增"新建会话"；控制台概览首次配置卡真实化（激活方式按 settings.activation/hotkey 显示真实状态、模型 API 按 provider_list 显示已配置数，前往按钮直达对应设置页，替换原"M2/M3 交付"死徽章）；模型页端点卡片支持"编辑"（载入记录覆盖保存）与"清除密钥"（apiKey 空串清除语义，记录保留）、新增顶层"默认模型"卡（settings.defaultModel 真实保存；规划时优先于端点记录默认生成模型，仅当该模型在端点列表内生效，附 2 项应用层测试）；规划取消 run_plan_cancel（在途请求表，模型块间生效，composer 规划中"取消规划"）；工具安装进度/取消 tools_install_status/tools_install_cancel（在途任务表 + ToolFacility 进度回调：下载字节量（确定/不确定）→ 校验 → 解压 → 发布，InstallProgress TS 绑定 72 个），工具页轮询进度 + 取消安装按钮 | vitest 33（新增概览真实状态 2 项、模型页编辑/清密钥/defaultModel 3 项、composer 引导入口与选择器新建 2 项；preview 热键注册同步 settings.hotkey、apiKey 语义对齐后端）+ Playwright 9（基线随概览卡与输入条结构更新）+ `pnpm check` 全绿 + 原生 run-a 含 AC-FLOW-009 显式用例（`!` 标记命令经宿主去标记后真实 zsh 执行输出可见；目标目录取会话真实 cwd，接受 sent/queued 两条投递路径）。发现并修复真实缺陷：endAll/session_end 主动结束时 PTY 以信号码退出（SIGHUP=129）被 TerminalEvent::Exited 按 status!=0 标为 failed，违反 FR-SESSION-005"多会话均进入结束状态"；TerminalService 现记录主动结束意图（expecting_exit），主动结束落 ended、意外退出仍标 failed，附 2 项应用层回归测试 |
| 窗口色：无边框自绘红绿灯（2026-09-19） | 三窗口去掉系统标题栏（`decorations=false` + 透明窗口 + macOSPrivateApi，CSS 按 `data-chrome="native"` 圆角 10/14），不出现白条；红绿灯由页面自绘（WindowChrome 组件）：控制台/设置三灯（红=关闭窗口销毁视图、黄=最小化、绿=缩放，经适配层 `windowControl` 下发 `core:window` 能力，capability 仅增 start-dragging/close/minimize/toggle-maximize 四项），输入条仅红点承担"隐藏输入条"（hideBehavior 合同不变）；顶部拖动带是唯一 `data-tauri-drag-region` 且不在滚动容器内——拖动窗口与内容滚动互不抢占；浏览器预览不渲染窗口条，布局与截图基线不变 | `pnpm check` 全绿 + vitest 37（新增 window-chrome 4 项：拖动区唯一/滚动容器无拖动属性回归守卫、三灯动作下发、输入条红点走 surfaceHide 不销毁窗口）+ Playwright 9 + 原生 run-a（15 用例）与 run-a2-04 全绿；像素证据 m1-console-overview.png、m2-composer-borderless.png（tests/.artifacts/desktop/）；规格落 ui-design.md §12.4，人工项（边缘缩放手感）入 verification-m4.md §4 |
| 参考对齐：贴附/条样式/结果回返（2026-09-19） | 按参考视频帧与 ui-design §4.1 纠偏三件事。①贴附：输入条从"贴 Finder 标题栏下方（盖内容）"改为**贴 Finder 窗口下缘外侧 4px、与 Finder 等宽（最小 560，删除 1120 上限）**，屏幕下缘放不下贴 Finder 内侧底部（新增平台 screen_bounds 桌面窗口边界查询）；窗口高度 88→72。②条样式按参考帧重排：内联红点（隐藏输入条）+ 历史（会话）按钮在左、AI/终端徽章 + 输入 + 终端 + 圆形提交在主行、第二行 = 真实工作目录 + 手动终端标注 + 右侧真实模型名（settings.defaultModel 或"未配置模型"）；无独立标题区——整条背景为拖动层（z-0），点空白拖动、点控件仍是控件。③结果回返（§7）：composer 订阅 run:changed，终态 run 经 run_get 上短结论气泡——成功取输出末行、失败带退出码、部分成功警示（无模型摘要时如实归纳不编造）；气泡 = 结论 + 详情（直达控制台任务页）+ 复制真实输出 + 回应回填草稿；计时按 bubbleSeconds、悬停/焦点暂停、常驻可配；主条不可见只登记未读徽标（历史按钮角标）不强行唤起；多条完成走有界队列（3） | `pnpm check` 全绿 + vitest 41（新增：runChanged 上泡/复制/回应、隐藏期未读不弹泡、runConclusion 三态归纳、提交仅通知不弹泡）+ Playwright 9（composer 基线随重排更新）+ run-a2-04 全绿；贴附几何在 spec 内断言（runner 自建 Finder 目标窗 + core:window outer_position/outer_size/scale_factor 只读能力取外框，四项检查全过：x 对齐 395/395、y 659=655+4、等宽 920/920、高 72），证据 m2-attach-bounds.json；截图 m2-composer-borderless.png。真实模型端到端的完成气泡截图依赖用户端点，留人工清单 |

M4 尚未完成（不计入完成度）：

- 扩展 105 项中的 50 个 gap 执行器补齐（逐项清单见 tests/.artifacts/ac-matrix/extended.json 与 docs/verification-m4.md §3；16 个 cond 项实现时保持真实条件反馈）。
- 人工验收清单执行（托盘点击、VoiceOver、录屏取证）：见 docs/verification-m4.md §4。

## M5 实施核验（2026-09-18，进行中）

| 工作包 | 交付 | 实际结果 |
|---|---|---|
| 发布包构建与验证 | `pnpm tauri build --bundles app` → `target/release/bundle/macos/Fleqi.app`；`pnpm verify:package` | 通过：Info.plist 键值、adhoc 签名且 `codesign --verify --deep --strict` 通过、arm64、二进制不含 WebDriver/wdio 标记、实际启动 972×685 控制台窗口、quit 后无残留进程（证据 `tests/.artifacts/package/package-evidence.json`；窗口截图因本机无屏幕录制权限跳过，已如实记录） |

M5 尚未完成（不计入完成度，按开发计划标注发行环境阻塞，不宣称已发布）：

- 签名与公证：本机无 APPLE_ID/APPLE_API_KEY 等环境变量（构建日志已如实警告）；Hardened Runtime 与 Developer ID 签名材料缺失。
- Intel（x86_64-apple-darwin）目标未安装 rustup 工具链；跨编译不替代实际运行，双架构原生记录待补。
- 性能 p95、安装/升级、更新验签（更新公钥/发布地址未配置）、Finder/Dock/关于页图标复核、源码发布材料成册。


## M1 实施核验（2026-09-18）

| 工作包 | 交付 | 实际结果 |
|---|---|---|
| M1.1 应用合同 | `fleqi-domain`：Settings（需求 §2.1 默认值与限制、SettingsPatch 双 Option 可空字段、阶段允许表、范围校验）、Revision 十进制字符串、requestId 幂等判定、Permission/PermissionRecord（撤销只由 Allowed→拒绝推导）、ContextSnapshot/PathRef（虚拟目录不猜 cwd，超限不截取）、HostState/Generation、PlatformCapabilities；`fleqi-application`：端口、PathRegistry、HostLifecycle、SettingsService（幂等回执、expectedRevision、并发重复合并、提交后广播）、PermissionService（被动/显式、单在途、迟到代际丢弃）、ContextService、AppBootstrap/Diagnostics/事件 DTO；46 个 TS 绑定 | 领域 10 项 + 应用 10 项 + DTO 6 项测试通过；`pnpm check:contracts` 一致 |
| M1.2 存储与凭据 | rusqlite 0.40.2 bundled+backup，专用写线程、WAL/外键、quick_check、首个 schema（settings/request_receipts/schema_migrations）、待迁移先 backup API 备份、迁移失败回滚；脱敏日志（字段白名单 + 秘密模式）；Keychain（security-framework，自有命名空间，不同步 iCloud，自检项清理） | 存储 4 项（真实文件重启恢复、原子冲突、损坏不覆盖、备份/回滚）、日志 2 项、Keychain 2 项（真实 Keychain）通过 |
| M1.3 宿主生命周期 | 单实例插件先于初始化；日志 → SQLite → 服务装配 → 托盘菜单（打开控制台/设置/退出）→ 按需窗口 console/settings → 无提示异步自检；关闭最后一个窗口不退出宿主；`app_quit` 进入 stopping 并排空写入；13 个 M1 命令核对窗口标签与本地 origin；`Info.plist` NSAppleEventsUsageDescription、entitlements apple-events | 真实冒烟日志顺序 host.starting → storage.ready → permissions.checked → credentials.selftest → host.state Ready；白名单 == 已注册命令 |
| M1.4 macOS 自检 | AEDeterminePermissionToAutomateTarget（仅 com.apple.finder，被动/显式）、AXIsProcessTrustedWithOptions、固定 AppleScript + NSAppleEventDescriptor 结构化解析（窗口 id/physical·virtual·desktop/目录/选区计数/选区，4 秒超时）、NSOpenPanel 只选目录（宿主主线程执行器）、系统设置 URL 明确 argv | 7 项测试通过（含真实无提示探测）；原生验收读到本机事实 |
| M1.5 最小真实 UI | 控制台（概览/权限与自检 + Finder 上下文/关于与诊断）、设置（七类外壳；外观 theme/transparency/motionMode 真实保存：保存中→已保存、失败保留草稿、冲突展示最新值与草稿；其余类别只读呈现真实生效值并标注交付阶段）；HostProvider 只消费快照、事件重拉、过期回执不覆盖 | 12 项 vitest + 6 项 Playwright（4 张基线）通过 |
| M1.6 原生验收 | `pnpm test:desktop` 两次 wdio 运行（run-b 为新进程）共 7 项：bootstrap ready/schema 1/persisted；权限被动重检为本机 TCC 事实（本机：Finder 自动化 allowed、辅助功能 allowed）；Finder 快照读到真实 `/Users/trip/Desktop/`；诊断反映存储与 Keychain；设置窗口保存并经 `settings:changed` 同步到控制台窗口；过期 expectedRevision → conflict(currentRevision=2)；同 requestId 同载荷重放不重复提交、不同载荷 conflict；新进程恢复 rev 3/light/transparency off/motion reduce；两次运行宿主进程均干净退出 | 证据：`tests/.artifacts/desktop/m1-*.json`、`m1-*.png`、`lifecycle-run-a/b.json`（`.gitignore` 忽略，运行即重生成） |

M1 补充事实与边界：

- 事件名：Tauri 事件名不允许 `.`，合同的 `settings.changed`/`permissions.changed`/`platform.changed`/`context.changed` 在传输层写作 `settings:changed` 等（`fleqi_application::dto::AppEvent::name` 为单一来源，UI 从 Rust 名称对齐，有 charset 测试）。语义与合同一致。
- 显式原生验收模式：显式权限申请会弹系统授权对话框、目录选择会弹 NSOpenPanel，无人值守运行不触发系统对话框；两条路径已实现并有单元/结构化测试，成功路径需人工在 App 内点击"显式申请"/"选择文件夹…"确认（本机权限已 allowed，取消路径经 preview 适配器与应用层测试覆盖）。
- WKWebView 事实：被完全遮挡的窗口会暂停渲染并冻结 CSS transition；状态类控件（分段单选）不依赖过渡表达选中态；原生 `<select>` 无法由 WebDriver 驱动，已改分段控件；Tailwind preflight 的 `appearance: button` 在系统深色/页面浅色不一致时绘制深色原生按钮，已统一 `appearance: none`。
- M1 启动打开控制台窗口用于查看状态；产品输入条、快捷键、随 Finder 显示均属 M2，M1 不显示也不注册。
- Playwright 内置浏览器缓存缺失时回退到本机 Google Chrome（channel），不自动下载。

## P0 实施核验（2026-09-17）

新工程位于仓库根（`package.json`、`pnpm-workspace.yaml`、`Cargo.toml`、`rust-toolchain.toml`），目录按[架构](architecture.md#2-目标工程结构与依赖方向)：`apps/desktop/`、`packages/ui/`、`packages/contracts/`、`crates/fleqi-{domain,application,adapters,platform}/`、`resources/icons/`、`tests/desktop/`、`scripts/`。实施计划见 `docs/superpowers/plans/2026-09-17-fleqi-full-implementation.md`。

| 验收 ID | 交付 | 实际结果 |
|---|---|---|
| P0-DOC-001 | `pnpm check:docs`、`pnpm check:traceability` | 8 份文档格式合规、73 个内链与锚点可达、744 处 ID 引用有定义、三组枚举与 13 个共享默认值需求/UI 一致；30 基础 CAP ↔ AC-CAP-001..030、105 个唯一 legacy_id（10/18/7/12/5/20/4/13/12/4）、AC-CAP-001..135 连续唯一且开发计划全覆盖、AC-FLOW-001..015 定义完整且被计划引用、105 个 FR/NFR 唯一。AGPL、第三方、构建与贡献说明就位 |
| P0-REPO-001 | `pnpm check:references`、`pnpm check:repo` | 83/83 项参考资产路径、字节与 SHA-256 一致（含本机视频）；`Web APP/`、`Icon/` 无未登记跟踪文件；仓库根、`origin=Fleqi-App/fleqi`、单 `main`、视频/依赖/构建产物忽略边界、参考源跟踪均通过 |
| P0-ENV-001 | rustup 1.29.1 + `rust-toolchain.toml` 锁 1.98.0（rustfmt、clippy）；corepack 激活 pnpm 10.33.4；Node 26.8.1 | `pnpm run doctor` 通过；`pnpm install --frozen-lockfile` 与 `cargo … --locked` 可复现；`pnpm check:rust`（fmt + clippy -D warnings + workspace 测试）通过 |
| P0-CONTRACT-001 | `fleqi-application::dto`：BuildInfo（productName/version/bundleIdentifier/stage/targetOs/targetArch/buildProfile/minimumMacosVersion）、AppError（code=`forbidden`/message/retryable），serde camelCase；ts-rs 12.0.1 显式导出到 `packages/contracts/src/bindings/` | `pnpm check:contracts` 临时目录逐字节比对无差异；负向测试（篡改绑定）被捕获并由 `pnpm contracts:regen` 恢复；普通 `cargo test` 不生成文件；Cargo/根 package.json/contracts 版本一致 `0.0.1-beta.1`；5 项 DTO 契约测试通过 |
| P0-BOUNDARY-001 | `pnpm check:boundaries` | crate 依赖方向（domain 纯净、application 仅 domain、adapters/platform 互不依赖）、UI 仅 `src/adapters/host/` 可 import `@tauri-apps`、宿主注册命令 == `apps/desktop/commands.allowlist.json`（P0：仅 `app_build_info`）、capability 仅授权白名单命令、无 shell 插件、CSP 无 unsafe-eval、测试驱动 feature 门控、Rust 源无 `#[ts(export)]` 自动导出 |
| P0-UI-001 | `packages/ui` 工程状态页（Vite 8 / React 19 / Tailwind v4 / Lucide）：加载/成功/失败/重试，desktop 与 preview 适配器，浏览器持续标识预览；设计与动效 token 按 UI 文档 §12.1/§13.1，reduced-motion 归零 | `pnpm typecheck`、`pnpm build` 通过；`pnpm test:ui`：7 项 vitest 行为测试（含刷新不展示旧值、过期回执不覆盖、桥错误归一）+ 4 项 Playwright（深/浅/失败截图基线、键盘可达）通过；基线更新需 `pnpm test:ui:update` |
| P0-DESKTOP-001 | `apps/desktop/src-tauri`（tauri 2.11.5）：唯一命令 `app_build_info` 核对窗口标签与本地 origin；严格 CSP、导航限制；bootstrap 窗口 920×680/最小 640×520，关窗即退出；不建库、不起 shell、不连模型、不申请权限 | `pnpm test:desktop`：`desktop-test` feature + `tauri.test.conf.json` 内联 capability 构建含内嵌 WebDriver（tauri-plugin-wdio-webdriver 1.4.0）的测试构建，`@wdio/tauri-service` embedded 驱动真实 WKWebView（webkit 605.1.15）；3 项用例通过：真实 IPC 字段 `macos/aarch64/debug/0.0.1-beta.1/P0/app.fleqi.desktop/14.0`、刷新第 2 次读取、窗口尺寸；启动 PID 与退出后无残留记录于 `tests/.artifacts/desktop/`（截图、BuildInfo、lifecycle） |
| P0-PACKAGE-001 | `pnpm tauri build --bundles app` → `target/release/bundle/macos/Fleqi.app`；`pnpm verify:package` | Info.plist：`CFBundleIdentifier=app.fleqi.desktop`、`CFBundleShortVersionString=0.0.1`、`CFBundleVersion=0.0.1`、`LSMinimumSystemVersion=14.0`；ad-hoc 签名且 `codesign --verify --deep --strict` 通过；arm64；二进制不含 WebDriver/wdio 标记；实际启动普通包，CGWindowList 记录 920×680 窗口并截图，`quit` 后无残留进程（`tests/.artifacts/package/`） |

补充事实：

- 原生测试路线为 Tauri 官方文档所述 macOS 唯一可用的 embedded WebDriver（`tauri-plugin-wdio-webdriver` + `@wdio/tauri-service driverProvider: embedded`），与架构 §12.1 “WebdriverIO embedded” 一致；未引入 `tauri-plugin-wdio`/前端 mock 插件，不 mock `app_build_info`。
- `resources/icons/` 由 `Icon/exports/Fleqi-iOS-Default-1024@1x.png` 经 tauri-cli 2.11.4 `tauri icon` 生成，来源哈希、工具版本与各输出登记在 `resources/icons/MANIFEST.json`；移动端与 Windows Store 派生物已移除。M5 仍需在 Finder/Dock/关于页复核。
- 测试证据目录 `tests/.artifacts/` 被 `.gitignore` 忽略，运行相应命令即可重新生成。
- macOS 27 下 release 保持 `strip = "none"`（工作区 Cargo.toml），待 M5 工具链矩阵验证后再评估。

## 开发准备核验（2026-09-16 起）

| 核验项 | 结果 |
|---|---|
| 参考资产清单 | 2026-09-17 复核：83 项路径、字节数与 SHA-256 均与清单一致，本地视频哈希一致。清单在 2026-09-16 的 81 项之上新增 `Icon/Windows/Fleqi-1024.png` 与 `Icon/Linux/Fleqi-1024.png`，两者是 `Icon/exports/Fleqi-iOS-Default-1024@1x.png` 的逐字节副本；平台分发制品（ICO、安装尺寸集）仍未生成。此前记录的 `Icon/macos/Fleqi.icon/icon.json` 漂移在当前工作区、HEAD 与清单之间未重现。该核验现由 `pnpm check:references` 自动执行。 |
| 归档备份 | `repository.bundle` 通过 `git bundle verify`，记录完整历史，含回退来源快照 `708a566`、归档头 `1b61525` 与并行工作区补丁。 |
| 工具链 | Node 26.8.1、Xcode 27.0、macOS 27.0 可用。2026-09-17 补齐：rustup 1.29.1 安装并由 `rust-toolchain.toml` 锁定 1.98.0（含 rustfmt、clippy，`aarch64-apple-darwin`）；pnpm 经 corepack 激活为 10.33.4，与根 package.json `packageManager` 一致。M5 的 Intel 目标（`x86_64-apple-darwin`）尚未安装。 |
| 结论 | P0 前置条件已满足并完成 P0；下一步按[开发计划](development-plan.md)进入 M1。 |

[开发合同入口](README.md) · [开发计划](development-plan.md) · [回退与备份记录](restart.md)

## 2026-09-23 后续反馈修订

已实现首次启动工具准备队列（复用已有、缺失补装、进度/取消/重试）、固定概览看板、右侧规划/执行/结果统一浮层与规划期间的红色输入状态，以及 conversionSourceHandling 原文件偏好。MP4/MOV/MKV 原生视频转换和 AI 原生 conversion 计划共用成功校验后处理原件的逻辑。相关验证与安装记录见 [试用反馈修订](refinement-2026-09-23.md)，本轮证据位于 tests/.artifacts/workflow-2026-09-23/。这些修订不改变原有 33 条条件验收与 M5 未完成的状态。

2026-09-23 第四轮补充：普通 Dock 启动/管理窗口关闭后菜单栏常驻、同名输出 uniqueName/overwrite 全链路与输入条连续尺寸更新已修订，验证见 [第四轮记录](refinement-2026-09-23.md#第四轮普通-app-启动同名处理与输入条抖动)。

2026-09-23 更新与权限（历史记录，当时尚未接通自动更新；2026-09-25 的新实现见发布流程）：普通安装包已加入新构建的 macOS 隐私授权重置流程，同一构建正常重启不重复清理，由用户自行重新授权，见[更新后的重新授权](permission-update-policy.md)。
