import type { HTMLAttributes, ReactNode } from "react";
import { Card as ShadcnCard, CardHeader, CardTitle, CardDescription, CardContent, CardAction } from "./ui/card";

export interface CardProps extends Omit<HTMLAttributes<HTMLElement>, "title"> {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}

/** 控制台卡片：圆角 14、内边距 20（ui-design.md §12.2）。 */
export function Card({ title, description, actions, children, className = "", ...rest }: CardProps) {
  return (
    <ShadcnCard className={`gap-4 py-5 shadow-none ${className}`} {...rest}>
      {(title || description || actions) && (
        <CardHeader className="gap-2 px-5">
          <div className="min-w-0">
            {title && <CardTitle className="text-sm leading-5">{title}</CardTitle>}
            {description && <CardDescription className="mt-1 text-xs leading-5">{description}</CardDescription>}
          </div>
          {actions && <CardAction className="flex items-center gap-2">{actions}</CardAction>}
        </CardHeader>
      )}
      {children && <CardContent className="px-5">{children}</CardContent>}
    </ShadcnCard>
  );
}

export function DescriptionList({ items }: { items: Array<{ label: string; value: ReactNode; mono?: boolean; field?: string }> }) {
  return (
    <dl className="grid grid-cols-[minmax(6rem,auto)_1fr] gap-x-6 gap-y-2">
      {items.map((item) => (
        <div key={item.label} className="contents">
          <dt className="text-sm text-muted-foreground">{item.label}</dt>
          <dd className={`min-w-0 break-all text-sm ${item.mono ? "font-mono" : ""}`} data-field={item.field}>
            {item.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
