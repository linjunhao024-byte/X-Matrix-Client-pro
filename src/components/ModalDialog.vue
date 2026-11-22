<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { t } from '../i18n'

const props = defineProps<{
  open: boolean
  title: string
  width?: number
  /**
   * Whether a click on the dimmed backdrop closes the dialog.
   *
   * True suits confirmations and read-only dialogs. Forms holding typed content
   * pass false: a stray click beside the panel used to throw away a half-filled
   * form with no undo. Escape and the × still close, because those are
   * deliberate.
   */
  closeOnOverlay?: boolean
}>()
const emit = defineEmits<{ close: [] }>()

const panel = ref<HTMLElement | null>(null)
/** Where focus was before the dialog opened, so it can be handed back. */
let restoreFocusTo: HTMLElement | null = null

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

function focusables(): HTMLElement[] {
  const root = panel.value
  if (!root) return []
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (element) => element.offsetParent !== null,
  )
}

function onBackdropClick() {
  if (props.closeOnOverlay === false) return
  emit('close')
}

function onKeydown(event: KeyboardEvent) {
  if (!props.open) return
  if (event.key === 'Escape') {
    emit('close')
    return
  }
  if (event.key !== 'Tab') return
  // Tab must cycle inside the panel: without this it walks out into the page
  // behind the scrim, where nothing is clickable but everything is focusable.
  const items = focusables()
  if (items.length === 0) {
    event.preventDefault()
    return
  }
  const first = items[0]
  const last = items[items.length - 1]
  const active = document.activeElement as HTMLElement | null
  if (event.shiftKey && (active === first || active === null || !panel.value?.contains(active))) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && (active === last || active === null || !panel.value?.contains(active))) {
    event.preventDefault()
    first.focus()
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      restoreFocusTo = document.activeElement as HTMLElement | null
      // Focus the first control so the dialog is usable without a mouse.
      requestAnimationFrame(() => {
        const preferred = panel.value?.querySelector<HTMLElement>(
          'input:not([disabled]), select:not([disabled]), textarea:not([disabled])',
        )
        ;(preferred ?? focusables()[0])?.focus()
      })
      return
    }
    // Handing focus back matters most for confirm dialogs: the row's button is
    // where the eye and the keyboard both expect to land again.
    const target = restoreFocusTo
    restoreFocusTo = null
    requestAnimationFrame(() => {
      if (target && document.contains(target)) target.focus()
    })
  },
)

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="open" class="modal-backdrop" @click.self="onBackdropClick">
        <div
          ref="panel"
          class="modal-panel"
          role="dialog"
          aria-modal="true"
          :aria-label="title"
          :style="{ width: `${width ?? 520}px` }"
        >
          <header class="modal-header">
            <h2 class="modal-title">{{ title }}</h2>
            <button class="btn btn-ghost btn-sm" data-close type="button" @click="emit('close')">
              {{ t('common.close') }}
            </button>
          </header>
          <div class="modal-body">
            <slot />
          </div>
          <footer v-if="$slots.footer" class="modal-footer">
            <slot name="footer" />
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: grid;
  place-items: center;
  padding: 24px;
  background: var(--overlay-scrim);
  backdrop-filter: blur(6px);
}

.modal-panel {
  max-width: 100%;
  max-height: calc(100vh - 48px);
  display: flex;
  flex-direction: column;
  background: var(--surface-glass-strong);
  backdrop-filter: blur(var(--blur-glass)) saturate(180%);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-card);
  box-shadow: var(--shadow-pop);
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 18px 12px;
  border-bottom: 1px solid var(--border-subtle);
}

.modal-title {
  font-size: 15px;
  font-weight: 620;
}

.modal-body {
  /* Slot content is a bare stack of .field blocks: without an explicit gap the
     next field's label sits flush against the previous input and reads as if it
     belongs to it. */
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px;
  overflow-y: auto;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 18px 16px;
  border-top: 1px solid var(--border-subtle);
}

.modal-enter-active,
.modal-leave-active {
  transition: opacity var(--duration-base) var(--ease-out);
}

.modal-enter-active .modal-panel,
.modal-leave-active .modal-panel {
  transition:
    transform var(--duration-base) var(--ease-out),
    opacity var(--duration-base) var(--ease-out);
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-from .modal-panel,
.modal-leave-to .modal-panel {
  transform: translateY(8px) scale(0.985);
  opacity: 0;
}
</style>
