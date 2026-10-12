import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

// JSDOM 无布局观察器；弹层及提示框几何由浏览器/原生回归验证。
vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });

Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: (query: string): MediaQueryList => ({
    media: query,
    matches: query.includes("min-width") ? window.innerWidth >= Number(query.match(/\d+/)?.[0] ?? 0) : query.includes("max-width") ? window.innerWidth <= Number(query.match(/\d+/)?.[0] ?? 0) : false,
    onchange: null,
    addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => true,
  }),
});

afterEach(() => {
  cleanup();
});
