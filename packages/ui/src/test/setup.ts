import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

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
