import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  api,
  describeError,
  type ImportOutcome_Serialize,
  type Node_Deserialize,
  type Node_Serialize,
} from '../services/contract'

/**
 * Node list mirror of the Rust-owned collection. The backend is the single
 * source of truth: every mutation returns the full list and replaces the local
 * copy, so the UI can never drift from what is persisted.
 */
export const useNodesStore = defineStore('nodes', () => {
  const items = ref<Node_Serialize[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    loading.value = true
    error.value = null
    try {
      items.value = await api.listNodes()
      loaded.value = true
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  async function save(node: Node_Deserialize) {
    items.value = await api.upsertNode(node)
    loaded.value = true
  }

  async function remove(id: string) {
    items.value = await api.deleteNode(id)
  }

  /** Delete a selection in one round trip, so it cannot be half applied. */
  async function removeMany(ids: string[]) {
    items.value = await api.deleteNodes(ids)
  }

  /** Add or remove one tag across a selection, in one round trip. */
  async function tagMany(ids: string[], tag: string, remove: boolean) {
    items.value = await api.tagNodes(ids, tag, remove)
  }

  /** Share links, one per line, for the given nodes. */
  async function exportLinks(ids: string[]): Promise<string> {
    return api.exportNodeLinks(ids)
  }

  /** Parse share links / a subscription document and merge them in. */
  async function importFromText(text: string): Promise<ImportOutcome_Serialize> {
    const outcome = await api.importNodes(text)
    items.value = outcome.nodes
    loaded.value = true
    return outcome
  }

  return { items, loading, error, loaded, load, save, remove, removeMany, tagMany, exportLinks, importFromText }
})
