<script setup lang="ts">
/**
 * Kernel log viewer.
 *
 * Reads the tail of the kernel's own stdout/stderr file — the same text a user
 * would find in the app data folder, but without leaving the window. Useful
 * exactly when a kernel refuses to start and the reason is one line in a log.
 */
import { ref, watch } from 'vue'

import ModalDialog from './ModalDialog.vue'
import AppIcon from './AppIcon.vue'
import { t } from '../i18n'
import { api, describeError, type EngineKind } from '../services/contract'

const props = defineProps<{ open: boolean; kind: EngineKind }>()
const emit = defineEmits<{ close: [] }>()

const text = ref('')
const loading = ref(false)
const error = ref<string | null>(null)
const openError = ref<string | null>(null)

/**
 * Hand the whole file to the OS: the dialog shows the last 200 lines, but a
 * kernel that refuses to start often explains itself hundreds of lines
 * earlier, and "open the folder" is the one thing a log viewer must not
 * withhold.
 */
async function openLogDir(): Promise<void> {
  openError.value = null
  try {
    await api.appOpenLogDir()
  } catch (cause) {
    openError.value = describeError(cause)
  }
}

async function load() {
  loading.value = true
  error.value = null
  try {
    text.value = await api.engineLogTail(props.kind, 200)
  } catch (cause) {
    error.value = describeError(cause)
  } finally {
    loading.value = false
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void load()
  },
)
</script>

<template>
  <ModalDialog :open="props.open" :title="t('log.title')" :width="720" @close="emit('close')">
    <div class="row">
      <span class="muted small mono">{{ props.kind === 'xray' ? 'xray.log' : 'sing-box.log' }}</span>
      <span class="spacer" />
      <button class="btn btn-sm" type="button" @click="openLogDir">
        {{ t('log.openDir') }}
      </button>
      <button class="btn btn-sm" type="button" :disabled="loading" @click="load">
        <AppIcon name="refresh" :size="14" />
        {{ loading ? t('common.loading') : t('common.refresh') }}
      </button>
    </div>

    <p v-if="openError" class="alert">{{ openError }}</p>
    <p v-if="error" class="alert">{{ error }}</p>

    <pre v-if="text" class="log">{{ text }}</pre>
    <div v-else-if="!loading" class="empty-state">
      <div class="empty-state-title">{{ t('log.empty.title') }}</div>
      <div class="empty-state-body">
        {{ t('log.empty.body') }}
      </div>
    </div>
  </ModalDialog>
</template>

<style scoped>
.log {
  margin: 0;
  max-height: 52vh;
  overflow: auto;
  padding: 12px 14px;
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
  border: 1px solid var(--border-subtle);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--text-secondary);
}
</style>
