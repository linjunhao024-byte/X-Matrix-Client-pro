<script setup lang="ts">
/**
 * Generated config viewer.
 *
 * A proxy client that writes JSON for a kernel should be able to show that
 * JSON. It is rendered from the current settings without spawning anything,
 * so it also answers "what would happen if I started this?" before starting.
 */
import { ref, watch } from 'vue'

import ModalDialog from './ModalDialog.vue'
import AppIcon from './AppIcon.vue'
import { t } from '../i18n'
import { api, describeError, type EngineKind } from '../services/contract'

const props = defineProps<{ open: boolean; kind: EngineKind; nodeId: string }>()
const emit = defineEmits<{ close: [] }>()

const text = ref('')
const loading = ref(false)
const error = ref<string | null>(null)

async function load() {
  if (!props.nodeId) {
    error.value = t('config.err.noNode')
    return
  }
  loading.value = true
  error.value = null
  try {
    text.value = await api.engineConfigPreview(props.kind, props.nodeId)
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
  <ModalDialog :open="props.open" :title="t('config.title')" :width="760" @close="emit('close')">
    <div class="row">
      <span class="muted small mono">{{ props.kind === 'xray' ? 'configs/xray.json' : 'configs/sing-box.json' }}</span>
      <span class="spacer" />
      <button class="btn btn-sm" type="button" :disabled="loading" @click="load">
        <AppIcon name="refresh" :size="14" />
        {{ loading ? t('common.rendering') : t('common.render') }}
      </button>
    </div>

    <p v-if="error" class="alert">{{ error }}</p>
    <pre v-if="text" class="config">{{ text }}</pre>
  </ModalDialog>
</template>

<style scoped>
.config {
  margin: 0;
  max-height: 56vh;
  overflow: auto;
  padding: 12px 14px;
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
  border: 1px solid var(--border-subtle);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary);
}
</style>
