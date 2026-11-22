import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  api,
  describeError,
  type Settings_Deserialize,
  type Settings_Serialize,
} from '../services/contract'

/**
 * Mirror of the settings the Rust side owns. Reads are `Settings_Serialize`
 * (every field present), writes accept the `_Deserialize` shape, so a form can
 * leave optional fields out.
 */
export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<Settings_Serialize | null>(null)
  const loading = ref(false)
  const saving = ref(false)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    loading.value = true
    error.value = null
    try {
      settings.value = await api.getSettings()
      loaded.value = true
    } catch (cause) {
      error.value = describeError(cause)
    } finally {
      loading.value = false
    }
  }

  /** Throws on rejection so the form can surface the field-level message. */
  async function save(next: Settings_Deserialize) {
    saving.value = true
    try {
      settings.value = await api.saveSettings(next)
      loaded.value = true
      return settings.value
    } finally {
      saving.value = false
    }
  }

  return { settings, loading, saving, error, loaded, load, save }
})
