import { beforeEach, describe, expect, it } from 'vitest'

import { setLocale } from '../i18n'
import { bytes, formatBytes, formatBytesCompact, formatClock, formatDay, formatRate, formatWhen } from './format'

describe('bytes', () => {
  it('treats null and undefined as zero', () => {
    // The generated contract types byte counters as `number | null`.
    expect(bytes(null)).toBe(0)
    expect(bytes(undefined)).toBe(0)
  })

  it('passes real counters through unchanged', () => {
    expect(bytes(0)).toBe(0)
    expect(bytes(1536)).toBe(1536)
  })
})

describe('formatBytes', () => {
  it('keeps small values in bytes', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(1023)).toBe('1023 B')
  })

  it('steps up through the units', () => {
    expect(formatBytes(1024)).toBe('1.0 KB')
    expect(formatBytes(1536)).toBe('1.5 KB')
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB')
    expect(formatBytes(1024 ** 4)).toBe('1.0 TB')
  })

  it('never goes past terabytes', () => {
    expect(formatBytes(1024 ** 5)).toBe('1024.0 TB')
  })

  it('accepts the null the contract can produce', () => {
    expect(formatBytes(null)).toBe('0 B')
  })
})

describe('formatRate', () => {
  it('appends a per-second suffix', () => {
    expect(formatRate(2048)).toBe('2.0 KB/s')
  })

  it('clamps negative deltas to zero instead of printing a negative rate', () => {
    // A kernel restart resets its counters, so a difference can go backwards.
    expect(formatRate(-4096)).toBe('0 B/s')
  })
})

describe('formatWhen', () => {
  const now = Date.UTC(2026, 10, 21, 12, 0, 0)

  // The words come from the dictionary now, so pin the language first.
  beforeEach(() => setLocale('zh'))

  it('says 从未 for a missing timestamp', () => {
    expect(formatWhen(null, now)).toBe('从未')
    expect(formatWhen(undefined, now)).toBe('从未')
    expect(formatWhen(0, now)).toBe('从未')
  })

  it('rounds into minutes, hours and days', () => {
    expect(formatWhen(now - 30_000, now)).toBe('刚刚')
    expect(formatWhen(now - 5 * 60_000, now)).toBe('5 分钟前')
    expect(formatWhen(now - 3 * 3_600_000, now)).toBe('3 小时前')
    expect(formatWhen(now - 26 * 3_600_000, now)).toBe('昨天')
    expect(formatWhen(now - 5 * 24 * 3_600_000, now)).toBe('5 天前')
    expect(formatWhen(now - 70 * 24 * 3_600_000, now)).toBe('2 个月前')
  })

  it('never shows a negative age for a clock that jumped back', () => {
    expect(formatWhen(now + 60_000, now)).toBe('刚刚')
  })

  it('renders a kernel timestamp as a clock time', () => {
    // sing-box hands us RFC3339 with 100ns precision and an offset.
    const at = '2026-11-22T03:55:30.4070053+08:00'
    const shown = formatClock(at, new Date(at).getTime() + 60_000)
    expect(shown).toMatch(/^\d{2}:\d{2}:\d{2}$/)
  })

  it('keeps the date when the connection started another day', () => {
    const at = '2026-11-22T03:55:30.4070053+08:00'
    const tomorrow = new Date(at).getTime() + 30 * 3_600_000
    expect(formatClock(at, tomorrow)).toMatch(/^\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)
  })

  it('shows unparsable and missing timestamps without inventing one', () => {
    expect(formatClock('')).toBe('—')
    expect(formatClock(null)).toBe('—')
    expect(formatClock('not a time')).toBe('not a time')
  })

  it('follows the current language', () => {
    setLocale('en')
    expect(formatWhen(null, now)).toBe('never')
    expect(formatWhen(now - 5 * 60_000, now)).toBe('5 minutes ago')
    setLocale('zh')
  })
})

describe('formatDay', () => {
  it('prints a calendar day with zero padding', () => {
    const at = new Date(2026, 2, 1, 23, 30).getTime()
    expect(formatDay(at)).toBe('2026-03-01')
  })

  it('returns a dash for missing or broken values', () => {
    expect(formatDay(null)).toBe('—')
    expect(formatDay(Number.NaN)).toBe('—')
  })
})

describe('formatBytesCompact', () => {
  it('drops a trailing .0 but keeps a real fraction', () => {
    expect(formatBytesCompact(107374182400)).toBe('100 GB')
    expect(formatBytesCompact(13421772800)).toBe('12.5 GB')
  })

  it('treats missing values as zero bytes', () => {
    expect(formatBytesCompact(null)).toBe('0 B')
  })
})
