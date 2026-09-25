# Fleqi UI 参考工程

本目录保留工作区和“设置 → 通用”两份独立 UI，用于新 App 的视觉和组件参考。新产品需求、终端、AI、文件执行和平台实现以[开发文档](../docs/README.md)为准。

这里没有新 App 的业务运行时。源码中“通过 GitHub 登录”“侧边终端 P1”和不可操作导航是旧参考内容；新 App 采用用户自配模型 API、首版内置终端及[新的页面合同](../docs/ui-design.md)，不继续继承这些占位行为。

## 参考入口

| 文件 | 用途 |
|---|---|
| [CoreUI.tsx](CoreUI.tsx) | 仅导出 WorkspaceUI、GeneralSettingsUI 和相关类型 |
| [WorkspaceUI.tsx](src/core/WorkspaceUI.tsx) | 窗口、侧栏、六张文件能力卡片的参考实现 |
| [GeneralSettingsUI.tsx](src/core/GeneralSettingsUI.tsx) | 设置外壳、通用控件与快捷键录入 |
| [contracts.ts](src/core/contracts.ts) | 两份参考 UI 的 props/回调，不是新业务 DTO |
| [COUPLING.md](COUPLING.md) | 参考边界、保留与重建的职责 |
| [UI_INVENTORY.json](UI_INVENTORY.json) | 原始版本、复制清单、依赖与排除内容 |

截图“概览”配六张文件能力卡片是提取时的画面组合。新概览与能力库各有明确职责，见 UI 文档；静态卡片不代表相关处理已实现。

## 预览与检查

进入本目录运行：

```bash
pnpm install --frozen-lockfile
pnpm dev
```

工作区为 `http://127.0.0.1:1426/`，设置为 `http://127.0.0.1:1426/?surface=settings`；默认深色，可用 `theme=light` 查看浅色。

`pnpm build` 检查类型、UI 边界并构建；`pnpm check:boundaries` 只检查参考组件耦合；`pnpm test` 验证四个参考交互用例，并会重写 `docs/previews/` 中的预览截图。文档维护不需要重新运行截图测试。

预览容器[App.tsx](src/preview/App.tsx)使用独立 localStorage，仅演示设置值与弹窗连接。它不注册真实快捷键、不执行文件任务、不表示新 App 的跨窗口同步。

## 复用与来源

可以参考本地基础组件、样式和 motion token；新页面接入[新架构合同](../docs/architecture.md)。数据库、网络、原生 API、终端和执行策略放在新的宿主/服务模块，避免把预览状态一起搬入。

原始截图：[工作区](references/workspace.png)、[通用设置](references/settings-general.png)。
提取预览：[工作区](docs/previews/core-workspace.png)、[通用设置](docs/previews/core-settings-general.png)。
输入条与任务浮层另参考本机视频 `../docs/交互设计参考.mov`；可复核证据见[UI 文档 1.3 节](../docs/ui-design.md#13-视频证据登记)。

资源引用前核对各自来源与许可证；当前图标资产说明见[Icon](../Icon/README.md)。本轮保留 UI 源码和截图，仅更新本目录的参考说明。
