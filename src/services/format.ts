/**
 * Byte and rate formatting.
 *
 * Kept away from the contract layer so it can be unit-tested without touching
 * the Tauri bridge, and so every view formats numbers the same way.
 */
import { t } from '../i18n'

/**
 * Byte counters cross the IPC boundary as `number | null` (JSON has no NaN, so
 * the generated types are honest about it). Everything that does arithmetic
 * funnels through here instead of sprinkling `?? 0` around.
 */
export function bytes(value: number | null | undefined): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

/** `1.4 MB`, `812 B`, … — never more than one decimal place. */
export function formatBytes(value: number | null | undefined): string {
  const size = bytes(value)
  if (size < 1024) return `${Math.round(size)} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let scaled = size / 1024
  let unit = 0
  while (scaled >= 1024 && unit < units.length - 1) {
    scaled /= 1024
    unit += 1
  }
  return `${scaled.toFixed(1)} ${units[unit]}`
}

export function formatRate(bytesPerSecond: number | null | undefined): string {
  return `${formatBytes(Math.max(0, Math.round(bytes(bytesPerSecond))))}/s`
}

/**
 * Human age of a timestamp, in the current UI language (`time.*` keys).
 *
 * Timestamps cross IPC as unix milliseconds in an `f64`, so a missing value is
 * `null` rather than 0 — 0 would render as 1970.
 */
export function formatWhen(at: number | null | undefined, now = Date.now()): string {
  if (typeof at !== 'number' || !Number.isFinite(at) || at <= 0) return t('time.never')
  const minutes = Math.floor(Math.max(0, now - at) / 60_000)
  if (minutes < 1) return t('time.now')
  if (minutes < 60) return t('time.minutes', { count: minutes })
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return t('time.hours', { count: hours })
  const days = Math.floor(hours / 24)
  if (days === 1) return t('time.yesterday')
  if (days < 30) return t('time.days', { count: days })
  return t('time.months', { count: Math.floor(days / 30) })
}

/**
 * A remembered latency record, structurally.
 *
 * Declared here instead of imported from the contract so this module stays
 * free of the IPC layer (`format.test.ts` runs without the Tauri bridge).
 */
export interface ProbeRecord {
  delay_ms?: number | null
  at?: number | null
}

/**
 * Unix seconds to unix milliseconds.
 *
 * Probe records keep seconds — the number is one line of JSON and sub-second
 * precision would be noise — while the rest of the UI counts milliseconds,
 * so this is the one place the two units meet.
 */
export function probeTime(at: number | null | undefined): number | null {
  if (typeof at !== 'number' || !Number.isFinite(at) || at <= 0) return null
  return at * 1000
}

/** `120 ms`, or `测不到` when the probe ran and produced no number. */
export function formatDelay(delayMs: number | null | undefined): string {
  if (typeof delayMs !== 'number' || !Number.isFinite(delayMs) || delayMs <= 0) {
    return t('nodes.probe.miss')
  }
  return t('nodes.latency.ms', { value: Math.round(delayMs) })
}

/**
 * One remembered probe as the table shows it: `120 ms · 3 分钟前`, `测不到 · 昨天`.
 *
 * Returns an empty string when there is no record at all, so a caller can tell
 * "never measured" (nothing to show) from "measured, no answer" (`测不到`).
 */
export function formatProbe(probe: ProbeRecord | null | undefined, now = Date.now()): string {
  if (!probe) return ''
  return `${formatDelay(probe.delay_ms)} · ${formatWhen(probeTime(probe.at), now)}`
}

/**
 * Clock time for a timestamp the kernel handed us as text.
 *
 * sing-box reports `2026-11-22T03:55:30.4070053+08:00`; printing that raw in a
 * live table is unreadable. The date is added only when the moment is not
 * today, so a row from a long-lived connection is not mistaken for one just
 * opened.
 */
export function formatClock(value: string | null | undefined, now = Date.now()): string {
  if (!value) return '—'
  const at = new Date(value)
  if (Number.isNaN(at.getTime())) return value
  const time = at.toLocaleTimeString(undefined, { hour12: false })
  if (new Date(now).toDateString() === at.toDateString()) return time
  const month = String(at.getMonth() + 1).padStart(2, '0')
  const day = String(at.getDate()).padStart(2, '0')
  return `${month}-${day} ${time}`
}

/** `100 GB` instead of `100.0 GB` — a decimal that only costs column width. */
export function formatBytesCompact(value: number | null | undefined): string {
  return formatBytes(value).replace(/\.0 (?=[A-Z])/, ' ')
}

/**
 * `2026-03-01` — a plain calendar day, for things like a plan's expiry date
 * where "3 天后" would be a moving target nobody can plan around.
 */
export function formatDay(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—'
  const at = new Date(value)
  if (Number.isNaN(at.getTime())) return '—'
  const month = String(at.getMonth() + 1).padStart(2, '0')
  const day = String(at.getDate()).padStart(2, '0')
  return `${at.getFullYear()}-${month}-${day}`
}

/**
 * The fields a subscription needs for its refresh countdown, structurally.
 *
 * Declared here rather than imported from the contract for the same reason as
 * `ProbeRecord`: `format.test.ts` runs without the Tauri bridge.
 */
export interface RefreshSchedule {
  enabled: boolean
  update_interval_minutes?: number | null
  updated_at_ms?: number | null
}

/** The shape of `t()`, so a sentence can be built without a fixed dictionary. */
export type Translator = (key: string, params?: Record<string, string | number>) => string

/**
 * Milliseconds until the next automatic refresh, `null` when there is none to
 * count down to.
 *
 * Three honest "nothing to count" cases, all matching the backend's `is_due`:
 * the source is disabled, it is "only when I ask" (interval 0 or absent), or it
 * was never fetched successfully — a countdown started from a timestamp that
 * does not exist would be invented, and the row already says 从未.
 *
 * The result can be negative: that means the source is overdue and the sweep
 * simply has not reached it yet.
 */
export function nextRefreshInMs(sub: RefreshSchedule, nowMs: number): number | null {
  if (!sub.enabled) return null
  const interval = sub.update_interval_minutes
  if (typeof interval !== 'number' || !Number.isFinite(interval) || interval <= 0) return null
  const updatedAt = sub.updated_at_ms
  // 0 is what an older build left for "no timestamp" — same as `formatWhen`.
  if (typeof updatedAt !== 'number' || !Number.isFinite(updatedAt) || updatedAt <= 0) return null
  return updatedAt + interval * 60_000 - nowMs
}

/** `5 分钟` / `1 hour` — singular is spelled out instead of shipping "1 hours". */
function refreshDuration(count: number, unit: 'minute' | 'hour' | 'day', translate: Translator): string {
  return translate(`sub.next.${count === 1 ? unit : `${unit}s`}`, { count })
}

/**
 * When the next automatic refresh is due, as a sentence:
 * `还有 5 分钟`, `该刷新了`, `只手动刷新`, or `''` for a row that has nothing to add.
 *
 * Down-rounded on purpose: a countdown that says `还有 5 小时` must never promise
 * less time than is actually left, so the refresh can only be early, never late.
 *
 * `nowMs` is a parameter (not `Date.now()`) so the caller owns the ticking clock
 * and this stays a pure function of its inputs.
 */
export function formatNextRefresh(
  sub: RefreshSchedule,
  nowMs: number,
  translate: Translator = t,
): string {
  const remaining = nextRefreshInMs(sub, nowMs)
  if (remaining === null) {
    // Disabled and "only when I ask" read the same to the user: nothing happens
    // on its own, refreshing is their call. Never-fetched with a real interval
    // stays quiet — the 从未 next to it is the whole truth.
    if (!sub.enabled) return translate('sub.next.manual')
    const interval = sub.update_interval_minutes
    if (typeof interval !== 'number' || !Number.isFinite(interval) || interval <= 0) {
      return translate('sub.next.manual')
    }
    return ''
  }
  if (remaining <= 0) return translate('sub.next.due')
  // A few seconds short of the interval still reads as the smallest unit that
  // exists rather than as "还有 0 分钟".
  const minutes = Math.max(1, Math.floor(remaining / 60_000))
  let when: string
  if (minutes < 60) {
    when = refreshDuration(minutes, 'minute', translate)
  } else if (minutes < 48 * 60) {
    when = refreshDuration(Math.floor(minutes / 60), 'hour', translate)
  } else {
    when = refreshDuration(Math.floor(minutes / 1440), 'day', translate)
  }
  return translate('sub.next.in', { when })
}
