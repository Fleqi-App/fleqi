# 2026-09-16 文档起点重建

用户要求放弃当前实现，回到开发文档已确定、正式开发未开始的状态。本次没有继续 UI 修改或重新构建 App。

## Git 核对结果

GitHub 仓库为 `Fleqi-App/fleqi`。回退前远程只有 `main`，提交为 `27c831a9d5b6be95e4549562b7ab8bfce96b422b`，没有标签。最早提交 `22902b928bd8c043fb93349a47db96defb76fd84` 已包含 P0/M1 代码；本地分支、reflog 与可恢复 Git 对象中也没有纯文档提交。

另外在本地 Git 的工作快照中找到更早的文档树 `708a566fe654e98b371e78fc4c503382c8d694f2`（`refs/codex/turn-diffs/captures/1789467898080/145c5f75-cc67-4f9f-b4a2-fb93335c4beb/base`）。该快照包含最初的六份开发合同，没有后加的 GitHub 登录需求与 UI 保持决定，但也已经含有工程代码。

因此本次以该早期快照的文档为来源，重建**纯文档状态**，不是不存在的零开发历史提交的原样检出。通过新的回退提交更新主线，保留全部历史，不强推改写历史。

## 保留与归档

恢复早期的需求、能力台账、UI 设计、架构、开发计划及入口六份合同；后加的 GitHub 登录合同和开发期 UI 决定随旧实现归档。清除旧实施进度、自动连续开发指令与失效工程入口。原始 UI、图标、截图和本地视频保持不变。

正式 App 的 `apps/`、`crates/`、`packages/`、`resources/`、`scripts/`、`tests/`、CI、工具链配置及依赖清单从当前工作区移出；本地 `target/`、依赖缓存、日志和旧实施文档一并归档。没有删除用户文件、App 用户数据库、Keychain 项、GitHub 组织或 App。

回退时保留了旧开发备份分支 `Beta/archive-before-docs-reset-20260916`（提交 `1b61525`，包含中断前的两处未提交修改）与文档重启分支 `Beta/docs-restart`；两者已在当天的分支整理中并入 `main`，见下文。

本机完整备份目录：`/Users/trip/TRUE 开发/Fleqi-backups/20260916-before-docs-restart`。其中 `repository.bundle` 保存全部分支历史，`parallel-worktree.patch` 与 `parallel-untracked/` 保存旧并行工作区尚未提交的内容；旧并行工作区保持原样。

下一轮从 [P0 工程准备](development-plan.md) 开始，当前实现与验收均为未开始。此次回退后停留在文档状态。

## 分支整理（2026-09-16）

按用户要求把仓库收敛为单一 `main`。归档头 `1b61525` 通过合并提交 `df08ee1` 以 `ours` 策略并入主线：只记录祖先关系，不把旧实现代码树带回工作区（合并前后 `main` 的 tree 一致）。此后本地四个侧枝（`Beta/archive-before-docs-reset-20260916`、`Beta/docs-restart`、`Beta/ui-cleanup-0.1.1` 与并行工作区分支）及远程 `Beta/*` 引用删除；失效的并行工作区登记一并清理，其未提交内容仍在本地备份中。

归档提交仍可按哈希检出（例如 `git checkout 1b61525`），完整分支与历史另存于本机备份目录的 `repository.bundle`。当前开发入口只有 `main` 的 `docs/`。
