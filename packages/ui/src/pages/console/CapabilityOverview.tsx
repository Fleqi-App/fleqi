import { useEffect, useState } from "react";
import { ArrowRight } from "lucide-react";
import { Button as TileButton } from "../../components/ui/button";
import { InlineStatus } from "../../components/InlineStatus";
import { isAppError, type CatalogEntry } from "../../adapters/host";
import { useHost } from "../../store/host";
import { FILE_CATEGORIES } from "./catalog";

export function CapabilityOverview({ onNavigate }: { onNavigate: (page: string) => void }) {
  const host = useHost();
  const [entries, setEntries] = useState<CatalogEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    void host.adapter.catalogQuery().then((list) => {
      if (current) setEntries(list);
    }, (reason: unknown) => { if (current) setError(isAppError(reason) ? reason.message : String(reason)); });
    return () => { current = false; };
  }, [host.adapter]);

  return <section aria-label="文件处理能力" className="overview-capabilities flex min-h-0 flex-col gap-3">
    <div>
      <h3 className="text-xl font-semibold leading-7">文件处理能力</h3>
      <p className="mt-1 text-sm text-muted-foreground">选好文件，开始处理。</p>
    </div>
    {error ? <InlineStatus tone="error">{error}</InlineStatus> : entries === null ? <InlineStatus tone="loading">正在读取能力目录…</InlineStatus> : <div className="overview-capability-grid grid min-h-0 flex-1 grid-cols-3 grid-rows-2 gap-3">
      {FILE_CATEGORIES.map(({ id, title, icon: Icon }) => {
        const group = entries.filter((entry) => entry.category === id);
        return <TileButton variant="outline" key={id} type="button" onClick={() => onNavigate(`library?category=${id}`)} data-testid="capability-category" className="overview-capability group flex h-auto min-h-0 flex-col items-stretch justify-center whitespace-normal rounded-xl bg-card p-4 text-left shadow-none">
          <div className="flex items-center justify-between"><Icon className="size-5 text-foreground" aria-hidden="true" /><ArrowRight className="size-4 text-muted-foreground" aria-hidden="true" /></div>
          <div className="mt-2 flex items-center justify-between gap-2"><h4 className="truncate font-medium">{title}</h4><span className="shrink-0 text-xs text-muted-foreground">{group.length} 项</span></div>
        </TileButton>;
      })}
    </div>}
  </section>;
}
