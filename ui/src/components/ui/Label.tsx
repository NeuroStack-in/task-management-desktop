import type { JSX } from "preact"
import { cn } from "../../lib/cn"

// JSX.IntrinsicElements['label'] carries `for` / `htmlFor`; the
// generic JSX.HTMLAttributes<HTMLLabelElement> does not. `class` is
// extracted so it doesn't re-spread through rest and wipe our styles
// (see Button.tsx).
type LabelProps = Omit<JSX.IntrinsicElements["label"], "class"> & {
  class?: string
}

export function Label({ class: cls, className, children, ...rest }: LabelProps) {
  return (
    <label
      class={cn(
        "text-xs font-medium leading-none",
        "peer-disabled:cursor-not-allowed peer-disabled:opacity-70",
        cls,
        className as string | undefined,
      )}
      {...rest}
    >
      {children}
    </label>
  )
}
