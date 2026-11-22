import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api } from '../services/contract'

export type ThemeMode = 'light' | 'dark' | 'system'

const THEME_KEY = 'xm.ui.theme'
const COLLAPSE_KEY = 'xm.ui.sidebar-collapsed'

function isThemeMode(value: string | null): value is ThemeMode {
  return value === 'light' || value === 'dark' || value === 'system'
}

/**
 * UI-only preferences. These are *not* business state, so they stay in
 * localStorage instead of the backend database (ARCHITECTURE.md §3).
 */
export const useUiStore = defineStore('ui', () => {
  const themeMode = ref<ThemeMode>('system')
  const systemPrefersDark = ref(false)
  const sidebarCollapsed = ref(false)

  /**
   * Last uncaught failure, shown as one dismissible bar at the top of the app.
   *
   * Every store already surfaces its own errors next to the thing that failed;
   * this is the net for the ones nobody caught (a rejected promise in a watcher,
   * a render error). It is deliberately a single slot: two banners stacked is
   * noise, and the second error is almost always a consequence of the first.
   */
  const lastError = ref<string | null>(null)

  function reportError(message: string) {
    lastError.value = message
  }

  function dismissError() {
    lastError.value = null
  }

  const resolvedTheme = computed<'light' | 'dark'>(() => {
    if (themeMode.value === 'system') return systemPrefersDark.value ? 'dark' : 'light'
    return themeMode.value
  })

  function applyTheme() {
    document.documentElement.dataset.theme = resolvedTheme.value
    // The title bar belongs to the OS, not to our CSS: without this the window
    // goes dark and leaves a white frame behind.
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      void api.uiSetWindowTheme(resolvedTheme.value === 'dark').catch(() => {
        // A theme that cannot be applied to the frame is cosmetic; never fatal.
      })
    }
  }

  function setTheme(mode: ThemeMode) {
    themeMode.value = mode
    localStorage.setItem(THEME_KEY, mode)
    applyTheme()
  }

  function toggleTheme() {
    setTheme(resolvedTheme.value === 'dark' ? 'light' : 'dark')
  }

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value
    localStorage.setItem(COLLAPSE_KEY, String(sidebarCollapsed.value))
  }

  /** Called once from `main.ts`, before the first paint. */
  function init() {
    const savedTheme = localStorage.getItem(THEME_KEY)
    if (isThemeMode(savedTheme)) themeMode.value = savedTheme
    sidebarCollapsed.value = localStorage.getItem(COLLAPSE_KEY) === 'true'

    const query = window.matchMedia('(prefers-color-scheme: dark)')
    systemPrefersDark.value = query.matches
    query.addEventListener('change', (event) => {
      systemPrefersDark.value = event.matches
      applyTheme()
    })

    applyTheme()
  }

  return {
    themeMode,
    resolvedTheme,
    sidebarCollapsed,
    lastError,
    reportError,
    dismissError,
    init,
    setTheme,
    toggleTheme,
    toggleSidebar,
  }
})
