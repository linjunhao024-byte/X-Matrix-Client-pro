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
  async function save(next: Settings_Deserialize, webdavPassword: string | null = null) {
    saving.value = true
    try {
      settings.value = await api.saveSettings(next, webdavPassword)
      loaded.value = true
      return settings.value
    } finally {
      saving.value = false
    }
  }

  /**
   * Persist the language alone, so the tray and the exit notification speak it
   * too. Local storage keeps the UI side; the backend keeps the wording it has
   * to print without the webview.
   */
  async function setLanguage(language: Settings_Serialize['language']) {
    const saved = await api.setLanguage(language)
    if (settings.value) settings.value = { ...settings.value, language: saved }
    return saved
  }

  return { settings, loading, saving, error, loaded, load, save, setLanguage }
})
