/**
 * The Windows system proxy, as seen by the backend.
 *
 * The UI can read it and give it back, but never seize it: takeover happens
 * when a kernel starts with "接管系统代理" enabled. That keeps a single, obvious
 * owner for the setting instead of two buttons fighting each other.
 */

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { api, describeError, type SystemProxyStatus } from '../services/contract'

export const useSystemProxyStore = defineStore('systemProxy', () => {
  const status = ref<SystemProxyStatus | null>(null)
  const loading = ref(false)
  const busy = ref(false)
  const error = ref<string | null>(null)

  /** True when X-Matrix itself is holding the system proxy. */
  const held = computed(() => status.value?.active ?? false)
  /** True when *something* (us, V2RayN, a manual setting) routes traffic. */
  const enabled = computed(() => status.value?.enabled ?? false)

  async function refresh() {
    loading.value = true
    try {
      status.value = await api.systemProxyStatus()
      error.value = null
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  /** Restore whatever the system had before we took over. */
  async function restore() {
    busy.value = true
    try {
      status.value = await api.systemProxyDisable()
      error.value = null
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      busy.value = false
    }
  }

  return { status, loading, busy, error, held, enabled, refresh, restore }
})
