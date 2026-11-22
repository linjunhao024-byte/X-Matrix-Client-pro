import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

/**
 * The session counter is the one number in the store that is *derived* rather
 * than mirrored, so it needs pinning: the kernel reports a total that only
 * grows (Xray) or a sliding window (sing-box), and we turn the difference
 * between two samples into "how much this session used". A reset that cleared
 * the baseline instead of the counter would report the kernel's whole total as
 * one spike on the next tick — that is the bug this file exists to catch.
 */

const { api, feed } = vi.hoisted(() => {
  let nextSample: { up_total: number; down_total: number } | null = null

  const feed = (up: number, down: number) => {
    nextSample = { up_total: up, down_total: down }
  }

  const api = {
    engineVersion: vi.fn(async (kind: 'xray' | 'sing-box') => `version of ${kind}`),
    engineStatus: vi.fn(async () => ({
      running: true,
      kind: 'xray' as const,
      node_id: 'n1',
      mixed_port: 7890,
      api_port: 9090,
      started_at_ms: 0,
      last_exit_code: null,
      last_error: null,
    })),
    trafficSample: vi.fn(async () => {
      if (!nextSample) return null
      return {
        up_total: nextSample.up_total,
        down_total: nextSample.down_total,
        up_rate: 0,
        down_rate: 0,
        connections: [],
        supports_connections: false,
      }
    }),
  }

  return { api, feed }
})

vi.mock('../services/contract', () => ({ api, describeError: (e: unknown) => String(e) }))

import { useEngineStore } from './engine'

describe('engine store session counter', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    feed(0, 0)
  })

  it('accumulates the difference between samples, not the kernel total', async () => {
    const store = useEngineStore()

    feed(1_000, 4_000)
    await store.refreshStatus()
    await store.refreshTraffic()
    // The first sample has no predecessor, so it only sets the baseline.
    expect(store.session).toEqual({ up: 0, down: 0 })

    feed(1_500, 6_000)
    await store.refreshTraffic()
    expect(store.session).toEqual({ up: 500, down: 2_000 })

    feed(2_000, 7_000)
    await store.refreshTraffic()
    expect(store.session).toEqual({ up: 1_000, down: 3_000 })
  })

  it('reset zeroes the counter but keeps the baseline', async () => {
    const store = useEngineStore()

    feed(0, 0)
    await store.refreshStatus()
    await store.refreshTraffic()
    feed(5_000, 9_000)
    await store.refreshTraffic()
    expect(store.session.up).toBe(5_000)

    store.resetSession()
    expect(store.session).toEqual({ up: 0, down: 0 })

    // One more sample with no new traffic must not resurrect the old total.
    feed(5_000, 9_000)
    await store.refreshTraffic()
    expect(store.session).toEqual({ up: 0, down: 0 })

    feed(5_100, 9_000)
    await store.refreshTraffic()
    expect(store.session).toEqual({ up: 100, down: 0 })
  })

  it('ignores a shrinking total instead of reporting negative traffic', async () => {
    const store = useEngineStore()

    feed(10_000, 10_000)
    await store.refreshStatus()
    await store.refreshTraffic()

    // sing-box restarts its counters with the process; a reset tunnel must not
    // show up as a huge negative number.
    feed(0, 0)
    await store.refreshTraffic()
    expect(store.session).toEqual({ up: 0, down: 0 })
    expect(store.rates.up).toBe(0)
  })
})

describe('engine store kernel versions', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('keeps both kernels when their probes overlap', async () => {
    // The real failure: both probes take about a second, the two writes each
    // started from the same empty snapshot, and the later resolve overwrote the
    // earlier one — so the Xray version line disappeared on a machine with both
    // kernels installed.
    const store = useEngineStore()
    await Promise.all([store.refreshVersion('xray'), store.refreshVersion('sing-box')])

    expect(store.versions.xray).toBe('version of xray')
    expect(store.versions['sing-box']).toBe('version of sing-box')
  })

  it('a failed probe leaves the other version alone', async () => {
    const store = useEngineStore()
    await store.refreshVersion('xray')
    api.engineVersion.mockRejectedValueOnce(new Error('no binary'))
    await store.refreshVersion('sing-box')

    expect(store.versions.xray).toBe('version of xray')
    expect(store.versions['sing-box']).toBeUndefined()
  })
})
