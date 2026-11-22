import { describe, expect, it } from 'vitest'

import type { Subscription_Serialize } from '../services/contract'
import { EXPIRING_DAYS, LOW_RATIO, quotaAlerts, quotaReport } from './quota'

const NOW = 1_700_000_000_000

function subscription(overrides: Partial<Subscription_Serialize> = {}): Subscription_Serialize {
  return {
    id: 'sub-1',
    name: '机场',
    url: 'https://example.org/sub',
    enabled: true,
    update_interval_minutes: 360,
    updated_at_ms: NOW,
    node_count: 2,
    last_error: null,
    traffic_used: null,
    traffic_total: null,
    expires_at_ms: null,
    ...overrides,
  } as Subscription_Serialize
}

describe('quotaReport', () => {
  it('says nothing when the provider gave us no numbers', () => {
    const report = quotaReport(subscription(), NOW)
    expect(report.level).toBe('ok')
    expect(report.ratio).toBeNull()
    expect(report.daysLeft).toBeNull()
  })

  it('warns once nine tenths of the plan is used', () => {
    const report = quotaReport(subscription({ traffic_used: 900, traffic_total: 1000 }), NOW)
    expect(report.level).toBe('low')
    expect(report.ratio).toBeCloseTo(LOW_RATIO)
  })

  it('treats an exhausted plan as full rather than low', () => {
    expect(quotaReport(subscription({ traffic_used: 1000, traffic_total: 1000 }), NOW).level).toBe(
      'full',
    )
  })

  it('counts days left and warns inside the last week', () => {
    const report = quotaReport(
      subscription({ expires_at_ms: NOW + EXPIRING_DAYS * 86_400_000 }),
      NOW,
    )
    expect(report.level).toBe('expiring')
    expect(report.daysLeft).toBe(EXPIRING_DAYS)
  })

  it('calls an already expired plan expired even if traffic looks fine', () => {
    const report = quotaReport(
      subscription({ expires_at_ms: NOW - 86_400_000, traffic_used: 1, traffic_total: 1000 }),
      NOW,
    )
    expect(report.level).toBe('expired')
  })

  it('ignores a zero total — that is the provider saying "no limit"', () => {
    const report = quotaReport(subscription({ traffic_used: 999_999, traffic_total: 0 }), NOW)
    expect(report.ratio).toBeNull()
    expect(report.level).toBe('ok')
  })
})

describe('quotaAlerts', () => {
  it('keeps only the sources with something to say, worst first', () => {
    const alerts = quotaAlerts(
      [
        subscription({ id: 'ok', name: '健康', traffic_used: 1, traffic_total: 1000 }),
        subscription({ id: 'expiring', name: '快到期', expires_at_ms: NOW + 2 * 86_400_000 }),
        subscription({ id: 'full', name: '用光了', traffic_used: 100, traffic_total: 100 }),
      ],
      NOW,
    )
    expect(alerts.map((alert) => alert.id)).toEqual(['full', 'expiring'])
  })

  it('does not nag about a source the user disabled', () => {
    const alerts = quotaAlerts(
      [subscription({ enabled: false, traffic_used: 100, traffic_total: 100 })],
      NOW,
    )
    expect(alerts).toEqual([])
  })
})
