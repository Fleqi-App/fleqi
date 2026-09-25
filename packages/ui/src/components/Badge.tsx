import type { HTMLAttributes, ReactNode } from "react";
import { Badge as ShadcnBadge } from "./ui/badge";

type Tone = "neutral" | "success" | "warning" | "error";

const tones: Record<Tone, string> = {
  neutral: "border-border text-muted-foreground",
  success: "border-success/50 text-success",
  warning: "border-warning/60 text-warning",
  error: "border-error/60 text-error",
};

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: Tone;
  children: ReactNode;
}

export function Badge({ tone = "neutral", children, className = "", ...rest }: BadgeProps) {
  return (
    <ShadcnBadge
      variant="outline"
      className={`inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs leading-4 ${tones[tone]} ${className}`}
      {...rest}
    >
      {children}
    </ShadcnBadge>
  );
}
