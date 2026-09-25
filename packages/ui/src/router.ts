/**
 * 哈希路由：宿主以 `index.html#/<窗口角色>[/<页面>]` 注入角色；浏览器预览默认控制台。
 */
export type WindowRoute = "console" | "settings" | "composer";
export type ConsolePage = "overview" | "runs" | "library" | "permissions" | "tools" | "about";
export type SettingsPage = "general" | "appearance" | "models" | "permissions" | "files" | "tasks" | "about";

export interface Route {
  window: WindowRoute;
  page: string;
}

export const CONSOLE_DEFAULT: ConsolePage = "overview";
export const SETTINGS_DEFAULT: SettingsPage = "appearance";

export function parseRoute(hash: string): Route {
  const parts = hash.split("?")[0]!.replace(/^#\/?/, "").split("/").filter(Boolean);
  const window: WindowRoute = parts[0] === "settings" ? "settings" : parts[0] === "composer" ? "composer" : "console";
  const allowed = window === "settings"
    ? ["general", "appearance", "models", "permissions", "files", "tasks", "about"]
    : ["overview", "runs", "library", "permissions", "tools", "about"];
  const fallback = window === "settings" ? SETTINGS_DEFAULT : CONSOLE_DEFAULT;
  const page = parts[1] && allowed.includes(parts[1]) ? parts[1] : fallback;
  return { window, page };
}

export function routeHash(window: WindowRoute, page: string): string {
  return `#/${window}/${page}`;
}
