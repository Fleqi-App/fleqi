# 发布与自动更新

首个公开测试包为 0.0.2，macOS 14+ / Apple Silicon。版本号统一使用三段数字，Beta 渠道由构建阶段和 GitHub prerelease 标识。正式发行仍需 Developer ID 签名、公证和未完成的验收证据。

公开仓库按用户要求于 2026-09-25 重建，从当前源码快照开始。旧仓库的 Git 历史、元数据和 Actions 日志已在本地备份；文档中的旧提交编号是历史记录，不属于新仓库的公开历史。

## 本地构建

Windows 11 x64 核心修复包：在 Windows MSVC 工具链下运行 `pnpm.cmd run tauri build --bundles nsis`，产物位于 `target/release/bundle/nsis/`。随后执行 `pnpm.cmd run verify:package`：核对 x64 GUI PE、不含测试驱动，并在独立目录完成安装、覆盖、真实 IPC 启动、退出和卸载。已有 Fleqi 安装或进程时验证脚本拒绝覆盖；使用 `/NS` 避免改动快捷方式，保留应用数据。Tauri 会将安装载荷中的唯一 bundle 标记由 `UNK` 改为 `NSS`，哈希校验只允许这一已知差异，其余字节必须一致。证据为 `tests/.artifacts/package/windows-package-evidence.json`。

Windows 尚未配置代码签名和自动更新频道，本地安装包不代表正式发行；不复用下面的 macOS 更新归档。以下步骤用于 macOS 发布。

1. 完成合同、Rust、UI、原生测试与普通包验证。用户模型凭据、真实文件和本地测试证据不放入 Git 或 release。
2. 设置 `TAURI_SIGNING_PRIVATE_KEY`（签名私钥文件路径）及 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。私钥保存在仓库外，仅用户可读，不写入源码或日志。公钥在 Tauri 配置中，可公开。
3. `pnpm tauri build --bundles app --config src-tauri/tauri.release.conf.json` 产生应用及签名更新归档；随后运行 `pnpm verify:package`。
4. `node scripts/prepare-release.mjs` 生成安装 ZIP、更新归档、签名、SHA-256 清单和 `latest.json`。发布前验证版本、架构、签名、公钥和归档内容一致。
5. 将安装包、更新归档与签名附在 `v<version>` GitHub prerelease。确认匿名下载成功后，将 `latest.json` 附在固定的 `update-channel` prerelease；先发布不可变版本包，最后推进更新入口。

`latest.json` 更新入口固定为 `https://github.com/Fleqi-App/fleqi/releases/download/update-channel/latest.json`。未来版本保持同一更新公钥；遗失私钥不能为现有安装用户发出可信更新。发布脚本只准备本地产物，不自行创建 tag 或发布。

## 应用行为

启动后异步检查一次，“关于与更新”可随时重试。检查失败明确报错，不显示“已是最新”；不会自动下载或中断工作。用户选择安装后，Tauri 官方 updater 下载并验证签名，校验失败不进入安装。

安装前要求没有活动会话、规划或工具安装。macOS 在当前 App 所在文件系统暂存，拒绝路径穿越与链接条目，验证 bundle ID、版本和代码签名；替换失败恢复旧包，恢复失败保留备份路径。当前目录不可写时提示手动安装，不执行提权 shell 脚本。成功后重启，新版按[权限策略](permission-update-policy.md)执行重新授权。

测试覆盖真实本地 HTTP 下载及签名拒绝篡改、版本比较、包结构与替换、权限清理去重与失败重试。网络测试只用回环地址；生产配置仅使用 HTTPS。完整官方签名、跨系统升级及 Windows/Linux 仍属于后续发行验收。
