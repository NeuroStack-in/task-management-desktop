import type { JSX } from "preact"
import { cn } from "../../lib/cn"

type Variant = "default" | "destructive" | "outline" | "secondary" | "ghost" | "link"
type Size = "default" | "sm" | "lg" | "icon"

// Omit `class` from the element attrs so we can destructure it below
// without TypeScript complaining about the reserved word. `className`
// is also kept in the type so both Preact-style `class="…"` and
// React-style `className="…"` callers work — the props merge either
// way. Without this, `...rest` would re-spread the caller's `class`
// AFTER our cn() output, silently wiping every variant style.
type ButtonProps = Omit<JSX.IntrinsicElements["button"], "size" | "class"> & {
  variant?: Variant
  size?: Size
  class?: string
}

const base =
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium " +
  "transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring " +
  "focus-visible:ring-offset-2 focus-visible:ring-offset-background " +
  "disabled:pointer-events-none disabled:opacity-50"

const variants: Record<Variant, string> = {
  default:
    "bg-primary text-primary-foreground shadow hover:bg-primary/90 active:scale-[.98]",
  destructive:
    "bg-destructive text-destructive-foreground shadow hover:bg-destructive/90 active:scale-[.98]",
  outline:
    "border border-input bg-background hover:bg-accent hover:text-accent-foreground",
  secondary:
    "bg-secondary text-secondary-foreground hover:bg-secondary/80 active:scale-[.98]",
  ghost:
    "hover:bg-accent hover:text-accent-foreground",
  link:
    "text-primary underline-offset-4 hover:underline",
}

const sizes: Record<Size, string> = {
  default: "h-9 px-4 py-2",
  sm: "h-8 rounded-md px-3 text-xs",
  lg: "h-10 rounded-md px-6",
  icon: "h-9 w-9",
}

export function Button({
  variant = "default",
  size = "default",
  class: cls,
  className,
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      class={cn(base, variants[variant], sizes[size], cls, className as string | undefined)}
      {...rest}
    >
      {children}
    </button>
  )
}
