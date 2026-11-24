/**
 * Filtering for the process picker in the rule editor.
 *
 * The picker keeps the whole list in memory (a machine has tens, not
 * thousands, of distinct executable names) and filters on every keystroke, so
 * this is a plain substring test rather than a search index. Extracted here
 * because component-level filtering is hard to unit test and easy to make
 * case-sensitive by accident.
 */
import type { ProcessInfo } from './bindings'

export function filterProcesses(items: ProcessInfo[], query: string): ProcessInfo[] {
  const needle = query.trim().toLowerCase()
  // An empty box means "show everything" — return the same array so a caller
  // can compare identity and skip a needless re-render.
  if (!needle) return items
  return items.filter((item) => item.name.toLowerCase().includes(needle))
}
