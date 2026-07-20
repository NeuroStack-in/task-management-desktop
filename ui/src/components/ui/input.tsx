import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

/**
 * Text field, matching Button's border/ring register so the login form reads as one control
 * group. shadcn's own Input, trimmed to what this panel uses.
 */
function Input({ className, type, ...props }: ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "flex h-8 w-full min-w-0 rounded-md border border-border bg-background px-2.5 py-1 text-sm",
        "transition-[color,box-shadow] outline-none selection:bg-primary selection:text-primary-foreground",
        "placeholder:text-muted-foreground/60",
        "focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40",
        "disabled:pointer-events-none disabled:opacity-50",
        "aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/20",
        "dark:border-input dark:bg-input/30",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
