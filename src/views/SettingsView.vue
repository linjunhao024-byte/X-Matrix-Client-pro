<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import StaleConfig from '../components/StaleConfig.vue'
import ToggleSwitch from '../components/ToggleSwitch.vue'
import { availableLocales, locale, setLocale, t } from '../i18n'
// The webview cache is not ours to clear through `store.json`: it is WebView2's,
// so this one call goes to the Tauri webview object instead of `api.*`. It is
// gated by `core:webview:allow-clear-all-browsing-data` in capabilities.
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { comboFromKeyPress } from '../services/hotkey'
import { ACCENTS, type AccentPreset } from '../services/accent'
import {
  api,
  describeError,
  type EngineBinary,
  type EngineKind,
  type KernelRelease_Serialize,
  type ProxyMode,
  type ServiceState_Serialize,
  type Settings_Serialize,
} from '../services/contract'
import { releases as kernelUpdates } from '../services/kernelupdate'
import { useEngineStore } from '../stores/engine'
import { useSettingsStore } from '../stores/settings'
import { useSystemProxyStore } from '../stores/sysproxy'
import { useUiStore, type ThemeMode } from '../stores/ui'

const store = useSettingsStore()
const proxy = useSystemProxyStore()
const engine = useEngineStore()
const ui = useUiStore()
const route = useRoute()

/**
 * Theme and accent live in the UI store, not in the saved settings: they are
 * look-and-feel, applied before the first paint from localStorage, and a save
 * button has nothing to do with them. That is also why this section sits in its
 * own panel with no "unsaved" state.
 */
const themeModes: { value: ThemeMode; labelKey: string }[] = [
  { value: 'light', labelKey: 'settings.themeLight' },
  { value: 'dark', labelKey: 'settings.themeDark' },
  { value: 'system', labelKey: 'settings.themeSystem' },
]

/** A swatch shows the accent as it will look under the theme on screen now. */
function swatchColor(preset: AccentPreset): string {
  return ui.resolvedTheme === 'dark' ? preset.dark : preset.light
}

function isPresetSelected(id: string): boolean {
  return ui.accent.kind === 'preset' && ui.accent.id === id
}

/**
 * The custom hex field is a draft, not captured state: a malformed value must
 * never reach the store or localStorage, so the message sits here and the app
 * keeps its current accent.
 */
const customAccent = ref('')
const customAccentError = ref<string | null>(null)
const CUSTOM_HEX = /^#?([0-9a-fA-F]{6})$/
/** Placeholder comes from the table, so no hex is hard-coded in this view. */
const accentPlaceholder = ACCENTS[0].light

function applyCustomAccent(): void {
  const match = CUSTOM_HEX.exec(customAccent.value.trim())
  if (match === null) {
    customAccentError.value = t('settings.accentInvalid')
    return
  }
  ui.setAccent(`custom:#${match[1].toLowerCase()}`)
  customAccentError.value = null
}
const draft = ref<Settings_Serialize | null>(null)
const error = ref<string | null>(null)
const saved = ref(false)
const autostart = ref<boolean | null>(null)
/** Result line for "clear UI cache": what happened, in one sentence. */
const cacheBusy = ref(false)
const cacheNote = ref<string | null>(null)
/** Result line for backup/restore: what happened, in one sentence. */
const backupNote = ref<string | null>(null)
const backupBusy = ref(false)

/**
 * TUN and the system proxy both claim the machine's traffic, and the kernel
 * refuses the pair at start. TUN is the stronger claim — it takes the traffic
 * at the adapter, one layer below the registry — so switching it on drops the
 * system proxy instead of leaving an illegal pair on screen. While TUN is on
 * the proxy switch is disabled; turning TUN off hands the switch back. There
 * is no state where both are locked: a store that somehow has both on can be
 * repaired by turning TUN off, which re-enables the proxy switch.
 */
watch(
  () => draft.value?.tun_enabled,
  (on) => {
    if (on === true && draft.value?.system_proxy === true) draft.value.system_proxy = false
  },
)

/**
 * WebDAV password, kept out of `draft` on purpose: `draft` is the settings
 * document, and a password must never reach `store.json` (nor the backups it
 * produces). `touched` is what separates "left the field alone" from "cleared
 * it": the first keeps the stored secret, the second forgets it.
 */
const webdavPassword = ref('')
const webdavPasswordTouched = ref(false)
const webdavHasPassword = ref(false)
const webdavNote = ref<string | null>(null)
const webdavBusy = ref(false)

/**
 * Compare resolver lists by content, not spelling: the kernel gets the parsed
 * list, so "1.1.1.1, 8.8.8.8" and "1.1.1.1 ,8.8.8.8" are the same setting.
 */function dnsList(raw: string): string {
  return raw
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry !== '')
    .join(',')
}

/**
 * The four actions a global chord can be bound to. Mirrors `hotkeys::Chord` in
 * `src-tauri/src/hotkeys.rs`: one field per action, in the order the panel
 * lists them.
 */
type HotkeyField = 'hotkey_toggle' | 'hotkey_window' | 'hotkey_mode' | 'hotkey_test_all'

/**
 * What "恢复默认" writes back for each row.
 *
 * The authority is `default_chord` in `src-tauri/src/hotkeys.rs`: the backend
 * fills a fresh store from that table, so a change there has to be mirrored
 * here or the button would restore a chord the shipped defaults do not use.
 */
const chordDefaults: Record<HotkeyField, string> = {
  hotkey_toggle: 'Ctrl+Shift+X',
  hotkey_window: 'Ctrl+Shift+W',
  hotkey_mode: 'Ctrl+Shift+M',
  hotkey_test_all: 'Ctrl+Shift+T',
}

/** One row of the hotkey panel: the field it edits and the hint under it. */
const hotkeyRows: { field: HotkeyField; hint: string }[] = [
  { field: 'hotkey_toggle', hint: 'settings.hotkeyToggleHint' },
  { field: 'hotkey_window', hint: 'settings.hotkeyWindowHint' },
  { field: 'hotkey_mode', hint: 'settings.hotkeyModeHint' },
  { field: 'hotkey_test_all', hint: 'settings.hotkeyTestAllHint' },
]

/**
 * Which chord row is waiting for a key press, if any.
 *
 * The row turns into a recorder on click: the next real key press becomes the
 * chord. That is how every shortcut editor behaves, and it removes the need to
 * spell "Ctrl+Shift+X" by hand (or to guess whether it is case sensitive).
 */
const recording = ref<HotkeyField | null>(null)

/**
 * Take the next press as a chord.
 *
 * `preventDefault` matters: without it the press also reaches the page, where
 * Ctrl+W would close the window and Space would scroll. A press that is only a
 * modifier keeps waiting, so typing "Ctrl+…" does not end the recording early.
 */
function captureKey(field: HotkeyField, event: KeyboardEvent): void {
  event.preventDefault()
  const combo = comboFromKeyPress(event)
  if (combo === null || !draft.value) return
  draft.value = { ...draft.value, [field]: combo }
  recording.value = null
  saved.value = false
}

/** Put one row back to the chord the app ships with. */
function resetChord(field: HotkeyField): void {
  if (!draft.value) return
  draft.value = { ...draft.value, [field]: chordDefaults[field] }
  recording.value = null
  saved.value = false
}

/** Release one chord without giving it another: the action stays reachable from the UI. */
function clearChord(field: HotkeyField): void {
  if (!draft.value) return
  draft.value = { ...draft.value, [field]: '' }
  recording.value = null
  saved.value = false
}

/**
 * The i18n keys of every saved setting the running kernel was started with
 * differently.
 *
 * Ports, the log level, LAN sharing, the resolver list and multiplexing are all
 * baked into the rendered config at start, so saving a new value changes only
 * the document until the kernel restarts; "已保存" alone would look broken. One
 * notice for all of them, in panel order, so the page never stacks near
 * duplicates. A stopped kernel has nothing stale — the next start renders the
 * saved settings anyway — and an unloaded document cannot be compared.
 * Deliberately compares the saved settings, not `draft`: a value the user typed
 * but has not saved is not a divergence yet. The stream cap only counts while
 * both sides multiplex: with the feature off it is not part of the config.
 */
const staleItems = computed<string[]>(() => {
  const saved = store.settings
  const status = engine.status
  if (!engine.running || !store.loaded || saved === null || status === null) return []

  const items: string[] = []
  if (saved.mode !== status.mode) items.push('settings.stale.itemMode')
  // `http_port`/`socks_port` are 0 in the store when they follow the mixed
  // port, but the status always reports the effective numbers: resolve the
  // saved side before comparing, or a default install reads as stale forever.
  const savedHttp = saved.http_port || saved.mixed_port
  const savedSocks = saved.socks_port || saved.mixed_port
  if (
    saved.mixed_port !== status.mixed_port ||
    saved.api_port !== status.api_port ||
    savedHttp !== status.http_port ||
    savedSocks !== status.socks_port
  ) {
    items.push('settings.stale.itemPorts')
  }
  if (saved.log_level !== status.log_level) items.push('settings.stale.itemLog')
  if (saved.allow_lan !== status.allow_lan) items.push('settings.stale.itemLan')
  if (
    saved.mux_enabled !== status.mux_enabled ||
    (saved.mux_enabled && saved.mux_concurrency !== status.mux_concurrency)
  ) {
    items.push('settings.stale.itemMux')
  }
  if (
    saved.fragment_enabled !== status.fragment_enabled ||
    (saved.fragment_enabled &&
      (saved.fragment_length !== status.fragment_length ||
        saved.fragment_interval !== status.fragment_interval))
  ) {
    items.push('settings.stale.itemFragment')
  }
  if (dnsList(saved.dns_servers) !== dnsList(status.dns_servers)) {
    items.push('settings.stale.itemDns')
  }
  // Only sing-box writes this into the config (every `urltest` group); Xray
  // never sees it, so claiming it is pending while Xray runs would be a lie.
  if (engine.kind === 'sing-box' && saved.test_url !== status.test_url) {
    items.push('settings.stale.itemTestUrl')
  }
  return items
})

const staleMessage = computed(() =>
  t('settings.staleList', {
    items: staleItems.value.map((key) => t(key)).join(t('settings.stale.sep')),
  }),
)

const modes: { value: ProxyMode; labelKey: string }[] = [
  { value: 'rule', labelKey: 'settings.mode.rule' },
  { value: 'global', labelKey: 'settings.mode.global' },
  { value: 'direct', labelKey: 'settings.mode.direct' },
]

/**
 * Each language names itself, so these labels are not translated.
 */
const locales = availableLocales()

/**
 * Switch the interface language, and tell the backend about it.
 *
 * Two stores hold this on purpose: `localStorage` decides what the pages render
 * (it has to be readable before the first paint), while `settings.language`
 * decides what the tray menu and the kernel-exit notification say, since those
 * are assembled in Rust with no webview to ask. This is the one place that
 * writes both, so they can never disagree.
 */
async function chooseLocale(next: typeof locale.value): Promise<void> {
  setLocale(next)
  try {
    await store.setLanguage(next)
  } catch (cause) {
    error.value = describeError(cause)
  }
}

/** Kernel verbosity. `warn` is the default because a debug log grows fast. */
/** `0` is "only when I press the button" — see `Settings::auto_test_minutes`. */
const testIntervals: { value: number; labelKey: string }[] = [
  { value: 0, labelKey: 'settings.autoTest.off' },
  { value: 5, labelKey: 'settings.autoTest.min5' },
  { value: 15, labelKey: 'settings.autoTest.min15' },
  { value: 60, labelKey: 'settings.autoTest.min60' },
]

const logLevels: { value: Settings_Serialize['log_level']; labelKey: string }[] = [
  { value: 'warn', labelKey: 'settings.logLevel.warn' },
  { value: 'info', labelKey: 'settings.logLevel.info' },
  { value: 'debug', labelKey: 'settings.logLevel.debug' },
]

const autostartState = computed(() =>
  autostart.value === null
    ? t('settings.autostartUnknown')
    : autostart.value
      ? t('settings.autostartOn')
      : t('settings.autostartOff'),
)

/**
 * Export the whole configuration to a file the user picks.
 *
 * A cancelled dialog is not a failure, so it says nothing rather than painting
 * a red line.
 */
async function exportConfig(): Promise<void> {
  backupBusy.value = true
  backupNote.value = null
  try {
    const path = await api.backupSave()
    if (path) backupNote.value = t('settings.backup.saved', { path })
  } catch (error) {
    backupNote.value = describeError(error)
  } finally {
    backupBusy.value = false
  }
}

/**
 * Replace the configuration from a backup file.
 *
 * The kernel must be stopped first (the backend refuses otherwise), because a
 * running kernel was rendered from the settings we are about to replace.
 */
async function restoreConfig(): Promise<void> {
  backupBusy.value = true
  backupNote.value = null
  try {
    const summary = await api.backupLoad()
    if (summary) {
      // specta renders numeric fields as `number | null`; a count that arrived
      // is never null, but the type says otherwise.
      backupNote.value = t('settings.backup.restored', {
        nodes: summary.nodes ?? 0,
        rules: summary.rules ?? 0,
        groups: summary.groups ?? 0,
        ruleSets: summary.rule_sets ?? 0,
      })
      // Everything on screen came from the old document.
      await store.load(true)
      if (store.settings) draft.value = { ...store.settings }
    }
  } catch (error) {
    backupNote.value = describeError(error)
  } finally {
    backupBusy.value = false
  }
}

/**
 * Clear the webview's cache and cookies.
 *
 * Nothing in `store.json` changes — this is WebView2's own storage — so the
 * call cannot go through `api.*`. The same storage holds the theme/accent
 * preferences, which is why the hint next to the button says they fall back to
 * their defaults rather than pretending the action is invisible.
 */
async function clearCache(): Promise<void> {
  cacheBusy.value = true
  cacheNote.value = null
  try {
    await getCurrentWebview().clearAllBrowsingData()
    cacheNote.value = t('settings.cacheCleared')
  } catch (error) {
    // Reported as-is: printing "cleared" after a rejected capability would tell
    // the user their cache is gone when nothing happened.
    cacheNote.value = describeError(error)
  } finally {
    cacheBusy.value = false
  }
}

/**
 * Whether a password is stored, for the field's placeholder. The password
 * itself never comes back over IPC, so "stored" is all the page can know.
 */
async function refreshWebdav(): Promise<void> {
  try {
    const state = await api.webdavState()
    webdavHasPassword.value = state.has_password
  } catch {
    // The block still works without this hint; failing to read it must not
    // paint an error over a settings page that is otherwise fine.
  }
}

/** Upload the configuration to the configured WebDAV folder. */
async function uploadBackup(): Promise<void> {
  webdavBusy.value = true
  webdavNote.value = null
  try {
    const result = await api.webdavUpload()
    webdavNote.value = t('settings.webdav.uploaded', {
      bytes: result.bytes ?? 0,
      url: result.url,
    })
  } catch (error) {
    webdavNote.value = describeError(error)
  } finally {
    webdavBusy.value = false
  }
}

/**
 * Replace the configuration with the cloud copy.
 *
 * Same rule as a file restore: the backend refuses while a kernel runs, because
 * that kernel was rendered from the document being replaced.
 */
async function restoreBackup(): Promise<void> {
  webdavBusy.value = true
  webdavNote.value = null
  try {
    const summary = await api.webdavRestore()
    if (summary) {
      webdavNote.value = t('settings.webdav.restored', {
        nodes: summary.nodes ?? 0,
        rules: summary.rules ?? 0,
        groups: summary.groups ?? 0,
        ruleSets: summary.rule_sets ?? 0,
      })
      await store.load(true)
      if (store.settings) draft.value = { ...store.settings }
      await refreshWebdav()
    }
  } catch (error) {
    webdavNote.value = describeError(error)
  } finally {
    webdavBusy.value = false
  }
}

async function refreshAutostart() {
  try {
    autostart.value = await api.autostartStatus()
  } catch {
    autostart.value = null
  }
}

/**
 * Whether the process is running with an administrator token.
 *
 * `null` means the question has not been answered yet — not `false`. The status
 * line and the restart button both wait for a real answer instead of guessing.
 */
const elevated = ref<boolean | null>(null)
/** Backend wording from a restart that did not happen, shown in that same line. */
const elevateError = ref<string | null>(null)
const elevateBusy = ref(false)

async function refreshElevated() {
  try {
    elevated.value = await api.appElevated()
  } catch {
    // No answer is not 「已提权」: leave it unread so neither status prints.
    elevated.value = null
  }
}

/**
 * Restart through the UAC consent dialog, then let the backend quit this one.
 *
 * On success nothing here runs afterwards (the window is on its way out), so
 * only a refused or failed relaunch reaches the status line — using the
 * backend's own sentence rather than a rewritten one.
 */
async function restartElevated(): Promise<void> {
  if (elevateBusy.value || elevated.value !== false) return
  elevateBusy.value = true
  elevateError.value = null
  try {
    await api.restartElevated()
  } catch (cause) {
    elevateError.value = describeError(cause)
  } finally {
    elevateBusy.value = false
  }
}

/**
 * The single line under the 权限 heading.
 *
 * A failed restart replaces the status instead of stacking a second sentence:
 * the backend explains why nothing restarted, and the elevation state itself
 * has not changed.
 */
const elevateLine = computed(() => {
  if (elevateError.value) return elevateError.value
  if (elevated.value === true) return t('settings.elevate.isAdmin')
  if (elevated.value === false) return t('settings.elevate.notAdmin')
  return null
})

/**
 * The kernel service: `null` until the backend answers, then the whole state.
 *
 * The state is read from the service control manager, so it is the real answer
 * to 「装了没有 / 在不在跑」 rather than an echo of what we asked for.
 */
const service = ref<ServiceState_Serialize | null>(null)
const serviceError = ref<string | null>(null)
const serviceBusy = ref(false)

async function refreshService() {
  try {
    service.value = await api.serviceState()
  } catch {
    service.value = null
  }
}

/**
 * Install the service, or remove it.
 *
 * Both need an administrator token, and both are refused by the backend when
 * the token is ordinary — the wording comes from Rust so it reads the same as
 * a failed restart. On success the backend re-probes, so a kernel started from
 * now on is handed to the service (or spawned here again after a removal).
 */
async function installService(): Promise<void> {
  if (serviceBusy.value) return
  serviceBusy.value = true
  serviceError.value = null
  try {
    service.value = await api.serviceInstall()
  } catch (cause) {
    serviceError.value = describeError(cause)
  } finally {
    serviceBusy.value = false
  }
}

async function uninstallService(): Promise<void> {
  if (serviceBusy.value) return
  serviceBusy.value = true
  serviceError.value = null
  try {
    service.value = await api.serviceUninstall()
  } catch (cause) {
    serviceError.value = describeError(cause)
  } finally {
    serviceBusy.value = false
  }
}

/** What the service line says: the backend's own sentence, or a fault. */
const serviceLine = computed(() => {
  if (serviceError.value) return serviceError.value
  return service.value?.detail ?? null
})

/**
 * The two kernels, in the order a user meets them.
 *
 * `Xray` and `sing-box` are product names, so they are not translated — the
 * same rule the language list follows.
 */
const kernels: { kind: EngineKind; label: string }[] = [
  { kind: 'xray', label: 'Xray' },
  { kind: 'sing-box', label: 'sing-box' },
]

/** Which settings field carries the override for each kernel. */
const overrideField: Record<EngineKind, 'xray_binary' | 'singbox_binary'> = {
  xray: 'xray_binary',
  'sing-box': 'singbox_binary',
}

/** What the runtime would actually start, per kernel, as last saved. */
const kernelBinaries = ref<EngineBinary[]>([])
/** Kind currently talking to the backend — a dialog or a `version` run. */
const kernelBusy = ref<EngineKind | null>(null)
/** One line per kernel: the reported version, or why there is none. */
const kernelNotes = ref<Partial<Record<EngineKind, string>>>({})

const kernelRows = computed(() =>
  kernels.map((kernel) => ({
    ...kernel,
    binary: kernelBinaries.value.find((item) => item.kind === kernel.kind),
    override: (draft.value?.[overrideField[kernel.kind]] ?? '').trim(),
  })),
)

async function refreshKernelBinaries(): Promise<void> {
  try {
    kernelBinaries.value = await api.engineBinaries()
  } catch (cause) {
    kernelNotes.value.xray = describeError(cause)
  }
}

/**
 * Persist one override (or clear it) through the settings store.
 *
 * Saving is what applies the path to the running runtime, so the row must not
 * only change the draft: the next 启动 has to use the chosen file without an
 * app restart. Both the picker and 恢复默认 go through here.
 */
async function saveKernelOverride(kind: EngineKind, path: string): Promise<void> {
  if (!draft.value) return
  kernelBusy.value = kind
  try {
    await store.save({ ...draft.value, [overrideField[kind]]: path })
    draft.value = { ...(store.settings as Settings_Serialize) }
    // The old note described the previous file; it would now be a lie.
    delete kernelNotes.value[kind]
    await refreshKernelBinaries()
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    kernelBusy.value = null
  }
}

async function chooseKernel(kind: EngineKind): Promise<void> {
  if (!draft.value) return
  kernelBusy.value = kind
  let chosen: string | null = null
  try {
    chosen = await api.pickEngineBinary(kind)
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    kernelBusy.value = null
  }
  // Cancelling the dialog is a decision, not a failure: keep everything as is.
  if (chosen) await saveKernelOverride(kind, chosen)
}

async function resetKernel(kind: EngineKind): Promise<void> {
  await saveKernelOverride(kind, '')
}

/**
 * Ask the kernel to report itself.
 *
 * A missing file comes back as an error naming the path — that is the honest
 * answer, and it is shown in the same line instead of a version.
 */
async function probeKernelVersion(kind: EngineKind): Promise<void> {
  kernelBusy.value = kind
  try {
    kernelNotes.value[kind] = await api.engineVersion(kind)
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    kernelBusy.value = null
  }
}

/** Newest official release per kernel, as last checked in this session. */
const kernelReleases = ref<Partial<Record<EngineKind, KernelRelease_Serialize>>>({})
/** Kind whose download is in flight — the only button that says 正在下载… */
const installing = ref<EngineKind | null>(null)

/**
 * Seed the rows with the lookup the app already made at startup.
 *
 * Asking again on arrival would be a second network round for the same answer.
 * The values are the backend's own; a manual 检查更新 still replaces them, and
 * the 更新 button appears without that extra click when a newer build exists.
 */
function seedKernelReleases(source: Partial<Record<EngineKind, KernelRelease_Serialize>>): void {
  for (const { kind } of kernels) {
    const release = source[kind]
    if (!release) continue
    kernelReleases.value[kind] = release
    // Say the same one-liner a manual check would. Skip when this session
    // already has something more specific (a check result, an install report):
    // the startup answer is the oldest news there is.
    if (!kernelNotes.value[kind]) kernelNotes.value[kind] = describeRelease(release)
  }
}
seedKernelReleases(kernelUpdates.value)
// The startup check may still be in flight when this page opens; take its
// answer when it lands rather than leaving the block blank until a manual click.
watch(kernelUpdates, (next) => seedKernelReleases(next))

/** The startup lookup's verdict for the selected kernel, when it is news. */
const kernelAutoRelease = computed<{ version: string; current: string } | null>(() => {
  const release = kernelUpdates.value[engine.kind]
  if (!release?.current || release.current === release.version) return null
  return { version: release.version, current: release.current }
})

/**
 * Is there a newer official build than the one on this machine?
 *
 * Nothing is offered before a check has run: without the newest version there
 * is nothing to compare against, and a button that guesses would be a lie.
 */
function outdated(kind: EngineKind): boolean {
  const release = kernelReleases.value[kind]
  if (!release) return false
  return release.version !== release.current
}

/**
 * One line describing a checked release.
 *
 * `current` is what the local binary reports. When there is none the file is
 * missing, which is not the same as being out of date and must not be dressed
 * up as one.
 */
function describeRelease(release: KernelRelease_Serialize): string {
  const latest = `${t('settings.kernel.latest')} ${release.version}`
  const line = !release.current
    ? `${latest} · ${t('settings.kernel.missing')}`
    : release.current === release.version
      ? t('settings.kernel.isLatest')
      : `${latest} · ${t('settings.kernel.current')} ${release.current}`
  // A direct fetch while 订阅走代理 is still a success, but the user asked for
  // the proxy and did not get it — say so instead of hiding it.
  return release.note ? `${line}；${release.note}` : line
}

/** Ask GitHub what the newest official build is; show it beside the local one. */
async function checkKernel(kind: EngineKind): Promise<void> {
  kernelBusy.value = kind
  try {
    const release = await api.kernelCheck(kind)
    kernelReleases.value[kind] = release
    kernelNotes.value[kind] = describeRelease(release)
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    kernelBusy.value = null
  }
}

/**
 * Download and swap in the newest official kernel.
 *
 * The backend may stop and restart the kernel being replaced, so every row
 * stays disabled for the whole call. Failures are shown verbatim: the reason
 * (checksum mismatch, unreadable config, a kernel that will not start) decides
 * what the user does next.
 */
async function installKernel(kind: EngineKind): Promise<void> {
  kernelBusy.value = kind
  installing.value = kind
  try {
    const update = await api.kernelInstall(kind)
    // Keep the cached latest honest: the file on disk is now `update.version`,
    // so the 更新 button has to disappear without a second network round.
    const release = kernelReleases.value[kind]
    if (release) kernelReleases.value[kind] = { ...release, current: update.version }
    const hash = update.sha256.slice(0, 12)
    const checksum = update.verified
      ? t('settings.kernel.checksumOk', { hash })
      : t('settings.kernel.checksumLocal', { hash })
    const parts = [t('settings.kernel.updated', { version: update.version }), checksum]
    // e.g. 替换成功，但内核没起来：… — the swap worked, the restart did not.
    if (update.note) parts.push(update.note)
    kernelNotes.value[kind] = parts.join(' · ')
    await refreshKernelBinaries()
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    installing.value = null
    kernelBusy.value = null
  }
}

onMounted(async () => {
  await store.load()
  if (store.settings) draft.value = { ...store.settings }
  await proxy.refresh()
  await refreshAutostart()
  await refreshElevated()
  await refreshService()
  await refreshKernelBinaries()
  await refreshWebdav()
  // Home's "打开设置" arrives with `#kernel`; without this the block it points
  // at usually sits below the fold. The router has no scrollBehavior because
  // `.page` scrolls its own container, not the window.
  if (route.hash === '#kernel') {
    await nextTick()
    document.getElementById('kernel')?.scrollIntoView({ block: 'start' })
  }
})

async function save() {
  if (!draft.value) return
  error.value = null
  saved.value = false
  try {
    // `null` means "keep what is stored"; `''` means "forget it". Both only
    // ever happen on an explicit edit, so an untouched field is not a silent
    // erase of a working password.
    await store.save(
      { ...draft.value },
      webdavPasswordTouched.value ? webdavPassword.value : null,
    )
    draft.value = { ...(store.settings as Settings_Serialize) }
    // The secret now lives in the Credential Manager; the field keeps nothing.
    webdavPassword.value = ''
    webdavPasswordTouched.value = false
    await refreshWebdav()
    saved.value = true
    await refreshAutostart()
    // Saving may have just taken or released the system proxy (the backend
    // applies it to a running kernel immediately), so the live block below the
    // toggle must show the machine as it is now, not as it was on mount.
    await proxy.refresh()
    window.setTimeout(() => {
      saved.value = false
    }, 2000)
  } catch (cause) {
    error.value = describeError(cause)
  }
}
</script>

<template>
  <div class="page settings-page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.settings.title') }}</h1>
        <p class="page-subtitle">{{ t('page.settings.subtitle') }}</p>
      </div>
      <div class="row">
        <span v-if="saved" class="badge badge-success">{{ t('common.saved') }}</span>
        <button class="btn btn-primary" type="button" :disabled="!draft || store.saving" @click="save">
          {{ store.saving ? t('common.saving') : t('common.save') }}
        </button>
      </div>
    </header>

    <div class="page-body">
    <p v-if="store.error" class="alert">{{ store.error }}</p>
    <p v-if="error" class="alert">{{ error }}</p>

    <section v-if="!draft" class="surface-plain">
      <div class="empty-state">
        <div class="empty-state-title">{{ t('common.loading') }}</div>
      </div>
    </section>

    <template v-else>
      <!-- One notice for the whole page, at the top: it lists every setting the
           running kernel is behind on, so scattering copies next to each panel
           would repeat the same "not live yet" sentence. -->
      <StaleConfig
        :show="staleItems.length > 0"
        :message="staleMessage"
        :apply-label="t('settings.staleListApply')"
      />

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.ports') }}</h2>
        <div class="grid">
          <label class="field">
            <span class="field-label">{{ t('settings.mixed') }}</span>
            <input v-model.number="draft.mixed_port" class="input input-narrow" type="number" min="1024" max="65535" />
            <span class="field-hint">{{ t('settings.mixedHint') }}</span>
          </label>
          <label class="field">
            <span class="field-label">{{ t('settings.api') }}</span>
            <input v-model.number="draft.api_port" class="input input-narrow" type="number" min="1024" max="65535" />
            <span class="field-hint">{{ t('settings.apiHint') }}</span>
          </label>
        </div>
        <div class="grid port-split">
          <label class="field">
            <span class="field-label">{{ t('settings.httpPort') }}</span>
            <input v-model.number="draft.http_port" class="input input-narrow" type="number" min="0" max="65535" />
          </label>
          <label class="field">
            <span class="field-label">{{ t('settings.socksPort') }}</span>
            <input v-model.number="draft.socks_port" class="input input-narrow" type="number" min="0" max="65535" />
          </label>
        </div>
        <p class="field-hint port-hint">
          {{ t('settings.portSplitHint') }}
          <span v-if="engine.running && engine.status" class="mono port-now">
            HTTP {{ engine.status.http_port }} · SOCKS {{ engine.status.socks_port }}
          </span>
        </p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.mode') }}</h2>
        <div class="segmented">
          <button
            v-for="mode in modes"
            :key="mode.value"
            type="button"
            :aria-pressed="draft.mode === mode.value"
            @click="draft.mode = mode.value"
          >
            {{ t(mode.labelKey) }}
          </button>
        </div>
        <p class="field-hint">
          {{ t('settings.modeHint') }}
        </p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.system') }}</h2>
        <ToggleSwitch
          v-model="draft.allow_lan"
          :label="t('settings.allowLan')"
          :description="t('settings.allowLanHint')"
        />
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.system_proxy"
          :label="t('settings.systemProxy')"
          :description="t('settings.systemProxyHint')"
          :disabled="draft.tun_enabled"
        />
        <div class="divider" />
        <ToggleSwitch v-model="draft.tun_enabled" :label="t('settings.tun.title')" />
        <label class="field">
          <span class="field-label">{{ t('settings.tun.stack') }}</span>
          <select
            v-model="draft.tun_stack"
            class="select input-narrow"
            :disabled="!draft.tun_enabled"
          >
            <option value="system">{{ t('settings.tun.stackSystem') }}</option>
            <option value="gvisor">{{ t('settings.tun.stackGvisor') }}</option>
            <option value="mixed">{{ t('settings.tun.stackMixed') }}</option>
          </select>
        </label>
        <p class="field-hint">{{ t('settings.tun.hint') }}</p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.pac_enabled"
          :label="t('settings.pac')"
          :description="t('settings.pacHint')"
          :disabled="!draft.system_proxy"
        />
        <div class="proxy-state">
          <span class="muted">
            {{ t('settings.pacState') }}
            <strong class="mono">{{ proxy.status?.pac_url ?? t('settings.pacNone') }}</strong>
          </span>
        </div>
        <p class="field-hint">{{ t('settings.pacLimit') }}</p>
        <p v-if="draft.pac_enabled && !engine.running" class="field-hint">
          {{ t('settings.pacRestart') }}
        </p>
        <div class="proxy-state">
          <span class="muted">
            {{ t('settings.proxyState') }}
            <strong>{{ proxy.enabled ? t('settings.proxyOn') : t('settings.proxyOff') }}</strong>
            <template v-if="proxy.status?.server">{{
              t('settings.proxyServer', { server: proxy.status.server })
            }}</template>
            <template v-if="proxy.held">{{ t('settings.proxyHeldSuffix') }}</template>
          </span>
          <button
            v-if="proxy.held"
            class="btn btn-sm"
            type="button"
            :disabled="proxy.busy"
            @click="proxy.restore()"
          >
            {{ t('settings.restoreProxy') }}
          </button>
        </div>
        <p v-if="proxy.error" class="alert">{{ proxy.error }}</p>
        <label class="field">
          <span class="field-label">{{ t('settings.bypass') }}</span>
          <input v-model="draft.proxy_bypass" class="input input-mono" type="text" />
        </label>
        <p class="field-hint">{{ t('settings.bypassHint') }}</p>
        <p class="field-hint">
          {{ t('settings.proxyIntent') }}
          {{ t('settings.proxyWarning') }}
        </p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.auto_start"
          :label="t('settings.autostart')"
          :description="t('settings.autostartHint')"
        />
        <p class="field-hint">
          {{ t('settings.autostartState', { state: autostartState }) }}
          {{ t('settings.autostartNote') }}
        </p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.auto_connect"
          :label="t('settings.autoConnect')"
          :description="t('settings.autoConnectHint')"
        />
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.start_minimized"
          :label="t('settings.startMinimized')"
          :description="t('settings.startMinimizedHint')"
        />
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.close_to_tray"
          :label="t('settings.closeToTray')"
          :description="t('settings.closeToTrayHint')"
        />
        <div class="divider" />
        <ToggleSwitch v-model="draft.lite_mode" :label="t('settings.liteMode')" />
        <p class="field-hint">{{ t('settings.liteHint') }}</p>
        <div class="divider" />
        <div class="row">
          <button class="btn btn-ghost btn-sm" type="button" :disabled="cacheBusy" @click="clearCache()">
            {{ t('settings.clearCache') }}
          </button>
        </div>
        <p class="field-hint">{{ t('settings.clearCacheHint') }}</p>
        <p v-if="cacheNote" class="field-hint">{{ cacheNote }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.elevate.title') }}</h2>
        <p v-if="elevateLine" class="field-hint" :class="{ 'elevate-failed': elevateError }">
          {{ elevateLine }}
        </p>
        <div class="row">
          <button
            class="btn btn-sm"
            type="button"
            :disabled="elevated !== false || elevateBusy"
            @click="restartElevated()"
          >
            {{ t('settings.elevate.restart') }}
          </button>
        </div>
        <div class="divider" />
        <ToggleSwitch v-model="draft.prompt_elevation" :label="t('settings.elevate.prompt')" />
        <p class="field-hint">{{ t('settings.elevate.promptHint') }}</p>
        <div class="divider" />
        <h3 class="service-title">{{ t('settings.service.title') }}</h3>
        <p v-if="serviceLine" class="field-hint" :class="{ 'elevate-failed': serviceError }">
          {{ serviceLine }}
        </p>
        <div class="row">
          <button
            class="btn btn-sm"
            type="button"
            :disabled="serviceBusy || service === null || elevated === false || service.installed"
            @click="installService()"
          >
            {{ t('settings.service.install') }}
          </button>
          <button
            class="btn btn-ghost btn-sm"
            type="button"
            :disabled="serviceBusy || service === null || elevated === false || !service.installed"
            @click="uninstallService()"
          >
            {{ t('settings.service.uninstall') }}
          </button>
        </div>
        <p v-if="service?.running" class="field-hint">{{ t('settings.service.installed') }}</p>
        <p class="field-hint">{{ t('settings.service.hint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.mux') }}</h2>
        <ToggleSwitch v-model="draft.mux_enabled" :label="t('settings.mux')" />
        <div class="divider" />
        <label class="field">
          <span class="field-label">{{ t('settings.muxConcurrency') }}</span>
          <input
            v-model.number="draft.mux_concurrency"
            class="input input-narrow"
            type="number"
            min="1"
            max="128"
            step="1"
            :disabled="!draft.mux_enabled"
          />
        </label>
        <p class="field-hint">{{ t('settings.muxHint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.fragment') }}</h2>
        <ToggleSwitch v-model="draft.fragment_enabled" :label="t('settings.fragment')" />
        <div class="divider" />
        <label class="field">
          <span class="field-label">{{ t('settings.fragmentLength') }}</span>
          <input
            v-model="draft.fragment_length"
            class="input input-narrow"
            type="text"
            placeholder="100-200"
            :disabled="!draft.fragment_enabled"
          />
        </label>
        <label class="field">
          <span class="field-label">{{ t('settings.fragmentInterval') }}</span>
          <input
            v-model="draft.fragment_interval"
            class="input input-narrow"
            type="text"
            placeholder="10-20"
            :disabled="!draft.fragment_enabled"
          />
        </label>
        <p class="field-hint">{{ t('settings.fragmentHint') }}</p>
      </section>

      <section id="kernel" class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.kernel.title') }}</h2>
        <p class="field-hint">{{ t('settings.kernel.hint') }}</p>
        <!-- A real switch: off means `autoCheck` returns before it sends
             anything, not that the banner is merely hidden. -->
        <ToggleSwitch
          :model-value="ui.kernelUpdateCheck"
          :label="t('settings.kernel.autoCheck')"
          :description="t('settings.kernel.autoCheckHint')"
          @update:model-value="ui.setKernelUpdateCheck($event)"
        />
        <p v-if="kernelAutoRelease" class="field-hint">
          {{ t('home.kernelUpdate.new', { version: kernelAutoRelease.version, current: kernelAutoRelease.current }) }}
        </p>
        <div v-for="row in kernelRows" :key="row.kind" class="kernel-row">
          <div class="kernel-head">
            <span class="field-label">{{ row.label }}</span>
            <span class="badge" :class="row.binary?.present ? 'badge-success' : 'badge-warning'">
              {{ row.binary?.present ? t('settings.kernel.present') : t('settings.kernel.missing') }}
            </span>
            <span class="badge badge-accent">
              {{ row.override ? t('settings.kernel.custom') : t('settings.kernel.default') }}
            </span>
          </div>
          <code class="kernel-path">{{ row.binary?.path ?? '—' }}</code>
          <div class="row">
            <button
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null"
              @click="chooseKernel(row.kind)"
            >
              {{ t('settings.kernel.choose') }}
            </button>
            <button
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null || !row.override"
              @click="resetKernel(row.kind)"
            >
              {{ t('settings.kernel.reset') }}
            </button>
            <button
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null"
              @click="probeKernelVersion(row.kind)"
            >
              {{ t('settings.kernel.version') }}
            </button>
            <button
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null"
              @click="checkKernel(row.kind)"
            >
              {{ t('settings.kernel.check') }}
            </button>
            <button
              v-if="outdated(row.kind)"
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null"
              @click="installKernel(row.kind)"
            >
              {{
                installing === row.kind
                  ? t('settings.kernel.installing')
                  : t('settings.kernel.install')
              }}
            </button>
          </div>
          <p v-if="kernelNotes[row.kind]" class="field-hint">{{ kernelNotes[row.kind] }}</p>
        </div>
        <p class="field-hint">{{ t('settings.kernel.note') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.logLevel') }}</h2>
        <div class="segmented">
          <button
            v-for="level in logLevels"
            :key="level.value"
            type="button"
            :class="{ active: draft.log_level === level.value }"
            :aria-pressed="draft.log_level === level.value"
            @click="draft.log_level = level.value"
          >
            {{ t(level.labelKey) }}
          </button>
        </div>
        <p class="field-hint">{{ t('settings.logLevelHint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.autoTest') }}</h2>
        <div class="segmented">
          <button
            v-for="option in testIntervals"
            :key="option.value"
            type="button"
            :class="{ active: draft.auto_test_minutes === option.value }"
            :aria-pressed="draft.auto_test_minutes === option.value"
            @click="draft.auto_test_minutes = option.value"
          >
            {{ t(option.labelKey) }}
          </button>
        </div>
        <p class="field-hint">{{ t('settings.autoTestHint') }}</p>
        <div class="field">
          <span class="field-label">{{ t('settings.testUrl') }}</span>
          <input v-model="draft.test_url" class="input input-mono" type="text" />
        </div>
        <p class="field-hint">{{ t('settings.testUrlHint') }}</p>
        <div class="grid">
          <label class="field">
            <span class="field-label">{{ t('settings.testTimeout') }}</span>
            <input
              v-model.number="draft.test_timeout_ms"
              class="input input-narrow"
              type="number"
              min="500"
              max="20000"
              step="500"
            />
          </label>
          <label class="field">
            <span class="field-label">{{ t('settings.testConcurrency') }}</span>
            <input
              v-model.number="draft.test_concurrency"
              class="input input-narrow"
              type="number"
              min="1"
              max="16"
              step="1"
            />
          </label>
        </div>
        <p class="field-hint">{{ t('settings.testBudgetHint') }}</p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.test_on_start"
          :label="t('settings.testOnStart')"
          :description="t('settings.testOnStartHint')"
        />
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.subAgent') }}</h2>
        <div class="field">
          <span class="field-label">{{ t('settings.subAgentLabel') }}</span>
          <input
            v-model="draft.subscription_user_agent"
            class="input input-mono"
            type="text"
            :placeholder="t('settings.subAgentPlaceholder')"
          />
        </div>
        <p class="field-hint">{{ t('settings.subAgentHint') }}</p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.subscription_via_proxy"
          :label="t('settings.subViaProxy')"
          :description="t('settings.subViaProxyHint')"
        />
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.dns.title') }}</h2>
        <div class="field">
          <span class="field-label">{{ t('settings.dns.servers') }}</span>
          <input
            v-model="draft.dns_servers"
            class="input input-mono"
            type="text"
            :placeholder="t('settings.dns.placeholder')"
          />
        </div>
        <p class="field-hint">{{ t('settings.dns.hint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.backup.title') }}</h2>
        <p class="field-hint">{{ t('settings.backup.hint') }}</p>
        <div class="row">
          <button class="btn btn-sm" type="button" :disabled="backupBusy" @click="exportConfig()">
            {{ t('settings.backup.export') }}
          </button>
          <button class="btn btn-sm" type="button" :disabled="backupBusy" @click="restoreConfig()">
            {{ t('settings.backup.restore') }}
          </button>
        </div>
        <p v-if="backupNote" class="field-hint">{{ backupNote }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.webdav.title') }}</h2>
        <p class="field-hint">{{ t('settings.webdav.hint') }}</p>
        <div class="grid" v-if="draft">
          <label class="field">
            <span>{{ t('settings.webdav.address') }}</span>
            <input
              v-model="draft.webdav_url"
              class="input input-mono"
              type="text"
              :placeholder="t('settings.webdav.addressPlaceholder')"
            />
          </label>
          <label class="field">
            <span>{{ t('settings.webdav.username') }}</span>
            <input v-model="draft.webdav_username" class="input" type="text" autocomplete="username" />
          </label>
          <label class="field">
            <span>{{ t('settings.webdav.password') }}</span>
            <input
              v-model="webdavPassword"
              class="input"
              type="password"
              autocomplete="current-password"
              :placeholder="
                webdavHasPassword
                  ? t('settings.webdav.passwordStored')
                  : t('settings.webdav.passwordEmpty')
              "
              @input="webdavPasswordTouched = true"
            />
          </label>
        </div>
        <p class="field-hint">{{ t('settings.webdav.passwordHint') }}</p>
        <div v-if="draft">
          <ToggleSwitch
            v-model="draft.webdav_via_proxy"
            :label="t('settings.webdav.viaProxy')"
            :description="t('settings.webdav.viaProxyHint')"
          />
        </div>
        <div class="row">
          <button class="btn btn-sm" type="button" :disabled="webdavBusy" @click="uploadBackup()">
            {{ t('settings.webdav.upload') }}
          </button>
          <button class="btn btn-sm" type="button" :disabled="webdavBusy" @click="restoreBackup()">
            {{ t('settings.webdav.restore') }}
          </button>
        </div>
        <p v-if="webdavNote" class="field-hint">{{ webdavNote }}</p>
      </section>

      <p class="field-hint">
        {{ t('settings.tray') }}
      </p>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.hotkeysTitle') }}</h2>
        <p class="field-hint">{{ t('settings.hotkeysHint') }}</p>
        <template v-if="draft">
          <div v-for="row in hotkeyRows" :key="row.field" class="hotkey-row">
            <button
              type="button"
              class="hotkey-chip"
              :class="{ 'hotkey-recording': recording === row.field }"
              :title="t('settings.hotkeyCapture')"
              @click="recording = row.field"
              @blur="recording = null"
              @keydown="captureKey(row.field, $event)"
            >
              {{ draft[row.field] || t('settings.hotkeyEmpty') }}
            </button>
            <span>{{ t(row.hint) }}</span>
            <span class="hotkey-actions">
              <button class="btn btn-sm" type="button" @click="resetChord(row.field)">
                {{ t('settings.hotkeyReset') }}
              </button>
              <button class="btn btn-sm" type="button" @click="clearChord(row.field)">
                {{ t('settings.hotkeyClear') }}
              </button>
            </span>
          </div>
        </template>
        <p class="field-hint">{{ t('settings.hotkeySaveHint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.appearance') }}</h2>
        <div class="segmented">
          <button
            v-for="mode in themeModes"
            :key="mode.value"
            type="button"
            :class="{ active: ui.themeMode === mode.value }"
            :aria-pressed="ui.themeMode === mode.value"
            @click="ui.setTheme(mode.value)"
          >
            {{ t(mode.labelKey) }}
          </button>
        </div>
        <div class="field">
          <span class="field-label">{{ t('settings.accent') }}</span>
          <div class="accent-row">
            <button
              v-for="preset in ACCENTS"
              :key="preset.id"
              type="button"
              class="accent-swatch"
              :class="{ active: isPresetSelected(preset.id) }"
              :aria-label="preset.id"
              :aria-pressed="isPresetSelected(preset.id)"
              :title="preset.id"
              :style="{ background: swatchColor(preset) }"
              @click="ui.setAccent(preset.id)"
            />
          </div>
        </div>
        <div class="field">
          <span class="field-label">{{ t('settings.accentCustom') }}</span>
          <div class="accent-row">
            <input
              v-model="customAccent"
              class="input input-narrow accent-input"
              type="text"
              :placeholder="accentPlaceholder"
              @input="customAccentError = null"
              @keydown.enter.prevent="applyCustomAccent"
            />
            <button class="btn btn-sm" type="button" @click="applyCustomAccent">
              {{ t('settings.accentApply') }}
            </button>
            <button class="btn btn-sm" type="button" @click="ui.resetAccent()">
              {{ t('settings.accentReset') }}
            </button>
          </div>
          <p v-if="customAccentError" class="field-error">{{ customAccentError }}</p>
        </div>
        <p class="field-hint">{{ t('settings.accentHint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.language') }}</h2>
        <div class="segmented">
          <button
            v-for="item in locales"
            :key="item.value"
            type="button"
            :class="{ active: locale === item.value }"
            :aria-pressed="locale === item.value"
            @click="chooseLocale(item.value)"
          >
            {{ item.label }}
          </button>
        </div>
        <p class="field-hint">{{ t('settings.languageHint') }}</p>
      </section>
    </template>
    </div>
  </div>
</template>

<style scoped>
/* Nine panels do not fit in a 944px window, and saving is the only way to
   commit any of them: pin the header so the button cannot scroll out of reach. */
.settings-page {
  /* Header out of the scroller, panels in their own scroller: a sticky bar has
     to be opaque and still let a band of panels show above it, and the one
     control that matters (保存) must never scroll away. */
  gap: 0;
  padding: 0;
  overflow: hidden;
}

.page-header {
  flex: none;
  padding: 22px 32px 14px;
  background: var(--surface-solid);
  border-bottom: 1px solid var(--border-subtle);
}

.page-body {
  display: flex;
  flex-direction: column;
  gap: 20px;
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 24px 32px 40px;
}

.port-split {
  margin-top: 14px;
}

.port-hint {
  margin-top: 6px;
}

.port-now {
  margin-left: 8px;
  color: var(--text-secondary);
}

.hotkey-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-secondary);
}

/* The chip is a button because clicking it starts recording; it has to look
   like the <kbd> it replaced, or the row reads as a different control. */
.hotkey-chip {
  flex: none;
  min-width: 108px;
  padding: 3px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 12px;
  text-align: center;
  cursor: pointer;
}

.hotkey-chip:hover {
  border-color: var(--accent);
}

/* Recording state has to be visible while the eye is on the keyboard. */
.hotkey-recording,
.hotkey-recording:hover {
  border-color: var(--accent);
  color: var(--accent);
  outline: 2px solid var(--accent-soft);
  outline-offset: 1px;
}

.hotkey-actions {
  display: flex;
  gap: 6px;
  margin-left: auto;
}

kbd {
  flex: none;
  padding: 3px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 12px;
}

.panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 18px 20px;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 16px;
}

.divider {
  height: 1px;
  background: var(--border-subtle);
}

.proxy-state {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 13px;
}

.kernel-row {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.kernel-head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

/*
 * A restart that did not happen is a fault, not a status: the line keeps its
 * place under 权限 but takes the danger colour so it cannot be mistaken for
 * 「当前以管理员身份运行」.
 */
.elevate-failed {
  color: var(--danger);
}

/*
 * The service is the second half of 权限: same card, one step below the
 * elevation line, so 「谁在跑内核」 reads top to bottom.
 */
.service-title {
  margin: 0 0 6px;
  font-size: 14px;
  font-weight: 600;
}

.kernel-path {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-secondary);
  /* A full path is long and has no spaces to break at on Windows. */
  overflow-wrap: anywhere;
}

.accent-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

/* A swatch is a colour sample, not a label: the ring marks the chosen one and
   the 26px square keeps it clickable without turning the row into a toolbar. */
.accent-swatch {
  width: 26px;
  height: 26px;
  flex: none;
  padding: 0;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-pill);
  box-shadow: inset 0 1px 0 var(--glass-highlight);
  cursor: pointer;
  transition: transform var(--duration-fast) var(--ease-out);
}

.accent-swatch:hover {
  transform: scale(1.1);
}

/* The live accent draws the ring, so it reads even on a same-hue swatch. */
.accent-swatch.active {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

/* `.input` is full width for forms; a six-digit hex does not need 180px. */
.accent-input {
  width: 128px;
  flex: none;
}
</style>
