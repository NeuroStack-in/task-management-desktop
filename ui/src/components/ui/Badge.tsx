import type { JSX } from "preact"
import { cn } from "../../lib/cn"

type Variant = "default" | "secondary" | "destructive" | "outline" | "success" | "warning"

type BadgeProps = Omit<JSX.IntrinsicElements["div"], "class"> & {
  variant?: Variant
  class?: string
}

// "success" and "warning" aren't in vanilla shadcn but this app has
// first-class "timer live / idle" and "Wayland limited" states that
// the two base variants can't express cleanly. Tailwind semantic
// colors (emerald / amber) render the same under light + dark.
const variants: Record<Variant, string> = {
  default:
    "border-transparent bg-primary text-primary-foreground shadow hover:bg-primary/80",
  secondary:
    "border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80",
  destructive:
    "border-transparent bg-destructive text-destructive-foreground shadow hover:bg-destructive/80",
  outline:
    "text-foreground border-border",
  success:
    "border-transparent bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  warning:
    "border-transparent bg-amber-500/10 text-amber-700 dark:text-amber-300",
}

export function Badge({
  variant = "default",
  class: cls,
  className,
  children,
  ...rest
}: BadgeProps) {
  return (
    <div
      class={cn(
        "inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-semibold",
        "transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2",
        variants[variant],
        cls,
        className as string | undefined,
      )}
      {...rest}
    >
      {children}
    </div>
  )
}
