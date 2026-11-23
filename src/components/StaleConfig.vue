<script setup lang="ts">
import { ref, watch } from 'vue'

import { t } from '../i18n'
import { describeError } from '../services/contract'
import { useEngineStore } from '../stores/engine'
import { useSettingsStore } from '../stores/settings'

/**
 * "Saved, but not live yet" notice for the config pages.
 *
 * Rules, groups and rule sets are only read by a kernel when it renders its
 * config at startup, so a running kernel keeps serving the previous table after
 * a save. The notice makes that gap explicit and offers the one action that
 * closes it: the same-port restart node switching already uses.
 *
 * The parent owns `show`; the component owns the restart and its own busy/error
 * state. It emits `applied` once the kernel was swapped (the parent clears the
 * notice — there is no separate "已生效" line, the engine badge and this
 * disappearing box are the confirmation) and `failed` with the readable message
 * so a parent that wants it can also surface it elsewhere.
 */
const props = defineProps<{ show: boolean }>()

const emit = defineEmits<{
  applied: []
  failed: [message: string]
}>()

const engine = useEngineStore()
const settings = useSettingsStore()

const busy = ref(false)
const error = ref<string | null>(null)

/**
 * Any restart re-renders the config, so a new start timestamp means the saved
 * changes are live regardless of what started the kernel — this box, the tray,
 * or a crash recovery. Watching the timestamp rather than the status object
 * matters: the poller replaces that object every second, so an object watcher
 * would fire on every tick.
 */
watch(
  () => engine.status?.started_at_ms,
  (next, previous) => {
    if (next === undefined || previous === undefined || next === previous) return
    if (props.show) emit('applied')
  },
)

async function apply(): Promise<void> {
  if (busy.value) return
  busy.value = true
  error.value = null
  try {
    // The restart is started with the node the kernel is already running, and
    // that id lives in the settings mirror. The home page loads it, but a
    // window opened straight on a config page may not have — and `start`
    // writes the id on the backend side, so the mirror can sit a session
    // behind. One read covers both cases.
    await settings.load(true)
    const swapped = await engine.reapply()
    if (swapped) {
      emit('applied')
      return
    }
    // Nothing was restarted. A kernel that stopped in the meantime leaves
    // nothing stale, so the notice goes away; if it is still up, the settings
    // read above is the only thing that can have failed, and it says why.
    if (!engine.running) emit('applied')
    else error.value = settings.error ?? engine.error
  } catch (cause) {
    error.value = describeError(cause)
    emit('failed', error.value)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <!-- A kernel that stops after the save leaves nothing stale, so the box goes
       away with it; `busy` and `error` keep it on screen while a restart is in
       flight or has a reason to show. -->
  <div v-if="show && (engine.running || busy || error)" class="stale surface-plain" role="status">
    <p class="stale-text">{{ t('engine.stale') }}</p>
    <button class="btn btn-primary btn-sm" type="button" :disabled="busy" @click="apply">
      {{ busy ? t('engine.staleBusy') : t('engine.staleApply') }}
    </button>
    <!-- The reason a failed restart failed is a sentence, not a colour: the
         message is read, so it stays in body text instead of a red block. -->
    <p v-if="error" class="stale-error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.stale {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 14px;
  padding: 12px 14px;
}

.stale-text {
  flex: 1 1 260px;
  margin: 0;
  font-size: 13px;
  color: var(--text-primary);
}

.stale-error {
  flex: 1 1 100%;
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
</style>
