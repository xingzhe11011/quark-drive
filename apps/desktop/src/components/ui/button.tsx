import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cn } from "../../lib/utils";

type Variant = "default" | "secondary" | "ghost" | "outline" | "danger" | "soft";
type Size = "sm" | "md" | "lg" | "icon";

const variants: Record<Variant, string> = {
  default:
    "bg-[var(--accent)] text-white shadow-[0_8px_24px_-10px_var(--accent)] hover:brightness-110 active:brightness-95",
  secondary:
    "bg-[var(--panel-2)] text-[var(--ink)] border border-[var(--line)] hover:bg-[var(--panel)]",
  ghost: "text-[var(--muted)] hover:bg-[var(--panel-2)] hover:text-[var(--ink)]",
  outline:
    "border border-[var(--line)] bg-transparent text-[var(--ink)] hover:bg-[var(--panel-2)]",
  danger:
    "bg-[var(--danger)] text-white hover:brightness-110 active:brightness-95",
  soft: "bg-[var(--accent-soft)] text-[var(--accent)] hover:brightness-105",
};

const sizes: Record<Size, string> = {
  sm: "h-8 px-3 text-xs rounded-lg gap-1.5",
  md: "h-9 px-4 text-sm rounded-xl gap-2",
  lg: "h-11 px-5 text-sm rounded-xl gap-2",
  icon: "h-9 w-9 rounded-xl",
};

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  asChild?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "default", size = "md", asChild, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        ref={ref}
        className={cn(
          "inline-flex select-none items-center justify-center font-medium transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]/40 disabled:pointer-events-none disabled:opacity-50",
          variants[variant],
          sizes[size],
          className,
        )}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";
