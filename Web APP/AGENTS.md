# UI 参考工程维护约定

- 本目录只承载 WorkspaceUI 和 GeneralSettingsUI 及其独立预览。新产品以[文档入口](../docs/README.md)及根[AGENTS.md](../AGENTS.md)为准。
- `CoreUI.tsx` 是参考入口；核心组件只接收 props 与回调，保持现有边界检查。
- `src/preview/` 只用于检视，端口为 1426；预览存储不是新 App 的持久化。
- 图片和代码提供视觉来源；[UI 设计](../docs/ui-design.md)定义新页面语义及新增输入条、会话和终端。
- 新业务不加到本目录。维护文档可更新来源和引用；改变参考代码或截图需明确属于当前任务。
- `pnpm build`、`pnpm check:boundaries` 验证参考工程；`pnpm test` 会改写两张预览截图。文档改动只做文档验证，不能把旧测试结果当新 App 验收。
