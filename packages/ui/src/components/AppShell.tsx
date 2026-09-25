import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { Sidebar, type SidebarProps } from "./Sidebar";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "./ui/sidebar";
import { Separator } from "./ui/separator";
import { Badge } from "./Badge";
import { useHost } from "../store/host";

export function AppShell({ title, pageLabel, children, fixed = false, ...sidebar }: SidebarProps & { pageLabel: string; children: ReactNode; fixed?: boolean }) {
  const host = useHost();
  const native = host.adapter.kind === "desktop";
  const [open, setOpen] = useState(() => window.innerWidth >= 800);
  useEffect(() => {
    const media = window.matchMedia("(min-width: 800px)");
    const resize = () => setOpen(media.matches);
    media.addEventListener("change", resize);
    return () => media.removeEventListener("change", resize);
  }, []);
  return <SidebarProvider data-settings-revision={host.bootstrap?.settings.revision} open={open} onOpenChange={setOpen} className="app-shell h-full min-h-0 overflow-hidden" style={{ "--sidebar-width": "232px", "--sidebar-width-icon": "80px" } as CSSProperties}>
    <Sidebar {...sidebar} title={title} />
    <SidebarInset className="min-w-0 overflow-hidden bg-background">
      <header data-testid={native ? "window-chrome" : undefined} className="app-toolbar relative flex h-12 shrink-0 items-center gap-3 border-b border-border/60 px-4">
        {native && <div data-tauri-drag-region data-testid="window-drag-region" aria-hidden="true" className="absolute inset-0" />}
        <SidebarTrigger className="relative z-10 size-7 text-muted-foreground" aria-label={open ? "收起侧栏" : "展开侧栏"} />
        <Separator orientation="vertical" className="relative h-4!" />
        <div className="pointer-events-none relative flex items-center gap-2 text-xs text-muted-foreground"><span>{title.replace("Fleqi ", "")}</span><ChevronRight className="size-3" /><span className="text-foreground" data-testid="breadcrumb-page">{pageLabel}</span></div>
        {host.adapter.kind === "preview" && <Badge className="relative ml-auto text-[10px]" tone="warning" data-testid="host-kind" data-host-kind="preview">浏览器预览 · 数据不计原生证据</Badge>}
        {native && <span className="sr-only" data-testid="host-kind" data-host-kind="desktop">桌面宿主</span>}
      </header>
      <main data-fixed-board={fixed || undefined} className={`app-content min-h-0 min-w-0 flex-1 overscroll-contain px-7 py-6 ${fixed ? "overflow-hidden" : "overflow-y-auto"}`}><div key={sidebar.current} className={`page-transition mx-auto w-full max-w-[1080px] ${fixed ? "h-full min-h-0" : ""}`}>{children}</div></main>
    </SidebarInset>
  </SidebarProvider>;
}
