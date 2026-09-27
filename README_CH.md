<div align="center">
  <img src="resources/icons/128x128@2x.png" width="132" alt="Fleqi 应用图标" />
</div>

# Fleqi

**贴近 Finder 的开源桌面命令助手。** Fleqi 在当前 Finder 文件夹和选中项的上下文中处理自然语言请求、手动命令与持续交互终端。日常入口是输入条；工作区用于查看会话、任务、能力与设置。用户自行配置模型 API 或本地服务，无需 Fleqi 账号或订阅。

[English](README_EN.md) · [语言入口](README.md)

[下载 0.0.2 Beta（macOS 14+，Apple Silicon）](https://github.com/Fleqi-App/fleqi/releases/tag/v0.0.2) · [更新说明](docs/release-notes/0.0.2.md) · [当前进度与验收](docs/status.md)

## 可以做什么

- **自然语言任务**：生成可检查的计划，按用户选择的 AI 策略确认或执行；模型与密钥由用户配置。
- **手动命令和终端**：以 `!` 提交命令，或在持续终端中使用 shell、vim、REPL 等交互程序；这两种用户输入直通终端。
- **Finder 与多会话**：可见会话跟随 Finder 目录，忙碌或存在未提交输入时等待安全时机再同步；每个会话保留自己的目录、记录和终端。
- **本地能力与管理**：文件、ZIP、图片、音视频、PDF、文本与文档处理，以及能力库、规则、收藏、工具和设置。具体范围与验收条件见[能力台账](docs/capabilities.md)和[当前进度](docs/status.md)。

首个公开测试版面向 macOS。Windows、Linux 和实时语音属于后续版本；完整首版的验收情况以 [docs/status.md](docs/status.md) 为准。

## 界面与参考资料

[工作区参考截图](Web%20APP/docs/previews/core-workspace.png) · [通用设置参考截图](Web%20APP/docs/previews/core-settings-general.png) · [UI 与交互设计](docs/ui-design.md)

以上截图来自 `Web APP/` 视觉参考工程，并非当前 App 的运行截图。输入条与任务浮层的参考依据、时间点和观察记录见 [UI 设计](docs/ui-design.md#13-视频证据登记)；本机参考视频不随仓库分发。图标展示使用仓库中的 [App 图标派生资源](resources/icons/MANIFEST.json)，设计源见 [Icon/README.md](Icon/README.md)。

## 开发入口

仓库根目录是 Tauri 2、Rust、React/TypeScript 与 Vite 的工作区。macOS 开发环境与完整命令见[贡献指南](CONTRIBUTING.md)和[开发文档入口](docs/README.md)。常用命令：

```bash
pnpm install --frozen-lockfile
pnpm run doctor
pnpm check
pnpm tauri dev
```

`pnpm test:ui` 验证界面，`pnpm test:desktop` 验证原生 App；打包与签名更新流程见[发布文档](docs/releasing.md)。`Web APP/` 的预览和测试只适用于参考工程。

## 文档与仓库

| 入口 | 内容 |
|---|---|
| [需求与验收](docs/requirements.md)、[能力台账](docs/capabilities.md) | 产品行为、默认值、能力输入输出与验收条件 |
| [UI 设计](docs/ui-design.md)、[架构与接口](docs/architecture.md) | 界面状态、交互、模块与数据边界 |
| [开发计划](docs/development-plan.md)、[当前进度](docs/status.md) | 实施与验证要求、现有结果及剩余工作 |
| [桌面宿主](apps/desktop/)、[Web UI](packages/ui/)、[Rust crates](crates/)、[类型契约](packages/contracts/) | 当前 App 源码；Rust DTO 是类型来源 |
| [参考 UI](Web%20APP/README.md)、[图标设计源](Icon/README.md) | 视觉参考资料；不代表 App 功能验收 |

## 提交目标与内容要求

变更以 `main` 为目标分支。提交或发起面向 `main` 的 PR 时，按改动内容一并更新对应文件：

| 改动 | 提交内容 |
|---|---|
| README 文案 | 同步更新 `README_CH.md` 与 `README_EN.md`，保持 `README.md` 的双语入口可用；不恢复项目状态章节，进度只链接到[当前状态](docs/status.md)。 |
| 产品行为或界面 | 同步受影响的[需求、能力、UI、架构与验收文档](docs/README.md)，保持 ID、默认值和引用一致。 |
| App 实现或 Rust DTO | 提交对应源码，并在 PR 中说明相称的验证结果；DTO 变化时运行 `pnpm contracts:regen`，一并提交[生成的 TypeScript 绑定](packages/contracts/src/bindings/)。 |
| 图片、参考资料或发行信息 | 只引用已入库、可访问的图片与资料；图标派生文件同步[来源记录](resources/icons/MANIFEST.json)，版本与发行事实同步[发布文档](docs/releasing.md)及[当前进度](docs/status.md)。 |

提交前的具体检查见[贡献指南](CONTRIBUTING.md)。

## 许可

Fleqi 采用 [AGPL-3.0-only](LICENSE)；第三方来源与许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
