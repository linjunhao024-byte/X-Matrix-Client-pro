import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import {
  api,
  describeError,
  type RefreshOutcome_Serialize,
  type Subscription_Deserialize,
  type Subscription_Serialize,
} from '../services/contract'
import { useNodesStore } from './nodes'

/**
 * Subscription sources.
 *
 * The backend owns the list and always answers with the whole table, so every
 * mutation here simply replaces the local copy — the same rule the node and
 * routing stores follow.
 */
export const useSubscriptionsStore = defineStore('subscriptions', () => {
  const items = ref<Subscription_Serialize[]>([])
  const loading = ref(false)
  const /** id of the source currently being refreshed */ busy = ref<string | null>(null)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  const count = computed(() => items.value.length)

  async function load(force = false): Promise<void> {
    if (loaded.value && !force) return
    loading.value = true
    error.value = null
    try {
      items.value = await api.listSubscriptions()
      loaded.value = true
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  /**
   * Creating a source takes name and url only, so giving it a rename/filter
   * policy is a second call. The caller therefore needs the row that was just
   * added: the backend answers with the whole table, so pick out the one id
   * that was not there before.
   */
  async function create(name: string, url: string): Promise<Subscription_Serialize> {
    const known = new Set(items.value.map((item) => item.id))
    const table = await api.createSubscription(name, url)
    items.value = table
    loaded.value = true
    const created = table.find((item) => !known.has(item.id)) ?? table[table.length - 1]
    if (!created) throw new Error('订阅已创建，但列表里找不到它')
    return created
  }

  async function save(subscription: Subscription_Deserialize): Promise<void> {
    items.value = await api.upsertSubscription(subscription)
    loaded.value = true
  }

  async function remove(id: string): Promise<void> {
    items.value = await api.deleteSubscription(id)
    // The nodes it brought in went with it.
    await useNodesStore().load(true)
  }

  async function refresh(id: string): Promise<RefreshOutcome_Serialize> {
    busy.value = id
    error.value = null
    try {
      const outcome = await api.refreshSubscription(id)
      items.value = await api.listSubscriptions()
      await useNodesStore().load(true)
      return outcome
    } catch (cause) {
      error.value = describeError(cause)
      items.value = await api.listSubscriptions()
      throw cause
    } finally {
      busy.value = null
    }
  }

  /**
   * Refreshing every due source answers with three counts: nodes that came in,
   * nodes dropped as duplicates, and nodes dropped by the source's own
   * include/exclude patterns. Reporting only the first would make a provider
   * that lists duplicates look like it shrank, and a filter look like a bug.
   */
  async function refreshAll(): Promise<{
    imported: number
    skipped: number
    filtered: number
    note: string | null
  }> {
    loading.value = true
    error.value = null
    try {
      const outcomes = await api.refreshDue()
      items.value = await api.listSubscriptions()
      await useNodesStore().load(true)
      const total = outcomes.reduce(
        (sum, outcome) => ({
          imported: sum.imported + (outcome.imported ?? 0),
          skipped: sum.skipped + (outcome.skipped?.length ?? 0),
          filtered: sum.filtered + (outcome.filtered ?? 0),
          note: sum.note ?? outcome.note ?? null,
        }),
        { imported: 0, skipped: 0, filtered: 0, note: null as string | null },
      )
      return total
    } catch (cause) {
      error.value = describeError(cause)
      throw cause
    } finally {
      loading.value = false
    }
  }

  return { items, loading, busy, error, loaded, count, load, create, save, remove, refresh, refreshAll }
})
