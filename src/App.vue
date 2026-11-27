<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import AppIcon from './components/AppIcon.vue'
import AppSidebar from './components/AppSidebar.vue'
import { t } from './i18n'
import { autoCheck } from './services/kernelupdate'
import { initQuiet, disposeQuiet } from './stores/lite'
import { useEngineStore } from './stores/engine'
import { useUiStore } from './stores/ui'

// One poller for the whole app: the status badge, the home cards and the
// connection list all read the same mirror of the Rust runtime. The crash
// subscription is registered here, once, because the shell mounts once — it
// only makes the *next* read happen immediately instead of a second later.
const engine = useEngineStore()
const ui = useUiStore()

onMounted(() => {
  // Lite mode first: `initQuiet` also probes whether the window came up hidden,
  // so the poller is armed only after that answer is known. `startPolling`
  // refuses while `quiet`, and the store's own watch arms it on a reveal.
  void initQuiet().then(() => engine.startPolling())
  void engine.watchExits()
  void checkKernelUpdate()
})

/**
 * One quiet look-up for a newer kernel, at most once a day.
 *
 * The *backend's* status decides which kernel is asked about, not the default
 * in the store: a session that resumes with sing-box running must be told about
 * sing-box. `autoCheck` swallows its own failures — offline and a blocked
 * GitHub both end as "no banner", never as an error the user has to dismiss.
 */
async function checkKernelUpdate(): Promise<void> {
  // The poller's first tick usually answers this already; ask only when it has
  // not landed yet (a window opened straight into the tray skips the poller).
  if (!engine.status) await engine.refreshStatus()
  await autoCheck(engine.kind, { enabled: ui.kernelUpdateCheck })
}

onBeforeUnmount(() => {
  engine.unwatchExits()
  engine.stopPolling()
  disposeQuiet()
})
</script>

<template>
  <div class="app-shell">
    <AppSidebar />
    <main class="app-main">
      <div v-if="ui.lastError" class="error-bar" role="alert">
        <AppIcon name="info" :size="16" />
        <span class="error-title">{{ t('error.title') }}</span>
        <span class="error-text">{{ ui.lastError }}</span>
        <button type="button" class="btn btn-ghost btn-sm" @click="ui.dismissError()">
          {{ t('error.dismiss') }}
        </button>
      </div>
      <RouterView v-slot="{ Component }">
        <Transition name="fade" mode="out-in">
          <component :is="Component" />
        </Transition>
      </RouterView>
    </main>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  gap: 14px;
  height: 100%;
  padding: 14px;
}

.app-main {
  flex: 1;
  min-width: 0;
  height: 100%;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* One line, no stack: the second error is usually the first one's shadow. */
.error-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border-radius: var(--radius-control);
  border: 1px solid var(--danger);
  background: var(--danger-soft);
  color: var(--text-primary);
  font-size: 12.5px;
  flex: none;
}

.error-title {
  font-weight: 600;
  flex: none;
}

.error-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
