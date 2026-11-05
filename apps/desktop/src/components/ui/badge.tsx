import type { ReactNode } from "react";
import { cn } from "../../lib/utils";

export function Badge({
  children,
  tone = "muted",
  className,
}: {
  children: ReactNode;
  tone?: "accent" | "success" | "danger" | "muted" | "warning";
  className?: string;
}) {
  const tones = {
    accent: "bg-[var(--accent-soft)] text-[var(--accent)]",
    success: "bg-emerald-500/12 text-emerald-500",
    danger: "bg-red-500/12 text-[var(--danger)]",
    warning: "bg-amber-500/14 text-amber-600 dark:text-amber-400",
    muted: "bg-[var(--panel-2)] text-[var(--muted)] border border-[var(--line)]",
  };
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[11px] font-medium",
        tones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
