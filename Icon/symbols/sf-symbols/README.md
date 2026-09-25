# SF Symbols 导出样本

本目录记录 SVG 的获取、整理和读取方式。2026-09-15 从本机 Apple SF Symbols 应用实际导出 `gearshape.2`。

| 文件 | 内容 |
|---|---|
| `gearshape.2.svg` | “文件 → 导出符号…”生成的原始 SVG，导出选项为 Xcode 26。包含辅助线、Apple 专用标注及 Ultralight-S、Regular-S、Black-S 三个字重源。 |
| `gearshape.2.regular.svg` | 从原始文件提取的 Regular-S 单图标。保留原始路径，去掉画板和预览样式，按图形边界设置 viewBox，使用 currentColor 填充。 |

## 获取与导出

在 SF Symbols 中搜索图标名称或用途，选中符号，使用“文件 → 导出符号…”保存 `.svg`。需要可编辑的设计模板时使用“文件 → 导出模板…”。原始导出文件可能包含多个字重和说明画板，因此普通 SVG 渲染器使用前需要提取所需图形。

开发过程中，当前电脑上的自动化工具可以直接操作 SF Symbols，按需完成搜索和导出，再读取导出的本地文件。这里使用的是本地应用的导出功能，不是在线 SVG 下载 API。

## 读取

为所需图标登记稳定名称，例如 `settings` 对应 `gearshape.2.regular.svg`。App 运行时通过该名称取得随安装包发布的 SVG，由选定的 SVG 渲染器显示。Rust 可以嵌入或读取资源字节；支持 SVG 的界面层也可以直接加载资源文件。选择哪种读取入口不影响 SVG 文件本身，也不决定 UI 技术栈。

内联 SVG 的 currentColor 可以跟随所在元素的颜色；作为独立图片加载时，应由渲染器指定颜色，或使用支持着色的遮罩方式。

## 动画与平台范围

检查此次原始导出：存在 `-sfsymbols-*` 专用标注，没有 `animate`、`animateTransform`、`animateMotion` 元素或 CSS keyframes。SF Symbols 应用中的动画预览不会导出成可在通用 SVG 渲染器中直接播放的动画。跨平台的旋转、弹跳或分层动画需要在所选动画系统中另行实现。

Apple 的系统图形授权限于对应 Apple 平台。本样本用于本地导出验证及符合授权的 Apple 平台用途；Windows/Linux 发布需使用具备相应授权的图形资源。

## 验证

- 原始与整理后的文件均通过 XML 解析。
- 整理后路径数据与原始 Regular-S 路径完全一致。
- 已用 macOS Quick Look 渲染并检查单图标的外观和边界。

## 官方资料

- [导出符号与模板](https://developer.apple.com/documentation/uikit/creating-custom-symbol-images-for-your-app)
- [Apple Symbols 动画框架](https://developer.apple.com/documentation/symbols/)
- [Xcode and Apple SDKs Agreement](https://www.apple.com/legal/sla/docs/xcode.pdf)，System-Provided Images 条款。
