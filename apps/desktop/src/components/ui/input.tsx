import * as React from "react";
import { cn } from "../../lib/utils";

export const Input = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      "h-10 w-full rounded-xl border border-[var(--line)] bg-[var(--panel)] px-3.5 text-sm text-[var(--ink)] outline-none transition placeholder:text-[var(--muted)]/70 focus:border-[var(--accent)]/60 focus:ring-4 focus:ring-[var(--accent)]/10",
      className,
    )}
    {...props}
  />
));
Input.displayName = "Input";
