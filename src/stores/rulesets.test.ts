import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

/**
 * These tests pin the seam that nothing else covers: the store must call the
 * *exact* command names the contract exposes, and it must treat the backend's
 * answer as the new truth (the backend always replies with the whole list, so
 * a store that merged locally would drift the moment two writes race).
 *
 * The dialog → store half is typechecked; this file is the store → IPC half.
 */

const { api } = vi.hoisted(() => {
  const make = (id: string, name = id) => ({
    id,
    name,
    enabled: true,
    source: 'local' as const,
    url: '',
    update_interval_minutes: 0,
    updated_at_ms: null,
    entry_count: 0,
    last_error: null,
  })

  const api = {
    listRuleSets: vi.fn(async () => [make('a'), make('b')]),
    createRuleSet: vi.fn(async (name: string) => [make('a'), make(name)]),
    upsertRuleSet: vi.fn(async (ruleSet: { id: string }) => [make(ruleSet.id)]),
    deleteRuleSet: vi.fn(async () => [make('b')]),
    toggleRuleSet: vi.fn(async () => [make('a'), make('b')]),
    setRuleSetText: vi.fn(async (id: string) => make(id)),
    refreshRuleSet: vi.fn(async (id: string) => make(id)),
  }

  return { api, make }
})

vi.mock('../services/contract', () => ({
  api,
  describeError: (cause: unknown) => (cause instanceof Error ? cause.message : String(cause)),
}))

const { useRuleSetsStore } = await import('./rulesets')

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
})

describe('rulesets store', () => {
  it('loads once, then only when forced', async () => {
    const store = useRuleSetsStore()
    await store.load()
    expect(api.listRuleSets).toHaveBeenCalledTimes(1)
    expect(store.items.map((item) => item.id)).toEqual(['a', 'b'])
    expect(store.loaded).toBe(true)

    await store.load()
    expect(api.listRuleSets).toHaveBeenCalledTimes(1)

    await store.load(true)
    expect(api.listRuleSets).toHaveBeenCalledTimes(2)
  })

  it('reports a load failure instead of throwing', async () => {
    api.listRuleSets.mockRejectedValueOnce(new Error('boom'))
    const store = useRuleSetsStore()
    await store.load()
    expect(store.error).toBe('boom')
    expect(store.loaded).toBe(false)
  })

  it('saves through upsertRuleSet and takes the answer as the new list', async () => {
    const store = useRuleSetsStore()
    const draft = {
      id: 'a',
      name: 'rename',
      enabled: true,
      source: 'local' as const,
      url: '',
      update_interval_minutes: 0,
      updated_at_ms: null,
      entry_count: 0,
      last_error: null,
    }
    await store.save(draft)
    expect(api.upsertRuleSet).toHaveBeenCalledWith(draft)
    expect(store.items.map((item) => item.id)).toEqual(['a'])
  })

  it('passes name and url to createRuleSet', async () => {
    const store = useRuleSetsStore()
    await store.create('cn', 'https://example.com/cn.yaml')
    expect(api.createRuleSet).toHaveBeenCalledWith('cn', 'https://example.com/cn.yaml')
    expect(store.items).toHaveLength(2)
  })

  it('patches a single set in place so the order survives', async () => {
    const store = useRuleSetsStore()
    await store.load()
    await store.setText('b', 'example.com\n')
    expect(api.setRuleSetText).toHaveBeenCalledWith('b', 'example.com\n')
    expect(store.items.map((item) => item.id)).toEqual(['a', 'b'])
    expect(store.busyId).toBeNull()
  })

  it('clears busyId even when the text write fails', async () => {
    api.setRuleSetText.mockRejectedValueOnce(new Error('bad list'))
    const store = useRuleSetsStore()
    await expect(store.setText('a', '')).rejects.toThrow('bad list')
    expect(store.busyId).toBeNull()
  })

  it('deletes and toggles by id', async () => {
    const store = useRuleSetsStore()
    await store.remove('a')
    expect(api.deleteRuleSet).toHaveBeenCalledWith('a')
    expect(store.items.map((item) => item.id)).toEqual(['b'])

    await store.toggle('a')
    expect(api.toggleRuleSet).toHaveBeenCalledWith('a')
    expect(store.items.map((item) => item.id)).toEqual(['a', 'b'])
  })
})
