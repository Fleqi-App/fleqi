# Fleqi 图标资产与发布规划

本目录保存用户提供的 Fleqi 图标设计源和已有导出。产品范围见[开发文档](../docs/README.md)，界面使用规范见[UI 设计](../docs/ui-design.md)。本轮文档重建保留所有图像和工程内容，不表示各平台发布图标已经生成。

## 1. 现有资产

| 目录/文件 | 当前内容与用途 |
|---|---|
| `design/` | 六层设计 PNG、图层总览、最终预览及 Icon Composer 工程副本 |
| [最终预览](design/app-icon-preview.png) | 核对整体造型、配色与层级，不能按屏幕背景裁切当发布源 |
| `exports/` | 四张 1024×1024 RGBA 母版：Default、Dark、ClearDark、ClearLight |
| `macos/Fleqi.icon/` | Icon Composer 设计工程及 Assets，保存 macOS 设计源 |
| `Windows/` | 已放入 1024×1024 PNG 源图 `Fleqi-1024.png`；可分发的成品 ICO 待生成 |
| `Linux/` | 已放入 1024×1024 PNG 源图 `Fleqi-1024.png`；安装尺寸集待生成 |
| `symbols/sf-symbols/` | 本地导出验证样本及[来源说明](symbols/sf-symbols/README.md) |
| `../Web APP/public/app-icon.png` | UI 参考使用的小尺寸图标，不能替代高分辨率发布母版 |

导出文件名中的 iOS 是既有导出名称，不代表本项目已纳入移动端发布。原文件名保留以便核对来源。

`Windows/Fleqi-1024.png` 与 `Linux/Fleqi-1024.png` 是 `exports/Fleqi-iOS-Default-1024@1x.png` 的逐字节副本，同为 1024×1024 RGBA、1479239 字节，SHA-256 为 `e4abce9e8734b5f9ddbe662010007e44105e3a698fabe706887c3e9428580dcf`。按用户决定，Windows 与 Linux 直接使用该 PNG；ICO、安装尺寸集与 desktop entry 引用等分发制品仍按第 2、3 节在相应里程碑生成，不因本目录已有该图而算作已交付。

## 2. 平台交付矩阵

| 用途 | 生成依据 | 发布产物与验收 | 阶段 |
|---|---|---|---|
| macOS App | Default/Dark 母版与原始 .icon 工程 | 适用于打包工具和支持系统的 App 图标；常规 .icns 兼容资产；Finder/Dock/关于页检查 | M5 |
| macOS 菜单栏 | 单独制作的 Fleqi 单色语义图形 | template 图标，深浅菜单栏与 Retina 可辨 | M4/M5 |
| Web UI | 高分辨率母版 | 按展示密度导出适当 PNG，保留透明度，不把 128px 预览放大使用 | M4 |
| Windows | 经来源核对的母版 | 包含 16/24/32/48/64/128/256px 的 ICO，安装器/任务栏/窗口检查 | Windows 里程碑 |
| Linux | 经来源核对的母版 | 16/24/32/48/64/128/256/512px PNG 安装尺寸集与 desktop entry 引用 | Linux 里程碑 |

上述尺寸是发布设计值，具体制品随相应平台构建验证。`.icon` 是设计工程，不将其目录直接冒充 Windows/Linux 图标。

## 3. 生成与资源记录

发布构建从指定母版生成派生文件，记录源路径、源哈希、生成工具及版本、输出尺寸、平台与资源用途。派生文件放在新 App 的 `resources/icons/`，保留本目录源文件不变。

App 图标、菜单栏图标和页面语义图标各自登记，不能混用。页面默认使用 Lucide 与来源明确的自有 SVG；平台专属图形通过语义名称映射。

每个第三方资源保留来源与原许可证。项目的 AGPL-3.0-only 声明不替代第三方资源许可；SF Symbols 样本和工程内嵌图形按[现有来源说明](symbols/sf-symbols/README.md)核对适用平台。跨平台发布前给相应资源提供可分发的映射，不直接假定同一 Apple 图形可用于所有平台。

## 4. 验收

核对图标没有裁切、背景误合成、模糊放大或意外变色；在原生 Finder/Dock、菜单栏和安装包中检查实际结果。支持深浅外观的产物分别验证；生成记录与制品一一对应。没有生成的资源继续标明未生成，不能由目录存在推断已完成。
