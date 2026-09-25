import { BookOpen, Info, LayoutDashboard, ListChecks, Settings, ShieldCheck, Wrench } from "lucide-react";
import { AppShell } from "../../components/AppShell";
import { useHost } from "../../store/host";
import { AboutPage } from "./AboutPage";
import { LibraryPage } from "./LibraryPage";
import { OverviewPage } from "./OverviewPage";
import { PermissionsPage } from "./PermissionsPage";
import { RunsPage } from "./RunsPage";
import { ToolsPage } from "./ToolsPage";

const PAGE_LABELS: Record<string, string> = {
  overview: "概览",
  runs: "任务",
  library: "能力库",
  permissions: "权限与自检",
  tools: "工具",
  about: "关于与更新",
};

export function ConsoleShell({ page, onNavigate }: { page: string; onNavigate: (page: string) => void }) {
  const host = useHost();
  const boot = host.bootstrap;
  const select = (id: string) => {
    if (id === "settings") {
      void host.openWindow("settings");
      return;
    }
    onNavigate(id);
  };
  const footer = boot
    ? `${boot.buildInfo.version} · 测试版`
    : "正在读取宿主状态…";

  return (
    <AppShell
        fixed={page === "overview"}
        pageLabel={PAGE_LABELS[page] ?? page}
        title="Fleqi 控制台"
        current={page}
        onSelect={select}
        groups={[
          {
            label: "工作区",
            items: [
              { id: "overview", label: "概览", icon: LayoutDashboard },
              { id: "runs", label: "任务", icon: ListChecks },
              { id: "library", label: "能力库", icon: BookOpen },
            ],
          },
          {
            label: "连接与工具",
            items: [
              { id: "permissions", label: "权限与自检", icon: ShieldCheck },
              { id: "tools", label: "工具", icon: Wrench },
            ],
          },
        ]}
        bottom={{
          label: "底部",
          items: [
            { id: "about", label: "关于与更新", icon: Info },
            { id: "settings", label: "设置", icon: Settings },
          ],
        }}
        footer={footer}
      >
        {page === "overview" && <OverviewPage onNavigate={onNavigate} />}
        {page === "permissions" && <PermissionsPage />}
        {page === "about" && <AboutPage />}
        {page === "runs" && <RunsPage />}
        {page === "library" && <LibraryPage />}
        {page === "tools" && <ToolsPage />}
    </AppShell>
  );
}
