import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Button as ShadcnButton } from "./ui/button";

type Variant = "primary" | "secondary";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  children: ReactNode;
}

export function Button({ variant = "secondary", className = "", children, type = "button", ...rest }: ButtonProps) {
  return (
    <ShadcnButton size="sm" variant={variant === "primary" ? "default" : "outline"} type={type} className={className} {...rest}>
      {children}
    </ShadcnButton>
  );
}
