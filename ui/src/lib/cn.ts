/**
 * Class-name joiner — a minimal, zero-dependency replacement for
 * clsx + tailwind-merge. The desktop app's class lists are simple
 * enough that we don't need class deduplication (tailwind-merge's
 * value); cheap concat + filter is sufficient.
 *
 * Accepts any mix of strings, undefined, null, false, or objects
 * of the shape { className: condition } — the shadcn/ui idiom.
 */
export type ClassValue =
  | string
  | number
  | null
  | undefined
  | false
  | Record<string, boolean | null | undefined>

export function cn(...inputs: ClassValue[]): string {
  const parts: string[] = []
  for (const input of inputs) {
    if (!input) continue
    if (typeof input === "string" || typeof input === "number") {
      parts.push(String(input))
      continue
    }
    if (typeof input === "object") {
      for (const [key, value] of Object.entries(input)) {
        if (value) parts.push(key)
      }
    }
  }
  return parts.join(" ")
}
