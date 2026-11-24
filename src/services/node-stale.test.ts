import { describe, expect, it } from 'vitest'

import type { Node_Serialize } from '../services/contract'
import { rendersDifferently } from './node-stale'

/**
 * A complete, valid node: the helper compares the whole transport/security
 * union, so a fixture missing an option would test the wrong thing.
 */
function node(overrides: Partial<Node_Serialize> = {}): Node_Serialize {
  return {
    id: 'node-1',
    name: 'Tokyo',
    server: 'tokyo.example.org',
    port: 443,
    protocol: { kind: 'Vless', uuid: '6f1f0b1e-7f0a-4a3a-9a2f-1c2d3e4f5a6b', flow: null },
    transport: { kind: 'Ws', path: '/ws', host: null },
    security: {
      kind: 'Tls',
      sni: 'tokyo.example.org',
      alpn: ['h2'],
      allow_insecure: false,
      pinned_cert_sha256: [],
    },
    tags: ['sub:sub-1'],
    ...overrides,
  }
}

describe('rendersDifferently', () => {
  it('says nothing changed for an untouched save', () => {
    // Re-opening a node and pressing 保存 without editing it must not offer a
    // restart: the kernel is already serving exactly these values.
    expect(rendersDifferently(node(), node())).toBe(false)
  })

  it('notices a changed port', () => {
    expect(rendersDifferently(node(), node({ port: 8443 }))).toBe(true)
  })

  it('notices a changed server', () => {
    expect(rendersDifferently(node(), node({ server: 'osaka.example.org' }))).toBe(true)
  })

  it('notices a changed security option', () => {
    const before = node()
    const after = node({
      security: {
        kind: 'Tls',
        sni: 'osaka.example.org',
        alpn: ['h2'],
        allow_insecure: false,
        pinned_cert_sha256: [],
      },
    })
    expect(rendersDifferently(before, after)).toBe(true)
  })

  it('notices a changed transport path', () => {
    expect(rendersDifferently(node(), node({ transport: { kind: 'Ws', path: '/v2', host: null } }))).toBe(true)
  })

  it('notices a changed protocol', () => {
    expect(rendersDifferently(node(), node({ protocol: { kind: 'Trojan', password: 'secret' } }))).toBe(true)
  })

  it('ignores a rename', () => {
    // `name` is printed on the card and nowhere else, so renaming the running
    // node leaves the kernel correct and must not restart it.
    expect(rendersDifferently(node(), node({ name: 'Tokyo (renamed)' }))).toBe(false)
  })

  it('ignores a tag change', () => {
    // Tags only group the card; they never reach the rendered config.
    expect(rendersDifferently(node({ tags: [] }), node({ tags: ['daily', 'sub:sub-2'] }))).toBe(false)
  })
})
