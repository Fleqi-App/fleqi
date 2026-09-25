import type { MotionMode, Theme } from "@fleqi/contracts";

/**
 * 外观应用：产品设置（theme/transparency/motionMode）生效到根元素；
 * `?theme=light` 只是宿主快照到达前的开发预览默认，不持久化。
 * theme=system 跟随 prefers-color-scheme；motionMode=reduce 只能"更少"，不能忽略系统减少动态。
 */
export type ResolvedTheme = "dark" | "light";

export function resolveTheme(search: string): ResolvedTheme {
  return new URLSearchParams(search).get("theme") === "light" ? "light" : "dark";
}

export function resolveProductTheme(theme: Theme, prefersDark: boolean): ResolvedTheme {
  if (theme === "system") return prefersDark ? "dark" : "light";
  return theme;
}

export interface Appearance {
  theme: Theme;
  transparency: boolean;
  motionMode: MotionMode;
}

export function applyTheme(theme: ResolvedTheme, root: HTMLElement = document.documentElement): void {
  root.dataset.theme = theme;
  root.classList.toggle("dark", theme === "dark");
}

export function applyAppearance(appearance: Appearance, root: HTMLElement = document.documentElement, matchMedia = window.matchMedia): void {
  const prefersDark = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)").matches : true;
  applyTheme(resolveProductTheme(appearance.theme, prefersDark), root);
  root.dataset.transparency = appearance.transparency ? "on" : "off";
  root.dataset.motion = appearance.motionMode;
}
