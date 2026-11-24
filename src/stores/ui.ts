import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  ACCENT_VARS,
  DEFAULT_ACCENT,
  accentValues,
  resolveAccent,
  type ResolvedAccent,
} from '../services/accent'
import { api } from '../services/contract'

export type ThemeMode = 'light' | 'dark' | 'system'

const THEME_KEY = 'xm.ui.theme'
const COLLAPSE_KEY = 'xm.ui.sidebar-collapsed'
/** Accent choice lives beside the theme: both are look-and-feel, not settings. */
const ACCENT_KEY = 'xm.ui.accent'

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
   * The accent the user picked, read once at store creation.
   *
   * `resolveAccent` never throws: an unknown or corrupt entry costs the choice,
   * not the app. Presets live in CSS; a custom hex is derived per theme below.
   */
  const accent = ref<ResolvedAccent>(resolveAccent(localStorage.getItem(ACCENT_KEY)))

  /**
   * Bumped on every accent repaint, including the one that follows a theme
   * switch. Canvas widgets (ECharts) cannot inherit CSS variables, so they read
   * the tokens again whenever this changes.
   */
  const accentVersion = ref(0)

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
    // A custom accent is derived against the theme it sits on (its hover step
    // goes the other way in dark), so it must be repainted after the theme
    // attribute changed, never before.
    applyAccent()
    // The title bar belongs to the OS, not to our CSS: without this the window
    // goes dark and leaves a white frame behind.
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      void api.uiSetWindowTheme(resolvedTheme.value === 'dark').catch(() => {
        // A theme that cannot be applied to the frame is cosmetic; never fatal.
      })
    }
  }

  /**
   * Paint the accent onto the document root.
   *
   * A preset is pure CSS: set `data-accent` and clear the four inline variables
   * a previous custom colour may have left behind (inline wins over attribute
   * blocks, so forgetting this would pin the old colour forever). A custom
   * colour has no CSS block of its own, so its four values are written inline.
   */
  function applyAccent() {
    const root = document.documentElement
    if (accent.value.kind === 'preset') {
      root.dataset.accent = accent.value.id
      for (const name of ACCENT_VARS) root.style.removeProperty(name)
    } else {
      const values = accentValues(accent.value, resolvedTheme.value === 'dark')
      root.dataset.accent = 'custom'
      root.style.setProperty('--accent', values.accent)
      root.style.setProperty('--accent-hover', values.hover)
      root.style.setProperty('--accent-soft', values.soft)
      root.style.setProperty('--accent-contrast', values.contrast)
    }
    accentVersion.value += 1
  }

  /** Stored form: a preset id, or `custom:#rrggbb`. */
  function accentKey(value: ResolvedAccent): string {
    return value.kind === 'preset' ? value.id : `custom:${value.hex}`
  }

  function setAccent(value: string) {
    accent.value = resolveAccent(value)
    localStorage.setItem(ACCENT_KEY, accentKey(accent.value))
    applyAccent()
  }

  function resetAccent() {
    setAccent(DEFAULT_ACCENT)
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
    // Re-read rather than trust the value from store creation: storage may have
    // changed between the two moments (another window, a restored session).
    accent.value = resolveAccent(localStorage.getItem(ACCENT_KEY))

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
    accent,
    accentVersion,
    sidebarCollapsed,
    lastError,
    reportError,
    dismissError,
    init,
    setTheme,
    toggleTheme,
    toggleSidebar,
    setAccent,
    resetAccent,
    applyAccent,
  }
})
