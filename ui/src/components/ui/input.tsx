import { Input as InputPrimitive } from "@base-ui/react/input"

import { cn } from "@/lib/utils"

function Input({ className, ...props }: InputPrimitive.Props) {
  return (
    <InputPrimitive
      data-slot="input"
      className={cn(
        "flex h-8 w-full min-w-0 rounded-md border border-border bg-background px-2.5 py-1 text-sm transition-colors outline-none",
        "placeholder:text-muted-foreground/60 selection:bg-primary selection:text-primary-foreground",
        "focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40",
        "disabled:pointer-events-none disabled:opacity-50",
        "aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/20",
        "dark:border-input dark:bg-input/30",
        className
      )}
      {...props}
    />
  )
}

export { Input }
