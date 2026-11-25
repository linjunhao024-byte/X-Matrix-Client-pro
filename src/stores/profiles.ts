import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api, describeError, type ProfileView, type SwitchReport_Serialize } from '../services/contract'
import { useGroupsStore } from './groups'
import { useNodesStore } from './nodes'
import { useRoutingStore } from './routing'
import { useRuleSetsStore } from './rulesets'
import { useSettingsStore } from './settings'
import { useSubscriptionsStore } from './subscriptions'

/**
 * Profile mirror. Each profile keeps its own nodes, subscriptions, rules and
 * groups; this store only knows the names and which one is active.
 *
 * Switching a profile swaps what every other list store is looking at, so
 * `activate` refreshes them with `force = true` — their `loaded` flag would
 * otherwise keep the previous profile's rows on screen.
 */
export const useProfilesStore = defineStore('profiles', () => {
  const items = ref<ProfileView[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    loading.value = true
    error.value = null
    try {
      items.value = await api.profilesList()
      loaded.value = true
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  /** Create a profile; `from` copies another profile's nodes and rules. */
  async function create(name: string, from: string | null) {
    const created = await api.profileCreate(name, from)
    await load(true)
    return created
  }

  async function rename(id: string, name: string) {
    const updated = await api.profileRename(id, name)
    await load(true)
    return updated
  }

  async function remove(id: string) {
    await api.profileDelete(id)
    await load(true)
  }

  /**
   * Make a profile active and report what happened to the kernel. Throws on
   * rejection so the view can show the backend's own wording.
   */
  async function activate(id: string): Promise<SwitchReport_Serialize> {
    const report = await api.profileSwitch(id)
    await Promise.all([
      load(true),
      useSettingsStore().load(true),
      useNodesStore().load(true),
      useRoutingStore().load(true),
      useGroupsStore().load(true),
      useRuleSetsStore().load(true),
      useSubscriptionsStore().load(true),
    ])
    return report
  }

  /** The profile a row stands for, if the list still has it. */
  function byId(id: string): ProfileView | undefined {
    return items.value.find((profile) => profile.id === id)
  }

  return { items, loading, error, loaded, load, create, rename, remove, activate, byId }
})