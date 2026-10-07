/** 稳定 ID 仍是 finderContext。可见名称按构建目标区分，预览与 macOS 继续说 Finder。 */
export function fileManagerLabel(targetOs: string | undefined): string {
  if (targetOs === "linux") return "文件管理器";
  if (targetOs === "windows") return "资源管理器";
  return "Finder";
}
