import { describe, expect, it } from 'vitest'
import type { ConnectionInfo } from './bindings'
import { connectionsToCsv, csvField } from './csv'

function row(overrides: Partial<ConnectionInfo>): ConnectionInfo {
  return {
    id: 'c1',
    host: 'example.org',
    network: 'tcp',
    rule: 'final',
    chains: ['proxy'],
    upload: 10,
    download: 20,
    start: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('csvField', () => {
  it('leaves a plain value alone', () => {
    expect(csvField('example.org')).toBe('example.org')
  })

  it('quotes commas, quotes and newlines', () => {
    expect(csvField('a,b')).toBe('"a,b"')
    expect(csvField('say "hi"')).toBe('"say ""hi"""')
    expect(csvField('two\nlines')).toBe('"two\nlines"')
  })
})

describe('connectionsToCsv', () => {
  it('writes a header and one line per connection', () => {
    const csv = connectionsToCsv([row({}), row({ id: 'c2', host: 'bing.com' })])
    const lines = csv.trimEnd().split('\n')

    expect(lines[0]).toBe('host,network,rule,chains,upload,download,start')
    expect(lines).toHaveLength(3)
    expect(lines[1]).toBe('example.org,tcp,final,proxy,10,20,2026-01-01T00:00:00Z')
  })

  it('joins the chain with an arrow rather than a comma', () => {
    const csv = connectionsToCsv([row({ chains: ['proxy', 'group:g1'] })])
    expect(csv).toContain('proxy > group:g1')
  })

  it('leaves missing byte counts empty instead of writing null', () => {
    const csv = connectionsToCsv([row({ upload: null, download: null })])
    expect(csv.trimEnd().split('\n')[1]).toBe('example.org,tcp,final,proxy,,,2026-01-01T00:00:00Z')
  })
})
