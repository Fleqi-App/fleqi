<div align="center">
  <img src="Icon/exports/Fleqi-iOS-Default-1024@1x.png" width="132" alt="Fleqi 应用图标" />
</div>

<h1 align="center">Fleqi</h1>

<p align="center"><b>贴在 Finder 上的开源桌面命令助手</b></p>

<p align="center">
  用自然语言、<code>!</code> 手动命令与持续交互终端，在当前文件上下文里完成文件、系统、开发、网络、计算与消息任务。<br />
  输入条是日常入口，工作区是平时隐藏的管理控制台。
</p>

<p align="center">
  <a href="docs/README.md">开发文档</a> ·
  <a href="docs/status.md">当前状态</a> ·
  <a href="docs/architecture.md">架构</a> ·
  <a href="docs/development-plan.md">开发计划</a> ·
  <a href="CONTRIBUTING.md">贡献</a> ·
  <a href="LICENSE">许可</a>
</p>

<p align="center">
  <img alt="版本" src="https://img.shields.io/badge/version-0.0.1_BETA1-blue?style=flat-square" />
  <img alt="状态" src="https://img.shields.io/badge/status-development-orange?style=flat-square" />
  <img alt="许可" src="https://img.shields.io/badge/license-AGPL--3.0--only-blue?style=flat-square" />
</p>

<p align="center">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-14%2B-000000?style=flat-square&logo=apple&logoColor=white" />
  <img alt="Windows" src="https://img.shields.io/badge/Windows-planned-0078D6?style=flat-square&logo=windows&logoColor=white" />
  <img alt="Linux" src="https://img.shields.io/badge/Linux-planned-FCC624?style=flat-square&logo=linux&logoColor=black" />
</p>

<p align="center">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-000000?style=flat-square&logo=rust&logoColor=white" />
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-3178C6?style=flat-square&logo=typescript&logoColor=white" />
  <img alt="Tailwind CSS" src="https://img.shields.io/badge/Tailwind_CSS-4-06B6D4?style=flat-square&logo=tailwindcss&logoColor=white" />
</p>

<p align="center"><sub>An open-source desktop command assistant for macOS, built with Tauri 2, Rust and a shared web UI.</sub></p>

---

[下载 macOS 测试版](https://github.com/Fleqi-App/fleqi/releases/tag/v0.0.2) · [更新说明](docs/release-notes/0.0.2.md) · [发布流程](docs/releasing.md)

> **项目状态：P0/M1 已交付，M2–M4 主要路径可供内部试用，完整首版与发行验收尚未完成。** 当前版本 **0.0.2 Beta**。输入条、多会话、持续终端、AI 规划、本地能力与管理界面均已有实现；2026-09-25 修复 PDF 工具环境并加入签名更新，整理控制台界面。进度与实测范围见 [当前状态](docs/status.md) 与 [试用反馈修订](docs/refinement-2026-09-23.md)。`Web APP/` 仅为参考工程。

## 简介

Fleqi 是一个通用桌面命令助手。它在当前 Finder 的文件上下文里工作：读取当前文件夹与选中项，把自然语言请求、`!` 手动命令和交互终端都归到同一个会话里管理，并把完整记录保留下来。

日常使用时只需唤起贴在 Finder 上的输入条；需要查看记录、配置能力或管理工具时才打开工作区控制台。会话、目录上下文、任务与终端输出都是真实状态，界面不凭猜测显示成功。

## 首版范围

以下内容属于首版 macOS 交付范围；主要路径已经实现，逐项验收状态以当前状态文档为准。

- **三种输入**：自然语言（AI 计划与执行）、`!` 手动命令（直通终端）、持续交互终端（vim、REPL 等）。
- **文件能力**：六类 30 项基础能力——文件与文件夹、ZIP、图片、音视频、PDF、文本与文档。
- **操作意图**：十类 105 项历史操作意图逐条映射，合计 135 项独立验收，含系统、Git、网络、计算、消息、OCR、文件转写与字幕。
- **持续多会话**：每会话独立对话、输出、目录与生命周期，按需创建至多一个 PTY；历史可继续为关联的新会话。
- **目录跟随**：当前可见会话随 Finder 自动切换目录；终端忙碌或编辑行非空时等待安全提示符，后台会话不受影响。
- **管理与配置**：七类设置、能力库、规则、收藏、工具安装与模型与 API 配置。
- **下一版本**：实时语音；Windows 与 Linux 通过平台适配复用核心与界面。

## 工作方式

| 输入 | 路径 | 约束 |
|---|---|---|
| 自然语言 | 形成可检查的执行计划，按执行策略决定是否确认后执行 | 唯一受 AI 策略约束的通道；模型输出不能成为授权 |
| `!` 手动命令 | 首个非空字符为半角 `!` 时进入手动终端模式，命令直通当前会话的 PTY | 不调用模型、不进入 AI 确认流程 |
| 终端面板 | 方向键、Tab、Ctrl+C、Esc、粘贴与输入法按终端协议直接交给程序 | 与 AI 的提交/取消 API 完全分离 |

执行策略只有两种：`readOnlyAutoConfirmChanges`（默认，仅可信只读操作免确认）与 `yolo`（AI 操作全部免确认）。两者都只约束 AI；手动输入始终按普通终端直通。

## 技术栈

| 层 | 采用 |
|---|---|
| 桌面宿主 | Tauri 2，Rust 主进程，系统 WebView |
| 界面 | React + TypeScript + Vite、Tailwind CSS v4、Radix、Lucide、集中式 motion token |
| 终端 | `portable-pty` 与 `@xterm/xterm`，Rust 侧维护可序列化屏幕状态 |
| 存储 | SQLite（rusqlite bundled，单写入队列与版本化迁移）＋ 大输出分段文件；凭据存系统 Keychain |
| 类型 | Rust DTO 作为唯一来源，TypeScript 类型由 ts-rs 生成 |
| 结构 | Rust workspace：领域、应用编排、基础设施适配、平台适配；UI 只消费 ViewModel |

## 项目状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| M0 | 文档基线（需求、能力、UI、架构、开发计划） | 已完成 |
| P0 | 工程准备：workspace、契约生成、检查脚本、只读 IPC、打包验证 | 已完成（2026-09-17） |
| M1 | 宿主与权限底座：单实例、存储、Keychain、Finder 权限与自检 | 已完成（2026-09-18） |
| M2 | 入口、会话与终端：输入条、热键、PTY、安全目录同步 | 主要联动已修复，完整边界验收待完成 |
| M3 | AI、工具与全部能力：执行策略、模型、工具安装、135 项验收 | 主要阻塞已修复；扩展 72 通过、33 条件项 |
| M4 | 完整界面与异常闭环 | UI 与主要操作闭环已更新，完整验收未完成 |
| M5 | macOS 发布验收：双架构、性能、签名与公证 | 普通包已有历史验证，发行验收未完成 |

主要执行调度、上下文与本地能力接线阻塞已修复，111 个能力入口已接通；完整首版仍有工程收尾、33 条扩展条件验收和发行门槛。`Web APP/` 仅作视觉参考。详见[当前状态](docs/status.md)与[实现与修复记录](docs/implementation-2026-09-20.md)。

## 仓库结构

| 路径 | 说明 |
|---|---|
| `docs/` | 开发合同：需求与验收、能力台账、UI 设计、架构与接口、开发计划 |
| `apps/desktop/` | Tauri 桌面宿主：窗口、IPC 注册（白名单 `commands.allowlist.json`）、capability、打包配置 |
| `packages/ui/` | React/TypeScript Web UI（控制台、设置与输入条），desktop/preview 宿主适配器 |
| `packages/contracts/` | 从 Rust DTO 经 ts-rs 生成的 TypeScript 契约类型 |
| `crates/` | `fleqi-domain`、`fleqi-application`、`fleqi-adapters`、`fleqi-platform` |
| `resources/icons/` | 由 `Icon/` 母版生成的 App 图标派生文件与生成记录 |
| `tests/desktop/` | macOS 原生验收（WebdriverIO embedded） |
| `scripts/` | doctor 与 `pnpm check` 各项门禁脚本 |
| `Web APP/` | 已有两个控制台页面的 UI 参考源码与构建入口；不是新 App 的实现 |
| `Icon/` | 图标设计源、1024px 母版与各平台发布资源规划 |
| `AGENTS.md` | 工作区协作规则、实现边界与产品合同要点 |

本地交互视频 `docs/交互设计参考.mov` 按用户决定只保留在本机，不随仓库分发。

## 文档

| 文档 | 负责 |
|---|---|
| [文档入口](docs/README.md) | 阅读顺序、来源优先级、编号与变更方式 |
| [需求与验收合同](docs/requirements.md) | 产品范围、行为、设计默认值、非功能要求 |
| [能力台账](docs/capabilities.md) | 30 项基础能力与 105 项操作意图的输入输出与验收 |
| [UI 与交互设计](docs/ui-design.md) | 界面结构、状态、焦点、视觉与动效 |
| [架构与接口合同](docs/architecture.md) | 模块、端口、数据模型、IPC、存储与平台适配 |
| [开发顺序与验证计划](docs/development-plan.md) | 里程碑、工作包、必测场景与检查命令 |

## 参与贡献

开始之前请阅读[贡献指南](CONTRIBUTING.md)与[项目规则](AGENTS.md)。文档修改需要核对 ID、引用、行为与默认值的一致性；实现工作以开发计划中的工作包为单位，验收必须基于真实状态与结果，不能以模型摘要、静态文案或浏览器预览替代。

## 许可

项目采用 **AGPL-3.0-only**，全文见 [LICENSE](LICENSE)。第三方库与参考图形的来源与许可记录在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)；正式开发开始后随实际依赖与锁文件补齐。

<div align="center">
  <sub>Fleqi · 0.0.2 Beta</sub>
</div>
