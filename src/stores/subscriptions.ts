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

  async function create(name: string, url: string): Promise<void> {
    items.value = await api.createSubscription(name, url)
    loaded.value = true
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
   * Refreshing every due source answers with both numbers: how many nodes came
   * in and how many were dropped. Reporting only the first would make a
   * provider that lists duplicates look like it shrank.
   */
  async function refreshAll(): Promise<{ imported: number; skipped: number }> {
    loading.value = true
    error.value = null
    try {
      const outcomes = await api.refreshDue()
      items.value = await api.listSubscriptions()
      await useNodesStore().load(true)
      return outcomes.reduce(
        (total, outcome) => ({
          imported: total.imported + (outcome.imported ?? 0),
          skipped: total.skipped + (outcome.skipped?.length ?? 0),
        }),
        { imported: 0, skipped: 0 },
      )
    } catch (cause) {
      error.value = describeError(cause)
      throw cause
    } finally {
      loading.value = false
    }
  }

  return { items, loading, busy, error, loaded, count, load, create, save, remove, refresh, refreshAll }
})
