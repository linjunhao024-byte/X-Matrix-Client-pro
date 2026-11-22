/**
 * Plan quota and expiry, turned into a verdict the UI can act on.
 *
 * The numbers already arrive from the subscription provider's
 * `Subscription-Userinfo` header (`traffic_used`, `traffic_total`,
 * `expires_at_ms`). What the user needs is not the arithmetic again but an
 * answer to "is this about to stop working?", so the judgement lives here as a
 * pure function with thresholds in one place instead of being re-derived with
 * `if` soup inside a template.
 */
import type { Subscription_Serialize } from '../services/contract'

/** Warn once nine tenths of the plan is gone — the last tenth goes fast. */
export const LOW_RATIO = 0.9

/** A week is enough warning to renew before traffic stops mid-page. */
export const EXPIRING_DAYS = 7

const DAY_MS = 86_400_000

export type QuotaLevel = 'ok' | 'low' | 'full' | 'expiring' | 'expired'

export interface QuotaReport {
  level: QuotaLevel
  /** used / total, or `null` when the plan has no cap. */
  ratio: number | null
  /** Whole days left before expiry, or `null` when it never expires. */
  daysLeft: number | null
}

/**
 * Rank by how much trouble the user is in. Expired beats full (nothing works
 * at all), full beats low, and "expiring" is the mildest — the plan still
 * works today.
 */
const SEVERITY: Record<QuotaLevel, number> = {
  ok: 0,
  expiring: 1,
  low: 2,
  full: 3,
  expired: 4,
}

export function severity(level: QuotaLevel): number {
  return SEVERITY[level]
}

export function quotaReport(subscription: Subscription_Serialize, now = Date.now()): QuotaReport {
  const used = subscription.traffic_used
  const total = subscription.traffic_total
  const expires = subscription.expires_at_ms

  const ratio = typeof total === 'number' && total > 0 && typeof used === 'number' ? used / total : null
  const daysLeft =
    typeof expires === 'number' && expires > 0 ? Math.ceil((expires - now) / DAY_MS) : null

  // An already-expired plan is the loudest thing we can say, even if the byte
  // counters still look healthy — the provider has stopped serving it.
  if (daysLeft !== null && daysLeft <= 0) return { level: 'expired', ratio, daysLeft }
  if (ratio !== null && ratio >= 1) return { level: 'full', ratio, daysLeft }
  if (ratio !== null && ratio >= LOW_RATIO) return { level: 'low', ratio, daysLeft }
  if (daysLeft !== null && daysLeft <= EXPIRING_DAYS) return { level: 'expiring', ratio, daysLeft }

  return { level: 'ok', ratio, daysLeft }
}

export interface QuotaAlert {
  id: string
  name: string
  level: Exclude<QuotaLevel, 'ok'>
  report: QuotaReport
}

/**
 * Every subscription that deserves a word, worst first.
 *
 * Disabled sources are skipped: the user already told us not to use them, so
 * nagging about their expiry would be noise.
 */
export function quotaAlerts(
  subscriptions: Subscription_Serialize[],
  now = Date.now(),
): QuotaAlert[] {
  const alerts: QuotaAlert[] = []
  for (const subscription of subscriptions) {
    if (!subscription.enabled) continue
    const report = quotaReport(subscription, now)
    if (report.level === 'ok') continue
    alerts.push({ id: subscription.id, name: subscription.name, level: report.level, report })
  }
  return alerts.sort((left, right) => severity(right.level) - severity(left.level))
}
