import type { JSX } from "preact"
import { cn } from "../../lib/cn"

// `class` omitted from the element attrs so it doesn't re-spread
// through rest. See Button.tsx.
type DivProps = Omit<JSX.IntrinsicElements["div"], "class"> & { class?: string }

export function Card({ class: cls, className, children, ...rest }: DivProps) {
  return (
    <div
      class={cn(
        "rounded-lg border border-border bg-card text-card-foreground shadow-sm",
        cls,
        className as string | undefined,
      )}
      {...rest}
    >
      {children}
    </div>
  )
}

export function CardHeader({ class: cls, className, children, ...rest }: DivProps) {
  return (
    <div
      class={cn("flex flex-col space-y-1.5 p-4", cls, className as string | undefined)}
      {...rest}
    >
      {children}
    </div>
  )
}

export function CardTitle({ class: cls, className, children, ...rest }: DivProps) {
  return (
    <h3
      class={cn(
        "text-base font-semibold leading-none tracking-tight",
        cls,
        className as string | undefined,
      )}
      {...(rest as JSX.IntrinsicElements["h3"])}
    >
      {children}
    </h3>
  )
}

export function CardDescription({ class: cls, className, children, ...rest }: DivProps) {
  return (
    <p
      class={cn("text-xs text-muted-foreground", cls, className as string | undefined)}
      {...(rest as JSX.IntrinsicElements["p"])}
    >
      {children}
    </p>
  )
}

export function CardContent({ class: cls, className, children, ...rest }: DivProps) {
  return (
    <div class={cn("p-4 pt-0", cls, className as string | undefined)} {...rest}>
      {children}
    </div>
  )
}

export function CardFooter({ class: cls, className, children, ...rest }: DivProps) {
  return (
    <div
      class={cn("flex items-center p-4 pt-0", cls, className as string | undefined)}
      {...rest}
    >
      {children}
    </div>
  )
}
