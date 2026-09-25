import { FileArchive, FileText, Folder, Image, Music, ScanText } from "lucide-react";

/** 展示分类；条目、描述和数量始终来自同一宿主目录。 */
export const FILE_CATEGORIES = [
  { id: "file", title: "文件与文件夹", icon: Folder },
  { id: "zip", title: "ZIP", icon: FileArchive },
  { id: "image", title: "图片", icon: Image },
  { id: "media", title: "音频与视频", icon: Music },
  { id: "pdf", title: "PDF", icon: FileText },
  { id: "text", title: "文本与文档", icon: ScanText },
] as const;

export const categoryLabel = (id: string) => FILE_CATEGORIES.find((category) => category.id === id)?.title ?? ({ system: "系统", network: "网络", dev: "开发者", developer: "开发者", tools: "工具", calc: "计算", calculation: "计算" } as Record<string, string>)[id] ?? id;
