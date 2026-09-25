# 贡献指南

先阅读[开发合同](docs/README.md)及[项目规则](AGENTS.md)。P0 工程准备已完成（仓库根即 pnpm + Cargo workspace）；产品里程碑 M1–M5 按[开发计划](docs/development-plan.md)推进，实际进度见[实施状态](docs/status.md)。

## 环境

Node 26.8.1、pnpm 10.33.4（corepack）、rustup 分发的 Rust 1.98.0（`rust-toolchain.toml` 锁定，含 rustfmt/clippy）、Xcode 命令行工具与 macOS SDK。`pnpm install --frozen-lockfile` 后运行 `pnpm run doctor` 核对。

## 提交前

- 文档：核对 ID、引用、行为与默认值一致性；运行 `pnpm check:docs`、`pnpm check:traceability`，涉及参考资产时运行 `pnpm check:references`。格式遵循 `.editorconfig`（UTF-8、LF、末尾换行，2 空格；Rust 4 空格）。
- 代码：`pnpm check`（docs、traceability、references、repo、rust、contracts、boundaries、typecheck）；改动 UI 运行 `pnpm test:ui`（截图基线更新需显式 `pnpm test:ui:update`）；改动宿主/IPC 运行 `pnpm test:desktop`；改动 Rust DTO 后运行 `pnpm contracts:regen` 并提交生成的绑定。
- 宿主新增命令必须同步 `apps/desktop/commands.allowlist.json` 与 capability，且只在架构 §12.5 对应阶段启用；未实施模块不返回假成功。
- 凭据只从环境变量或系统凭据服务读取；源码、示例和测试不写入可用凭据。

保留 `Web APP/`、`Icon/` 和本机交互视频的既有边界；不把参考 UI 的测试当作新 App 验收。代码采用 AGPL-3.0-only；第三方资源按原许可处理，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
