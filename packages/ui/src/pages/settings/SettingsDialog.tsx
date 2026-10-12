import { useRef } from "react";
import { Dialog, DialogContent, DialogTitle } from "../../components/ui/dialog";
import { SettingsShell } from "./SettingsShell";
import type { RegisterSettingsLeaveGuard } from "../../router";

export function SettingsDialog({ page, onNavigate, onClose, registerLeaveGuard }: { page: string; onNavigate: (page: string) => void; onClose: () => void; registerLeaveGuard: RegisterSettingsLeaveGuard }) {
  const returnFocus = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const content = useRef<HTMLDivElement>(null);
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent ref={content} className="flex h-[calc(100%-4rem)] w-[calc(100%-4rem)] max-w-[1120px] gap-0 overflow-hidden rounded-2xl p-0 sm:max-w-[1120px]" onOpenAutoFocus={(event) => { event.preventDefault(); content.current?.focus(); }} onCloseAutoFocus={(event) => { event.preventDefault(); returnFocus.current?.focus(); }} onEscapeKeyDown={(event) => { if (event.isComposing) event.preventDefault(); }} aria-describedby={undefined}>
        <DialogTitle className="sr-only">设置</DialogTitle>
        <SettingsShell embedded page={page} onNavigate={onNavigate} registerLeaveGuard={registerLeaveGuard} />
      </DialogContent>
    </Dialog>
  );
}
