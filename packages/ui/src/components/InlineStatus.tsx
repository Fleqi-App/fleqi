import type { ReactNode } from "react";
import { AlertTriangle, CheckCircle2, CircleHelp, Loader2, XCircle } from "lucide-react";

export type StatusTone = "neutral" | "success" | "warning" | "error" | "loading";

const ICONS: Record<StatusTone, typeof CheckCircle2> = {
  neutral: CircleHelp,
  success: CheckCircle2,
  warning: AlertTriangle,
  error: XCircle,
  loading: Loader2,
};

const COLORS: Record<StatusTone, string> = {
  neutral: "text-muted-foreground",
  success: "text-success",
  warning: "text-warning",
  error: "text-error",
  loading: "text-muted-foreground",
};

/** 状态同时使用图形、文字和颜色（ui-design.md §11.4）。 */
export function InlineStatus({ tone, children, className = "", ...rest }: { tone: StatusTone; children: ReactNode; className?: string } & Record<string, unknown>) {
  const Icon = ICONS[tone];
  return (
    <span data-slot="inline-status" data-tone={tone} className={`inline-flex items-center gap-1.5 text-sm ${COLORS[tone]} ${className}`} {...rest}>
      <Icon aria-hidden="true" data-slot={tone === "loading" ? "spinner" : undefined} className="size-4 shrink-0" />
      <span>{children}</span>
    </span>
  );
}

export function EmptyState({ title, description, action }: { title: string; description?: string; action?: ReactNode }) {
  return (
    <div data-slot="empty-state" className="rounded-[14px] border border-dashed border-border p-6 text-center">
      <p className="text-sm font-medium">{title}</p>
      {description && <p className="mt-1 text-xs text-muted-foreground">{description}</p>}
      {action && <div className="mt-3 flex justify-center">{action}</div>}
    </div>
  );
}
