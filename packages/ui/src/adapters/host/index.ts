import { createDesktopAdapter, hasTauriBridge } from "./desktop";
import { createPreviewAdapter } from "./preview";
import type { HostAdapter } from "./types";

export type { CatalogEntry, HostAdapter, HostKind, SessionList, SurfaceView, TerminalStreamEvent, WindowRole } from "./types";
export { isAppError, newRequestId, toAppError } from "./types";
export { createDesktopAdapter, hasTauriBridge } from "./desktop";
export { createPreviewAdapter, PREVIEW_BUILD_INFO, PREVIEW_DEFAULT_SETTINGS, PREVIEW_FAILURE } from "./preview";

/**
 * 选择适配器：存在 Tauri 桥则用 desktop，否则浏览器预览。
 * 预览态由 URL 参数控制模拟场景；不会从预览切换到 desktop。
 */
export function createHostAdapter(target: Window = window): HostAdapter {
  if (hasTauriBridge(target)) return createDesktopAdapter();
  const params = new URLSearchParams(target.location.search);
  const preview = params.get("preview");
  return createPreviewAdapter({ fail: preview === "fail", degraded: preview === "degraded", pick: preview === "pick" });
}
