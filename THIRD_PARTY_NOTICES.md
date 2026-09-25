# 第三方来源与许可

Fleqi 计划采用 AGPL-3.0-only，文本见 [LICENSE](LICENSE)。第三方库与参考图形保留其原许可。

原始参考资料：

- `Web APP/` 中的 UI 参考、组件和样式，来源见其 [README](Web%20APP/README.md) 与 [组件清单](Web%20APP/UI_INVENTORY.json)。
- `Icon/` 中的设计源与图形，来源及平台限制见 [图标说明](Icon/README.md) 和 [SF Symbols 说明](Icon/symbols/sf-symbols/README.md)。

新 App 已使用 Tauri、React、Radix、Lucide、xterm 及 Rust 库，实际版本以 `Cargo.lock` 与 `pnpm-lock.yaml` 为准，不能沿用旧备份的依赖清单。正式分发前仍需生成并核对完整直接/传递依赖许可清单。

2026-09-20 本轮新增或扩展的运行时依赖（许可取自已锁定包的 Cargo.toml）：

| 包 | 锁定版本 | SPDX 许可 | 用途 |
|---|---|---|---|
| quick-xml | 0.42.0 | MIT | DOCX XML 正文解析 |
| image | 0.25.10 | MIT OR Apache-2.0 | 扩展 GIF/ICO/TIFF/BMP 编解码 |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | 运行时临时转换与失败清理 |

外部工具（如 ffmpeg、qpdf、Tesseract、ImageMagick、Whisper、Homebrew）通过本机检测路径或受管清单调用；调用不意味着该工具已经随 Fleqi 捆绑分发。受管分发应随实际来源补齐相应许可。

2026-09-23：`packages/ui/src/components/ui/` 与 `src/hooks/use-mobile.ts` 由 [shadcn/ui 官方 CLI 与 registry](https://github.com/shadcn-ui/ui) 引入（new-york / Radix 风格，MIT），并适配 Fleqi 的主题、桌面侧栏宽度、可访问名称和设置边界。原许可保留于 `packages/ui/src/components/ui/LICENSE.md`。新增 class-variance-authority、cn、tw-animate-css 的实际版本以 pnpm 锁文件为准。
