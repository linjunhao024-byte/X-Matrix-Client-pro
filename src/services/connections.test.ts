import { describe, expect, it } from 'vitest'
import type { ConnectionInfo } from './bindings'
import { groupConnections } from './connections'

function row(overrides: Partial<ConnectionInfo>): ConnectionInfo {
  return {
    id: 'c1',
    host: 'example.org',
    network: 'tcp',
    rule: '',
    chains: [],
    upload: 0,
    download: 0,
    start: '2026-01-01T00:00:00Z',
    process: '',
    ...overrides,
  }
}

describe('groupConnections', () => {
  it('sums connections by host, busiest first', () => {
    const groups = groupConnections(
      [
        row({ id: 'a', host: 'example.org', download: 100, upload: 10 }),
        row({ id: 'b', host: 'bing.com', download: 5, upload: 1 }),
        row({ id: 'c', host: 'example.org', download: 50, upload: 2 }),
      ],
      'host',
    )

    expect(groups.map((g) => g.name)).toEqual(['example.org', 'bing.com'])
    expect(groups[0]).toMatchObject({ count: 2, download: 150, upload: 12, unknown: false })
    expect(groups[1]).toMatchObject({ count: 1, download: 5, upload: 1 })
  })

  it('groups by the first chain entry only', () => {
    const groups = groupConnections(
      [
        row({ id: 'a', chains: ['proxy', 'group:g1'] }),
        row({ id: 'b', chains: ['proxy', 'group:g2'] }),
        row({ id: 'c', chains: ['direct'] }),
      ],
      'chain',
    )

    expect(groups.map((g) => g.name)).toEqual(['proxy', 'direct'])
    expect(groups[0].count).toBe(2)
  })

  it('treats a missing host as its own unknown bucket', () => {
    const groups = groupConnections([row({ host: '' }), row({ host: 'example.org' })], 'host')
    const unknown = groups.find((g) => g.unknown)

    expect(unknown).toBeDefined()
    expect(unknown?.name).toBe('')
    expect(unknown?.count).toBe(1)
  })

  it('counts missing byte totals as zero instead of NaN', () => {
    const groups = groupConnections(
      [row({ host: 'example.org', upload: null, download: null }), row({ host: 'example.org', download: 10 })],
      'host',
    )

    expect(groups[0]).toMatchObject({ count: 2, upload: 0, download: 10 })
  })
})
