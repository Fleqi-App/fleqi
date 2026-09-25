# Fleqi 项目开发规则

## 项目事实与唯一入口

P0 与 M1 已交付，M2–M4 已实现主要路径：多步骤/队列/上下文/结束联动/恢复/本地能力入口已接通，111 个 CAP ID；扩展 105 条验收为 72 pass / 33 cond / 0 gap / 0 fail（cond 不计完整通过）。完整历史/细粒度进度等工程收尾与 M5 发行门槛未完成，不能宣称产品完整验收。当前事实只认 [docs/status.md](docs/status.md) 与 [docs/implementation-2026-09-20.md](docs/implementation-2026-09-20.md)；[docs/review-2026-09-20.md](docs/review-2026-09-20.md) 是修复前证据。旧实现归档不计入当前进度。

仓库根即新 App 的 workspace（`package.json`、`pnpm-workspace.yaml`、`Cargo.toml`、`rust-toolchain.toml`），目录按架构文档：`apps/desktop/`、`packages/ui/`、`packages/contracts/`、`crates/fleqi-{domain,application,adapters,platform}/`、`resources/`、`tests/`、`scripts/`。不为凑进度写空壳脚本，不把参考工程的结果当 App 结果。

从 [docs/README.md](docs/README.md) 进入有效开发合同（README 表格列出每份文档的唯一职责与阅读顺序：需求 → 能力 → UI/架构 → 开发计划）。来源优先级：当前用户明确要求 → 需求/能力合同 → 对应 UI/架构合同 → 参考截图、视频、代码；UI/架构不得各自改产品范围，冲突在实现前同步合同。字段默认值以 requirements.md 的“设计默认值”为唯一来源。改动合同须在同一变更中同步受影响文档与验证，不能在实现中默默改产品行为。

项目以 AGPL-3.0-only 开源；macOS 首发完整，Windows/Linux 后续经平台适配复用 Rust 与同一 Web UI。没有订阅、试用、激活、强制登录或 Fleqi 自营模型代理；用户自配 API/本地服务。实时语音下一版，文件转写/字幕/OCR 在首版。

## 参考资料的范围

- `Web APP/` 是两个页面的 UI 参考工程：它的静态导航、账号占位、localStorage 和示例回调不代表新业务；`Web APP/references/` 与 `Web APP/docs/previews/` 提供控制台和设置视觉基线。
- `Icon/` 是设计源，派生 App 图标在 `resources/icons/`（来源哈希与工具版本记于 `resources/icons/MANIFEST.json`）；除明确的参考维护任务不修改参考 UI 源码/截图/视频/图标源，新源码只放架构文档指定的目录。
- `docs/交互设计参考.mov` 被 `.gitignore` 排除、只存在于本机；画面证据用 [UI 设计](docs/ui-design.md) 已登记的时间点与观察，不要整片加载。
- 2026-09-25 按用户要求重建公开仓库，从 0.0.2 源码快照开始；旧历史保存在本地归档。历史记录中的 `1b61525`、`708a566`、`df08ee1` 属于重建前的仓库，不能在新克隆中直接检出。未经用户明确指令不恢复、移植或参照旧代码反推需求。仓库只有 `main`，远程 `origin` 为 `Fleqi-App/fleqi`，同样只保留 `main`。

## 命令与验证范围

新 App 的命令在仓库根运行。环境：Node 26.8.1、pnpm@10.33.4（corepack，`.npmrc` 设了 engine-strict）、rustup 1.29.1 分发的 Rust 1.98.0（`rust-toolchain.toml` 锁定，含 rustfmt/clippy）、Xcode 27.0 / macOS 27.0 / arm64：

```bash
pnpm install --frozen-lockfile   # 之后先跑 pnpm run doctor 核对环境
pnpm run doctor                  # Node/pnpm/rustup/组件/Xcode 自检
pnpm check                       # docs → traceability → references → repo → rust → contracts → boundaries → typecheck
pnpm build                       # 递归 UI 构建
pnpm test:ui                     # vitest + Playwright（基线更新用 pnpm test:ui:update）
pnpm test:desktop                # 原生测试构建 + embedded WebDriver，证据在 tests/.artifacts/desktop/（.gitignore 忽略，运行即重生成）
FLEQI_ONLY=run-ui pnpm test:desktop   # 只跑一组；tag 见 scripts/test-desktop.mjs 的 runs 表
pnpm tauri dev                   # 桌面开发；自动起 UI dev server（127.0.0.1:1420 strictPort）
pnpm tauri build --bundles app   # 普通包；随后 pnpm verify:package
pnpm contracts:regen             # 仅 DTO 变化时显式重新生成 packages/contracts/src/bindings 并提交
node scripts/check-capability-evidence.mjs   # 跑完真实 Rust 能力矩阵后校验 tests/.artifacts/ac-matrix/extended.json，不在 pnpm check 内
```

定向验证：`cargo test -p fleqi-adapters <name>`、`pnpm --filter @fleqi/ui exec vitest run <file>`、`pnpm --filter @fleqi/ui exec playwright test <spec>`。

工具链陷阱：本机 `PATH` 中 `/opt/homebrew/bin` 在 `~/.cargo/bin` 之前，裸 `cargo`/`rustc` 是 Homebrew 单体版、不读 `rust-toolchain.toml`。仓库脚本（doctor、check:rust）会自行前置 `~/.cargo/bin`；手工跑 Rust 命令时用 `~/.cargo/bin/cargo` 或调整 PATH，否则锁定的工具链与组件不生效。`test:desktop` 在已有测试构建进程运行时拒绝启动，数据目录每次运行隔离。

`Web APP/` 仍是参考工程，其命令只验证参考副本，不作为新 App 验收：

```bash
cd "Web APP"
pnpm install           # pnpm@10
pnpm dev               # 127.0.0.1 本地预览
pnpm build             # tsc --noEmit + check:boundaries + vite build
pnpm check:boundaries  # 参考工程 UI 边界检查
pnpm test              # Playwright，会改写 docs/previews/ 两张预览截图
```

## 实现边界（`pnpm check:boundaries` 强制）

- crate 依赖方向：domain 纯净、application 仅依赖 domain、adapters 与 platform 互不依赖；domain 不得引用 tauri/reqwest/rusqlite/portable-pty/vt100/keyring/tokio 等基础设施库。
- UI：只有 `packages/ui/src/adapters/host/` 可 import `@tauri-apps/*`；UI 源码禁用 `eval`、`new Function`、`dangerouslySetInnerHTML`（命令输出不得拼进 HTML）。
- 宿主：`generate_handler!` 注册的命令必须与 `apps/desktop/commands.allowlist.json` 完全一致，capability 只授权白名单内命令，新命令按架构 §12.5 的阶段接口启用；禁用 `tauri-plugin-shell`；webdriver 依赖必须由 `desktop-test` feature 门控；bundle id 固定 `app.fleqi.desktop`；CSP 不含 `unsafe-eval`。
- 契约：crates 内禁用 `#[ts(export)]` 自动导出（普通 `cargo test` 会改生成文件）；`packages/contracts/src/bindings/` 是生成物，不手改。

采用 Tauri 2 + Rust + React/TypeScript/Vite；UI 用 pnpm、Tailwind v4、Radix、本地组件、Lucide 和集中式 motion token。UI 页面只消费 ViewModel 并发出用户意图，宿主适配器调用 IPC；数据库、网络、凭据、工具安装、进程和权限由 Rust 服务处理。AI 一次性任务与持续 PTY 分开执行；模型和普通输出不能伪装手动用户输入，终端原始按键不经过 AI 计划管道。凭据只从环境变量或系统凭据服务（Keychain）读取；源码、示例和测试不写入可用凭据，provider 密钥不回传前端。

## 平台与测试事实（踩过坑）

- Tauri 事件名不允许 `.`：合同的 `settings.changed` 等在传输层写作 `settings:changed`，`fleqi_application::dto::AppEvent::name` 是唯一来源，UI 从 Rust 名称对齐。
- WKWebView 事实：被完全遮挡的窗口暂停渲染并冻结 CSS transition，选中态不能依赖过渡表达；原生 `<select>` 无法由 WebDriver 驱动，用分段控件；Playwright 内置浏览器缓存缺失时回退本机 Google Chrome（channel），不自动下载。
- release profile 保持 `strip = "none"`（macOS 27 工具链下精简符号会破坏 proc-macro 的 LINKEDIT），M5 工具链矩阵验证前不要改。
- 验收必须有真实状态或结果：`Web APP/` 测试只验证参考副本，截图/日志/接口存在都不算通过；人工清单项见 [docs/verification-m4.md](docs/verification-m4.md)。

## 必须保持一致的产品合同

- `activation = manual | followFinder`（默认 manual），自动显示可由设置选择，与快捷键是否绑定独立；manual 未注册快捷键时菜单引导设置。
- 当前可见会话随 Finder 自动同步目录；终端忙或编辑行非空时 pending，到安全提示符自动应用最新目录，后台会话不改。
- `hideBehavior = keepAll | endAll`（默认 keepAll）作用于主动隐藏；Finder 移动/拖动的暂时隐藏不结束会话。
- 手动再唤起新建会话；后台会话从选择器主动切回；历史继续创建关联新会话。
- 图钉只做会话列表置顶。隐藏、结束当前、结束全部和删除是不同动作；删除不删用户文件。
- `aiPolicy = yolo | readOnlyAutoConfirmChanges`（默认后者）只约束 AI；`!` 与终端面板用户输入按普通终端直通。

具体错误、状态、默认值和验收以相应开发文档为准，不在本文件复制另一份完整规范。

## 协作与验证

需求和 UI 文档工作按用户要求可分别委派给 GPT-6 Astra、ultra 子 Agent；主 Agent 负责架构、跨文档一致性和最后核验。委派必须限定文件所有权，不并发修改同一文件。

搜索优先用 rg，独立读取可并行；连续决策、共享状态和最终清理由主 Agent 完成。临时分析产物在完成后清理。格式遵循 .editorconfig：UTF-8、LF、末尾换行，2 空格缩进（Rust 用 4 空格）。

文档更新检查 ID、引用和行为一致性；App 实现运行与变更相称的类型、构建、契约、运行时和原生测试。验证 UI 时记录截图/视频依据、焦点/键盘、IME、reduced-motion 与必要的平台差异。
