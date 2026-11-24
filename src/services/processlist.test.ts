import { describe, expect, it } from 'vitest'
import type { ProcessInfo } from './bindings'
import { filterProcesses } from './processlist'

const items: ProcessInfo[] = [
  { name: 'chrome.exe', pids: [100, 220] },
  { name: 'Discord.exe', pids: [330] },
  { name: 'explorer.exe', pids: [440] },
]

describe('filterProcesses', () => {
  it('returns the list untouched for an empty query', () => {
    expect(filterProcesses(items, '')).toBe(items)
    // A box holding only spaces is still an empty box.
    expect(filterProcesses(items, '   ')).toBe(items)
  })

  it('matches a substring without regard to case', () => {
    expect(filterProcesses(items, 'CHROME').map((item) => item.name)).toEqual(['chrome.exe'])
    expect(filterProcesses(items, 'cord').map((item) => item.name)).toEqual(['Discord.exe'])
    // `.exe` matches every row but nothing else: the filter does not append
    // suffixes or guess at extensions.
    expect(filterProcesses(items, '.exe')).toHaveLength(3)
  })

  it('returns nothing when no name matches', () => {
    expect(filterProcesses(items, 'firefox')).toEqual([])
  })
})