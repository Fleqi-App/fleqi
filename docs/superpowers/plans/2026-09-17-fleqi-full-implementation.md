# Fleqi 全量实施计划（P0 → M5）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从纯文档基线出发，按开发计划完成 P0 工程准备与 M1–M5 产品里程碑，使文档规划的全部内容真实可用并通过对应验收门槛。

**Architecture:** Tauri 2 + Rust workspace（domain/application/adapters/platform 分层）+ React/TS/Vite UI（pnpm workspace，packages/ui + packages/contracts），SQLite 持久化，PTY 终端（portable-pty + xterm.js），两种 AI 执行策略，AGPL-3.0-only，macOS 首发完整。

**Tech Stack:** Rust 1.98.0（rustup 锁定）、Tauri 2.11.5、React 19、TypeScript 5.9、Vite 8、Tailwind v4、Radix、Lucide、ts-rs 12、rusqlite 0.40（bundled）、portable-pty、vt100、pnpm@10.33.4、Node 26.8.1。

**Spec:** `docs/README.md`（唯一入口）→ `docs/requirements.md`、`docs/capabilities.md`、`docs/ui-design.md`、`docs/architecture.md`、`docs/development-plan.md`。所有任务的行为合同以这六份文档为准；本计划只定执行顺序、文件布局与验证，不复述产品规范。

## Global Constraints

- 版本：项目版本 `0.0.1-beta.1`（Cargo/npm 等 semver 位）；`CFBundleShortVersionString` 只写 `0.0.1`；BuildInfo.stage 承载阶段标识。
- bundle identifier：`app.fleqi.desktop`；macOS 最低版本 14.0；许可证 AGPL-3.0-only。
- 工具链：rustup + `rust-toolchain.toml` 锁 1.98.0（含 rustfmt、clippy）；Node 26.8.1；根 package.json `packageManager: pnpm@10.33.4` + 同值 engines。
- 依赖 pin（沿用归档验证矩阵，不逐 crate 各自取值）：tauri 2.11.5、tauri-build 2.6.3、rusqlite 0.40.2（bundled+backup）、ts-rs 12.0.1、reqwest 0.13.5、objc2 0.6.4 系列。release 保持 `strip = "none"`（macOS 27 工具链约束，M5 再验证替换）。
- 依赖方向：domain 不依赖 Tauri/HTTP/SQLite/React；application 只依赖 domain 与自身端口；adapters 与 platform 互不直调；UI 只消费 ViewModel 与用户意图，仅宿主适配器调 IPC；TS 公共类型从 Rust DTO 经 ts-rs 生成。
- 参考资产（`Web APP/`、`Icon/`、docs 视频与 ui-baselines）不改写；新代码放独立目录；`.editorconfig`：UTF-8/LF/末尾换行/2 空格（Rust 4 空格）。
- 检查入口全部建在新工程根：`pnpm check`（串行组织）、`check:docs`、`check:traceability`、`check:references`、`check:repo`、`check:rust`、`check:contracts`、`check:boundaries`、`typecheck`、`build`、`test:ui`、`test:desktop`、`run doctor`、`tauri`。旧 `Web APP/` 测试不运行。
- 凭据只从环境变量或密钥服务读取；源码/示例/测试不写入可用凭据字面量；密钥不落库、不进前端持久状态、不进日志。

## File Structure（P0 建立，M1–M5 增量填充）

```text
package.json / pnpm-workspace.yaml / Cargo.toml / rust-toolchain.toml   根 workspace
scripts/                          check:docs、check:traceability、check:references、check:repo、doctor 等脚本
apps/desktop/                     Tauri 壳：src-tauri（main.rs、commands、windows、permissions）、UI 宿主适配
packages/ui/                      React 状态页与后续产品 UI（Tailwind v4、Radix、Lucide、motion token）
packages/contracts/               ts-rs 生成的 TS 类型（提交产物，生成与校验分离）
crates/fleqi-domain/              实体、值对象、状态机、策略
crates/fleqi-application/         用例、端口、DTO（serde camelCase + ts-rs 导出）
crates/fleqi-adapters/            process、terminal、model、storage、工具下载
crates/fleqi-platform/            macOS 原生适配（objc2/Finder/权限/Keychain…）
resources/catalog/                能力数据、工具清单、提示词版本
resources/icons/                  经核对的发布图标
tests/                            跨模块与原生宿主验收
docs/                             六份合同（不改写合同本体，只回填事实）
```

## P0 · 工程准备（详细任务）

### Task P0-1：根 workspace 与工具链锁定（P0-ENV-001 / P0-REPO-001 一部分）

**Files:** Create `package.json`、`pnpm-workspace.yaml`、`rust-toolchain.toml`、根 `Cargo.toml`（workspace，成员 apps/desktop/src-tauri 与 crates/*）、`.npmrc`（engine-strict）。

- [ ] 根 package.json：`name: "fleqi"`、`version: 0.0.1-beta.1`、`private: true`、`packageManager: pnpm@10.33.4`、engines node `>=26.8.1` pnpm `10.33.4`；scripts 见 Global Constraints 的检查入口。
- [ ] `rust-toolchain.toml`：`[toolchain] channel = "1.98.0"`, components = ["rustfmt", "clippy"]。
- [ ] corepack 激活 pnpm@10.33.4；`pnpm install --frozen-lockfile` 可复现（首次生成锁文件后冻结）。
- [ ] `pnpm run doctor` 输出版本、组件、Xcode 与目标环境报告。
- [ ] Commit。

### Task P0-2：文档与追踪检查（P0-DOC-001 / P0-REPO-001）

**Files:** Create `scripts/check-docs.mjs`、`scripts/check-traceability.mjs`、`scripts/check-references.mjs`、`scripts/check-repo.mjs`。

- [ ] `check:docs`：六份合同互链可访问（相对链接解析）、编号枚举一致（manual/followFinder、keepAll/endAll、yolo/readOnlyAutoConfirmChanges 全文同一含义）、阶段口径一致、AGPL/第三方/构建与贡献说明就位。
- [ ] `check:traceability`：AC-CAP-001..135 连续唯一；legacy_id 恰好 105 个、唯一，各类目计数 10/18/7/12/5/20/4/13/12/4；AC-FLOW-001..015 全部存在于 requirements.md 且被开发计划引用；30 个 CAP- 基础能力各有独立 AC；FR-/NFR- ID 在需求与计划间一致。
- [ ] `check:references`：`docs/reference-assets.json` 逐项校验路径存在、字节数与 SHA-256 一致（storage=git 的项再核对 HEAD 树中哈希；本地视频项仅本机存在时不阻塞无视频环境，但要报告）。
- [ ] `check:repo`：仓库根正确、origin 为 `Fleqi-App/fleqi`、只有 `main`、忽略边界（视频/依赖/构建产物/缓存/工具记录被忽略，参考源不在忽略范围）。
- [ ] Commit。

### Task P0-3：契约 DTO 与生成校验（P0-CONTRACT-001）

**Files:** Create `crates/fleqi-domain/Cargo.toml+src/lib.rs`、`crates/fleqi-application/Cargo.toml+src/lib.rs`、`crates/fleqi-application/src/dto.rs`（BuildInfo/AppError）、`crates/fleqi-application/tests/dto_serde.rs`、`scripts/export-contracts.rs`（或 cargo test 显式 feature `export-contracts`）、`scripts/check-contracts.mjs`。

- [ ] BuildInfo：productName、version、bundleIdentifier、stage（"P0"）、targetOs、targetArch、buildProfile、minimumMacosVersion；常量受校验（编译期断言与 Cargo 元数据一致）。
- [ ] AppError：code（P0 仅 `forbidden`）、message、retryable；`Result<T, AppError>`。
- [ ] serde camelCase；ts-rs `#[ts(export)]` 显式导出至 `packages/contracts/src/bindings/`（生成物提交；普通测试不触发导出）。
- [ ] `pnpm check:contracts`：导出到临时目录后与提交产物逐字节比对，无差异才通过。
- [ ] 单元测试：serde 命名、字段完整性。`pnpm check:rust` = fmt + clippy(-D warnings, --all-targets) + `cargo test --workspace --locked`。
- [ ] Commit。

### Task P0-4：crate 边界与宿主边界检查（P0-BOUNDARY-001）

**Files:** Create `crates/fleqi-adapters/`、`crates/fleqi-platform/`（空模块骨架 + 明确 TODO 注释指向合同）、`scripts/check-boundaries.mjs`。

- [ ] 边界检查：crate 依赖方向（domain 无重依赖；application ≤ domain；adapters/platform ≤ application+domain；二者互不依赖）；`packages/ui/src` 不 import `@tauri-apps/api`（只允许宿主适配器文件）；无 `eval`/裸 shell 插件；宿主只注册 `app_build_info`。
- [ ] Commit。

### Task P0-5：UI 工程状态页（P0-UI-001）

**Files:** Create `packages/ui/`（Vite + React 19 + TS 5.9 + Tailwind v4）、`packages/ui/src/pages/StatusPage.tsx`、`packages/ui/src/adapters/host.ts`（desktop/preview 双实现）、视觉 token（ui-design.md 12.1 数值）。

- [ ] 920×680 / 最小 640×520 独立窗口页面：加载/成功/失败/重试；展示真实 BuildInfo；浏览器持续标识"浏览器预览"。
- [ ] theme dark 默认、light 为开发预览（不持久化）；reduced-motion 尊重系统；键盘可达。
- [ ] `pnpm typecheck`、`pnpm build`、`pnpm test:ui`（vitest 行为测试 + Playwright 截图基线，基线更新需显式命令）。
- [ ] Commit。

### Task P0-6：Tauri 宿主与只读 IPC（P0-DESKTOP-001）

**Files:** Create `apps/desktop/src-tauri/`（tauri.conf.json、main.rs、commands/build_info.rs、permissions）、`resources/icons/`（从 Icon/exports 核对拷入）、`tests/desktop/`（WebdriverIO embedded，独立 feature/入口）。

- [ ] `app_build_info() -> Result<BuildInfo, AppError>` 真实 IPC；CSP 仅本地资源 + IPC；禁止外部导航与新窗口；窗口 ACL 只允许 bootstrap 窗口调用该命令。
- [ ] 关窗口即退出（P0 语义）；不建数据库/shell/模型连接、不申请系统权限。
- [ ] `pnpm test:desktop`：原生测试构建加载打包本地 UI，真实读版本/平台，留截图与启动/退出证据；测试驱动仅显式测试构建启用。
- [ ] Commit。

### Task P0-7：打包（P0-PACKAGE-001）

- [ ] `pnpm tauri build --bundles app`：macOS 14 最低版本、`app.fleqi.desktop`、开发签名（ad-hoc）；核对 Info.plist（CFBundleShortVersionString=0.0.1）、签名与架构；普通包启动证据；无测试驱动。
- [ ] 更新 docs/status.md 事实回填。Commit。

## M1–M5 · 阶段计划（到达时按合同细化）

每个阶段开工前，以对应合同章节细化成与 P0 同粒度的任务表再执行；本文记录阶段门槛：

- **M1 宿主与权限底座**（architecture.md §12.2–12.4）：M1.1 应用合同（Settings/Permission/Context DTO、requestId 幂等、expectedRevision）→ M1.2 rusqlite 存储与 Keychain → M1.3 宿主生命周期（单实例、菜单、退出清理）→ M1.4 Finder/AX 权限自检（AEDeterminePermissionToAutomateTarget、AXIsProcessTrustedWithOptions、结构化 Finder 快照、NSOpenPanel）→ M1.5 最小真实 UI（构建/存储/自检/上下文页；仅 theme/transparency/motionMode 可改）→ M1.6 原生验收。门槛：真实持久化、跨窗口一致、权限各路径、损坏降级。
- **M2 入口、会话与终端**：M2.1 输入条/热键/自启动/manual|followFinder 显隐 → M2.2 会话 Registry（16 上限、置顶、结束/删除/继续）→ M2.3 portable-PTY + zsh integration + 输入租约 → M2.4 目录同步（pending/queuedLine/vt100 快照/ACK 重连）。门槛：AC-FLOW-001–007、009–011、014–015；无孤儿进程。
- **M3 AI、命令和工具**：M3.1 ProcessRunner + 两策略 → M3.2 模型端点/流式/摘要 → M3.3 工具设施（staging/校验/原子安装）→ M3.4 基础能力（AC-CAP-001–030）→ M3.5 扩展能力（AC-CAP-031–135）→ M3.6 规则/收藏/历史（AC-FLOW-012）。门槛：全部 AC-CAP 真实通过。
- **M4 完整界面与异常闭环**：输入条/浮层/结果气泡/终端/控制台/七类设置/主题/无障碍，对照 ui-design.md §15 验收矩阵。
- **M5 macOS 发布**：双架构回归、性能记录、签名公证、更新验签、安装包、图标与源码材料；发行环境阻塞项如实标注。

## Self-Review 记录

- 规格覆盖：P0 八个工作包（development-plan.md §2 P0 表）映射到 Task P0-1..P0-7（DOC/REPO 落在 P0-1/P0-2）。M1–M5 阶段门槛与 development-plan §1 表一致。
- 无占位：P0 任务均给出文件、行为与验证命令；M1–M5 明确"到达时细化"，细化件同样不允许占位。
- 类型一致性：BuildInfo/AppError 字段与 architecture.md §12.1 一致；版本/bundle/最低系统与 development-plan §2 尾段一致。
