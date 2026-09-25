import { X } from "lucide-react";
import { Button } from "./ui/button";
import { useHost } from "../store/host";

/** Console/settings use AppKit's real controls. The input bar's red button is a product action. */
export function TrafficLights({ variant, onClose, closeLabel = "隐藏输入条", className = "" }: {
  variant: "full" | "close"; onClose?: () => void; closeLabel?: string; className?: string;
}) {
  if (variant === "full") return null;
  return <Button variant="ghost" size="icon-sm" type="button" aria-label={closeLabel} title={closeLabel} data-testid="traffic-close" onClick={onClose} className={`traffic-light traffic-close group/chrome size-6 shrink-0 rounded-full p-0 ${className}`}>
    <X aria-hidden="true" className="size-2! stroke-[3] text-black/55 opacity-0 group-hover/chrome:opacity-100" />
  </Button>;
}

export function WindowDragLayer() {
  const host = useHost();
  if (host.adapter.kind !== "desktop") return null;
  return <div data-tauri-drag-region aria-hidden="true" data-testid="window-drag-region" className="absolute inset-0 z-0" />;
}
