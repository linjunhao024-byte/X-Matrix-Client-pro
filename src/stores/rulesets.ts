import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  api,
  describeError,
  type RuleSet_Deserialize,
  type RuleSet_Serialize,
} from '../services/contract'

/**
 * Rule set mirror. Mutations answer with the whole collection, except the two
 * that only touch one set's contents (`setText`, `refresh`) — those answer with
 * the updated set, which is patched in place so the list keeps its order.
 */
export const useRuleSetsStore = defineStore('rulesets', () => {
  const items = ref<RuleSet_Serialize[]>([])
  const loading = ref(false)
  const busyId = ref<string | null>(null)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    loading.value = true
    error.value = null
    try {
      items.value = await api.listRuleSets()
      loaded.value = true
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  function patch(updated: RuleSet_Serialize) {
    const index = items.value.findIndex((item) => item.id === updated.id)
    if (index === -1) items.value = [...items.value, updated]
    else items.value = items.value.map((item) => (item.id === updated.id ? updated : item))
  }

  async function create(name: string, url: string) {
    items.value = await api.createRuleSet(name, url)
    loaded.value = true
  }

  async function save(ruleSet: RuleSet_Deserialize) {
    items.value = await api.upsertRuleSet(ruleSet)
    loaded.value = true
  }

  async function remove(id: string) {
    items.value = await api.deleteRuleSet(id)
  }

  async function toggle(id: string) {
    items.value = await api.toggleRuleSet(id)
  }

  /** Replaces the whole list from pasted text; throws so the dialog can show why. */
  async function setText(id: string, text: string) {
    busyId.value = id
    try {
      const updated = await api.setRuleSetText(id, text)
      patch(updated)
      return updated
    } finally {
      busyId.value = null
    }
  }

  /** Re-fetches a remote set (or re-parses a local one). */
  async function refresh(id: string) {
    busyId.value = id
    try {
      const updated = await api.refreshRuleSet(id)
      patch(updated)
      return updated
    } finally {
      busyId.value = null
    }
  }

  /** Enabled sets only — a disabled set must not be offered as a rule target. */
  function usable() {
    return items.value.filter((item) => item.enabled)
  }

  function byId(id: string): RuleSet_Serialize | undefined {
    return items.value.find((item) => item.id === id)
  }

  return {
    items,
    loading,
    busyId,
    error,
    loaded,
    load,
    create,
    save,
    remove,
    toggle,
    setText,
    refresh,
    usable,
    byId,
  }
})
