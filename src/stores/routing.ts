import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api, describeError, type RoutingRule } from '../services/contract'

/**
 * Routing table mirror. Order is meaningful (first match wins), so every
 * mutation returns the whole list and replaces the local copy — the same rule
 * the node list follows.
 */
export const useRoutingStore = defineStore('routing', () => {
  const items = ref<RoutingRule[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    loading.value = true
    error.value = null
    try {
      items.value = await api.listRules()
      loaded.value = true
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  async function save(rule: RoutingRule) {
    items.value = await api.upsertRule(rule)
    loaded.value = true
  }

  async function remove(id: string) {
    items.value = await api.deleteRule(id)
  }

  async function move(id: string, delta: number) {
    items.value = await api.moveRule(id, delta)
  }

  async function toggle(id: string) {
    items.value = await api.toggleRule(id)
  }

  return { items, loading, error, loaded, load, save, remove, move, toggle }
})
