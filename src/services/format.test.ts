import { beforeEach, describe, expect, it } from 'vitest'

import { setLocale } from '../i18n'
import {
  bytes,
  formatBytes,
  formatBytesCompact,
  formatClock,
  formatDay,
  formatDelay,
  formatNextRefresh,
  formatProbe,
  formatRate,
  formatWhen,
  nextRefreshInMs,
  probeTime,
} from './format'
import type { RefreshSchedule } from './format'

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

describe('formatProbe', () => {
  // A probe stores unix seconds, so the fixtures are in seconds too.
  const now = Date.UTC(2026, 10, 21, 12, 0, 0)
  const secondsAgo = (ms: number) => Math.floor((now - ms) / 1000)

  beforeEach(() => setLocale('zh'))

  it('converts stored seconds to the milliseconds the UI counts in', () => {
    expect(probeTime(0)).toBeNull()
    expect(probeTime(null)).toBeNull()
    expect(probeTime(undefined)).toBeNull()
    expect(probeTime(1_700_000_000)).toBe(1_700_000_000_000)
  })

  it('shows a number and how long ago it was measured', () => {
    // The two halves the user needs: was it fast, and is that still current.
    expect(formatProbe({ delay_ms: 120, at: secondsAgo(3 * 60_000) }, now)).toBe('120 ms · 3 分钟前')
    expect(formatProbe({ delay_ms: 87, at: secondsAgo(30_000) }, now)).toBe('87 ms · 刚刚')
  })

  it('shows 测不到 with its age rather than dropping the row', () => {
    // A failed probe is information: the node was tried yesterday and did not
    // answer. Hiding it would read as "never tested".
    expect(formatProbe({ delay_ms: null, at: secondsAgo(26 * 3_600_000) }, now)).toBe('测不到 · 昨天')
  })

  it('says nothing at all when there is no record', () => {
    expect(formatProbe(null, now)).toBe('')
    expect(formatProbe(undefined, now)).toBe('')
  })

  it('keeps the number even when the timestamp is missing', () => {
    // Older builds wrote no time; the measurement is still worth showing.
    expect(formatProbe({ delay_ms: 210, at: 0 }, now)).toBe('210 ms · 从未')
  })

  it('follows the current language', () => {
    setLocale('en')
    expect(formatProbe({ delay_ms: 120, at: secondsAgo(5 * 60_000) }, now)).toBe('120 ms · 5 minutes ago')
    expect(formatProbe({ delay_ms: null, at: secondsAgo(26 * 3_600_000) }, now)).toBe('No result · yesterday')
    setLocale('zh')
  })
})

describe('formatDelay', () => {
  beforeEach(() => setLocale('zh'))

  it('rounds to whole milliseconds', () => {
    expect(formatDelay(119.6)).toBe('120 ms')
  })

  it('treats a missing or nonsensical number as no result', () => {
    expect(formatDelay(null)).toBe('测不到')
    expect(formatDelay(undefined)).toBe('测不到')
    expect(formatDelay(0)).toBe('测不到')
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

describe('nextRefreshInMs', () => {
  const now = Date.UTC(2026, 10, 21, 12, 0, 0)
  const minutesAgo = (minutes: number) => now - minutes * 60_000
  // A source refreshed 10 minutes ago, on an hourly schedule: 50 minutes left.
  const sub = (patch: Partial<RefreshSchedule> = {}): RefreshSchedule => ({
    enabled: true,
    update_interval_minutes: 60,
    updated_at_ms: minutesAgo(10),
    ...patch,
  })

  it('counts down from the last successful fetch', () => {
    expect(nextRefreshInMs(sub(), now)).toBe(50 * 60_000)
  })

  it('goes negative once the interval has passed', () => {
    // Overdue is a real state - the sweep simply has not reached it yet - so the
    // number stays signed instead of clamping to zero.
    expect(nextRefreshInMs(sub({ updated_at_ms: minutesAgo(90) }), now)).toBe(-30 * 60_000)
    expect(nextRefreshInMs(sub({ updated_at_ms: minutesAgo(60) }), now)).toBe(0)
  })

  it('says nothing for a manual interval', () => {
    // `0` is what the editor stores for "only when I ask", not "every 0 minutes".
    expect(nextRefreshInMs(sub({ update_interval_minutes: 0 }), now)).toBeNull()
    expect(nextRefreshInMs(sub({ update_interval_minutes: null }), now)).toBeNull()
  })

  it('says nothing for a disabled source', () => {
    expect(
      nextRefreshInMs(sub({ enabled: false, updated_at_ms: minutesAgo(120) }), now),
    ).toBeNull()
  })

  it('says nothing when it was never fetched', () => {
    // No timestamp means no countdown to count: the row's 从未 already says it.
    expect(nextRefreshInMs(sub({ updated_at_ms: null }), now)).toBeNull()
    expect(nextRefreshInMs({ enabled: true, update_interval_minutes: 60 }, now)).toBeNull()
  })
})

describe('formatNextRefresh', () => {
  const now = Date.UTC(2026, 10, 21, 12, 0, 0)
  const minutesAgo = (minutes: number) => now - minutes * 60_000
  const sub = (patch: Partial<RefreshSchedule> = {}): RefreshSchedule => ({
    enabled: true,
    update_interval_minutes: 60,
    updated_at_ms: minutesAgo(10),
    ...patch,
  })

  beforeEach(() => setLocale('zh'))

  it('says 该刷新了 once the interval has passed', () => {
    expect(formatNextRefresh(sub({ updated_at_ms: minutesAgo(120) }), now)).toBe('该刷新了')
    // The boundary is due, matching the backend's `now - last >= interval`.
    expect(formatNextRefresh(sub({ updated_at_ms: minutesAgo(60) }), now)).toBe('该刷新了')
  })

  it('counts minutes, then hours, then days', () => {
    expect(formatNextRefresh(sub({ updated_at_ms: minutesAgo(55) }), now)).toBe('还有 5 分钟')
    expect(formatNextRefresh(sub({ update_interval_minutes: 360, updated_at_ms: minutesAgo(60) }), now)).toBe(
      '还有 5 小时',
    )
    expect(
      formatNextRefresh(
        sub({ update_interval_minutes: 7 * 24 * 60, updated_at_ms: minutesAgo(2 * 24 * 60) }),
        now,
      ),
    ).toBe('还有 5 天')
  })

  it('keeps the number readable at the unit boundaries', () => {
    // 59 s and 61 min read as whole minutes/hours rather than "还有 0 分钟".
    expect(
      formatNextRefresh(sub({ update_interval_minutes: 1, updated_at_ms: now - 59_000 }), now),
    ).toBe('还有 1 分钟')
    expect(
      formatNextRefresh(sub({ update_interval_minutes: 61, updated_at_ms: minutesAgo(0) }), now),
    ).toBe('还有 1 小时')
    // 47 h stays in hours, 48 h becomes days.
    expect(
      formatNextRefresh(sub({ update_interval_minutes: 47 * 60, updated_at_ms: now }), now),
    ).toBe('还有 47 小时')
    expect(
      formatNextRefresh(sub({ update_interval_minutes: 48 * 60, updated_at_ms: now }), now),
    ).toBe('还有 2 天')
  })

  it('explains a source that will not refresh on its own', () => {
    expect(formatNextRefresh(sub({ update_interval_minutes: 0 }), now)).toBe('只手动刷新')
    expect(formatNextRefresh(sub({ enabled: false }), now)).toBe('只手动刷新')
  })

  it('stays quiet when there is no countdown to show', () => {
    // Never fetched: the caller omits the line instead of inventing a schedule.
    expect(formatNextRefresh(sub({ updated_at_ms: null }), now)).toBe('')
  })

  it('follows the current language and spells out a single unit', () => {
    setLocale('en')
    expect(formatNextRefresh(sub({ updated_at_ms: minutesAgo(55) }), now)).toBe('in 5 minutes')
    expect(
      formatNextRefresh(sub({ update_interval_minutes: 1, updated_at_ms: now - 59_000 }), now),
    ).toBe('in 1 minute')
    expect(formatNextRefresh(sub({ updated_at_ms: minutesAgo(120) }), now)).toBe(
      'due for a refresh',
    )
    expect(formatNextRefresh(sub({ update_interval_minutes: 0 }), now)).toBe(
      'manual refresh only',
    )
    setLocale('zh')
  })
})
