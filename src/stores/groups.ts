import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api, describeError, type ProxyGroup_Deserialize, type ProxyGroup_Serialize } from '../services/contract'

/**
 * Proxy group mirror. Like every other list in this app, a mutation answers
 * with the whole collection and the local copy is replaced wholesale — the
 * backend stays the only source of truth.
 */
export const useGroupsStore = defineStore('groups', () => {
  const items = ref<ProxyGroup_Serialize[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    loading.value = true
    error.value = null
    try {
      items.value = await api.listGroups()
      loaded.value = true
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  async function save(group: ProxyGroup_Deserialize) {
    items.value = await api.upsertGroup(group)
    loaded.value = true
  }

  async function remove(id: string) {
    items.value = await api.deleteGroup(id)
  }

  async function select(id: string, nodeId: string) {
    items.value = await api.selectGroupMember(id, nodeId)
  }

  async function toggle(id: string) {
    items.value = await api.toggleGroup(id)
  }

  /** The group a rule points at, if it still exists. */
  function byId(id: string): ProxyGroup_Serialize | undefined {
    return items.value.find((group) => group.id === id)
  }

  return { items, loading, error, loaded, load, save, remove, select, toggle, byId }
})
