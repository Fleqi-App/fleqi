/**
 * 哈希路由：宿主以 `index.html#/<窗口角色>[/<页面>]` 注入角色；浏览器预览默认控制台。
 */
export type WindowRoute = "console" | "settings" | "composer";
export type ConsolePage = "overview" | "runs" | "library" | "permissions" | "tools" | "about";
export type SettingsPage = "general" | "appearance" | "models" | "permissions" | "files" | "tasks" | "about";

export interface Route {
  window: WindowRoute;
  page: string;
  settingsPage?: string;
}

export const CONSOLE_DEFAULT: ConsolePage = "overview";
export const SETTINGS_DEFAULT: SettingsPage = "appearance";

export function parseRoute(hash: string): Route {
  const parts = hash.split("?")[0]!.replace(/^#\/?/, "").split("/").filter(Boolean);
  if (parts[0] === "settings") return { window: "console", page: CONSOLE_DEFAULT, settingsPage: settingsPage(parts[1]) };
  const window: WindowRoute = parts[0] === "composer" ? "composer" : "console";
  const allowed = ["overview", "runs", "library", "permissions", "tools", "about"];
  const page = parts[1] && allowed.includes(parts[1]) ? parts[1] : CONSOLE_DEFAULT;
  const settings = new URLSearchParams(hash.split("?")[1]).get("settings");
  return { window, page, ...(window === "console" && settings !== null ? { settingsPage: settingsPage(settings) } : {}) };
}

function settingsPage(page: string | undefined): string {
  return page && ["general", "appearance", "models", "permissions", "files", "tasks", "about"].includes(page) ? page : SETTINGS_DEFAULT;
}

export function settingsHash(hash: string, page: string | null): string {
  const [base, query] = hash.startsWith("#/console") ? hash.split("?") : ["#/console/overview"];
  const params = new URLSearchParams(query);
  if (page === null) params.delete("settings");
  else params.set("settings", settingsPage(page));
  return `${base}${params.size ? `?${params}` : ""}`;
}

export function routeHash(window: WindowRoute, page: string): string {
  return `#/${window}/${page}`;
}
