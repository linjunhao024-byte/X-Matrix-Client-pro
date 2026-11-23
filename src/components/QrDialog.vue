<script setup lang="ts">
import QRCode from 'qrcode'
import { onBeforeUnmount, ref, watch } from 'vue'
import AppIcon from './AppIcon.vue'
import ModalDialog from './ModalDialog.vue'
import { t } from '../i18n'

const props = defineProps<{ open: boolean; link: string; name: string }>()
const emit = defineEmits<{ close: [] }>()

/** The rendered `<svg>`, or `null` while it is still being drawn. */
const svg = ref<string | null>(null)
/** Why the code could not be drawn; the link itself is still shown below it. */
const failure = ref<string | null>(null)
const copied = ref(false)
const copyFailed = ref(false)

let copyTimer: ReturnType<typeof setTimeout> | null = null
/**
 * Guards against a slow render landing after the dialog moved on to another
 * node: the reply is only accepted when it is still the newest request.
 */
let generation = 0

watch(
  () => [props.open, props.link] as const,
  ([open, link]) => {
    if (!open) return
    copied.value = false
    copyFailed.value = false
    const token = ++generation
    svg.value = null
    failure.value = null
    if (!link) {
      failure.value = t('nodes.qr.empty')
      return
    }
    // `margin: 1` keeps the quiet zone the scanner needs; the white surface
    // behind it is part of the code, not decoration.
    QRCode.toString(link, { type: 'svg', margin: 1, width: 240 })
      .then((rendered) => {
        if (token !== generation) return
        svg.value = rendered
      })
      .catch((cause: unknown) => {
        if (token !== generation) return
        // A link the encoder cannot hold (too long for the default version)
        // must leave the dialog readable instead of a white, empty square.
        failure.value = cause instanceof Error ? cause.message : String(cause)
        console.warn('qr render failed', cause)
      })
  },
  { immediate: true },
)

async function copy() {
  copyFailed.value = false
  try {
    await navigator.clipboard.writeText(props.link)
    copied.value = true
    if (copyTimer) clearTimeout(copyTimer)
    // The badge reports a moment, not a state: leaving it up would hide the
    // label the user is looking for on the second click.
    copyTimer = setTimeout(() => {
      copied.value = false
    }, 2000)
  } catch (cause) {
    // WebView2 refuses the clipboard in some profiles; say so instead of
    // pretending the link is on its way.
    copyFailed.value = true
    console.warn('clipboard write failed', cause)
  }
}

onBeforeUnmount(() => {
  if (copyTimer) clearTimeout(copyTimer)
})
</script>

<template>
  <ModalDialog
    :open="open"
    :title="name || t('nodes.qr.title')"
    :width="380"
    @close="emit('close')"
  >
    <p class="muted qr-hint">{{ t('nodes.qr.hint') }}</p>

    <!-- A QR is read by a camera, so it stays black on white in every theme:
         no token colours, no inversion, no transparency behind the modules. -->
    <div v-if="svg" class="qr-stage" aria-hidden="true" v-html="svg" />
    <p v-else-if="failure" class="qr-failure">{{ t('nodes.qr.failed', { error: failure }) }}</p>

    <code class="qr-link">{{ link }}</code>

    <p v-if="copyFailed" class="alert">{{ t('nodes.copyFailed') }}</p>

    <template #footer>
      <button class="btn" type="button" @click="emit('close')">{{ t('common.close') }}</button>
      <button class="btn btn-primary" type="button" :disabled="link === ''" @click="copy">
        <AppIcon name="copy" :size="14" />
        {{ copied ? t('common.copied') : t('nodes.copyLink') }}
      </button>
    </template>
  </ModalDialog>
</template>

<style scoped>
.qr-hint {
  margin: 0;
  font-size: 12.5px;
  line-height: 1.6;
}

.qr-stage {
  align-self: center;
  display: grid;
  place-items: center;
  box-sizing: border-box;
  width: 264px;
  height: 264px;
  padding: 12px;
  /* Not a design token on purpose: a tinted or inverted code does not scan. */
  background: white;
  border-radius: var(--radius-control);
}

.qr-stage :deep(svg) {
  display: block;
  width: 100%;
  height: 100%;
}

.qr-failure {
  align-self: center;
  max-width: 280px;
  margin: 0;
  font-size: 12.5px;
  line-height: 1.6;
  text-align: center;
  color: var(--text-tertiary);
}

/* Share links are long and unbreakable; wrapping them beats a horizontal
   scrollbar in a dialog this narrow. */
.qr-link {
  display: block;
  max-height: 96px;
  overflow-y: auto;
  padding: 8px 10px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: 11.5px;
  line-height: 1.6;
  word-break: break-all;
  white-space: pre-wrap;
}
</style>
