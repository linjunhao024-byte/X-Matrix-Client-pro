import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

/**
 * The node store is the only place that knows which IPC command backs each
 * action, so this file pins the bulk paths: a batch delete that silently called
 * `deleteNode` once per id would still look right in the UI but would write the
 * document N times, and an export that returned the wrong shape would paste
 * garbage into the user's clipboard.
 */

const { api, reset } = vi.hoisted(() => {
  const make = (id: string, name = id) => ({
    id,
    name,
    server: 'example.com',
    port: 443,
    protocol: { kind: 'Vless' as const, uuid: 'u', flow: null },
    transport: { kind: 'Tcp' as const },
    security: { kind: 'None' as const },
    source: null,
  })

  // A tiny fake document: the backend always answers with the whole list, so
  // the mock has to behave the same way or the store tests prove nothing.
  let remaining = ['a', 'b', 'c']

  const api = {
    listNodes: vi.fn(async () => remaining.map((id) => make(id))),
    saveNode: vi.fn(async (node: { id: string }) => [make(node.id)]),
    deleteNode: vi.fn(async () => {
      remaining = remaining.slice(1)
      return remaining.map((id) => make(id))
    }),
    deleteNodes: vi.fn(async (ids: string[]) => {
      remaining = remaining.filter((id) => !ids.includes(id))
      return remaining.map((id) => make(id))
    }),
    exportNodeLinks: vi.fn(async () => 'vless://one\nvless://two'),
    importNodes: vi.fn(async () => ({ imported: 0, duplicates: 0, skipped: [], nodes: [] })),
  }

  const reset = () => {
    remaining = ['a', 'b', 'c']
  }

  return { api, reset }
})

vi.mock('../services/contract', () => ({
  api,
  describeError: (error: unknown) => String(error),
}))

import { useNodesStore } from './nodes'

describe('nodes store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    reset()
  })

  it('deletes a whole selection in one command call', async () => {
    const store = useNodesStore()
    await store.load()

    await store.removeMany(['a', 'b'])

    expect(api.deleteNodes).toHaveBeenCalledTimes(1)
    expect(api.deleteNodes).toHaveBeenCalledWith(['a', 'b'])
    // The per-node path must not be used for a batch, or the document is
    // rewritten once per id.
    expect(api.deleteNode).not.toHaveBeenCalled()
    expect(store.items.map((node) => node.id)).toEqual(['c'])
  })

  it('returns the export payload verbatim instead of re-joining it', async () => {
    const store = useNodesStore()

    const text = await store.exportLinks(['a', 'b'])

    expect(api.exportNodeLinks).toHaveBeenCalledWith(['a', 'b'])
    expect(text).toBe('vless://one\nvless://two')
  })

  it('replaces the list with whatever the backend answers', async () => {
    const store = useNodesStore()
    await store.load()
    expect(store.items).toHaveLength(3)

    await store.removeMany(['a', 'b', 'c'])

    // An empty reply is the truth too: the store must not keep a local copy.
    expect(store.items).toEqual([])
  })
})
