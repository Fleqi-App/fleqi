import { useEffect, useLayoutEffect, useState } from "react";
import { useHost } from "../store/host";

/** Keep the surface and its window space alive until its exit has been painted.
 * CSS transitions can reverse in place when the user immediately reopens it. */
export function usePresence<T>(value: T | null) {
  const host = useHost();
  const [systemReduced, setSystemReduced] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const reduced = systemReduced || host.bootstrap?.settings.motionMode === "reduce";
  const [retained, setRetained] = useState<T | null>(value);
  const [entered, setEntered] = useState(false);
  const active = value !== null;

  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => setSystemReduced(query.matches);
    query.addEventListener("change", changed);
    return () => query.removeEventListener("change", changed);
  }, []);

  useLayoutEffect(() => {
    if (value !== null) setRetained(value);
  }, [value]);

  useLayoutEffect(() => {
    if (!active) {
      setEntered(false);
      if (reduced) { setRetained(null); return; }
      // The production CSS optimizer can turn 140ms into .14s.
      const token = getComputedStyle(document.documentElement).getPropertyValue("--fleqi-motion-feedback").trim();
      const value = parseFloat(token);
      const duration = Number.isFinite(value) ? value * (token.endsWith("ms") ? 1 : 1000) : 140;
      const timer = window.setTimeout(() => setRetained(null), duration);
      return () => window.clearTimeout(timer);
    }
    if (reduced) { setEntered(true); return; }
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => setEntered(true));
    });
    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); };
  }, [active, reduced]);

  return {
    present: value ?? retained,
    phase: (active ? (entered || reduced ? "visible" : "entering") : "closing") as "visible" | "entering" | "closing",
  };
}
