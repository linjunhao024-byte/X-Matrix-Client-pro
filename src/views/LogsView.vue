<script setup lang="ts">
/**
 * The kernel's own words, live.
 *
 * Before this page the only log a user could see was the 200-line tail inside a
 * modal on the home screen, which is the wrong shape for the one job logs have:
 * watching what happens *next* while you change something. So the page holds a
 * cursor (the byte size it last saw) and asks the backend for the delta, which
 * keeps a quiet kernel down to one metadata call per tick.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import { t } from '../i18n'
import { api, describeError, type EngineKind } from '../services/contract'
import { useEngineStore } from '../stores/engine'
import { initQuiet, useQuiet } from '../stores/lite'

const engine = useEngineStore()

/** How often the page asks for new bytes. Slower than the traffic chart on
 *  purpose: a log line is a sentence, not a sample, and polling faster only
 *  re-reads the same empty string. */
const POLL_MS = 1200
/** Keeps a kernel left running overnight from turning the DOM into a novel. */
const MAX_LINES = 3000

const kind = ref<EngineKind>('xray')
const lines = ref<string[]>([])
const cursor = ref(0)
const follow = ref(true)
const filter = ref('')
const error = ref<string | null>(null)
const copied = ref(false)
const cleared = ref(false)

let timer: ReturnType<typeof setInterval> | null = null
let disposed = false

/**
 * Lite mode: a log page in the tray is a log page nobody reads, so the ticker
 * stops. The cursor survives (it is a byte count, not a timer), which is what
 * lets the reveal poll pick up the bytes written while the window was hidden
 * instead of starting over.
 */
const quiet = useQuiet()

function startTimer(): void {
  if (timer) return
  timer = setInterval(() => void poll(), POLL_MS)
}

function stopTimer(): void {
  if (!timer) return
  clearInterval(timer)
  timer = null
}

const visible = computed(() => {
  const needle = filter.value.trim().toLowerCase()
  if (needle === '') return lines.value
  return lines.value.filter((line) => line.toLowerCase().includes(needle))
})

const viewport = ref<HTMLElement | null>(null)

function scrollToEnd(): void {
  const element = viewport.value
  if (!element) return
  element.scrollTop = element.scrollHeight
}

async function poll(): Promise<void> {
  try {
    const delta = await api.engineLogDelta(kind.value, cursor.value)
    if (disposed) return
    error.value = null
    if (delta.reset) {
      // The file shrank: the kernel was restarted with a fresh log, or the
      // user pressed clear. Either way what we hold is from another run.
      lines.value = []
      cursor.value = delta.size ?? 0
      if (delta.text === '') return
    } else {
      cursor.value = delta.size ?? 0
    }
    if (delta.text === '') return
    const incoming = delta.text.split(/\r?\n/)
    // A trailing newline means the last element is not a line yet.
    if (incoming[incoming.length - 1] === '') incoming.pop()
    lines.value = [...lines.value, ...incoming].slice(-MAX_LINES)
    if (follow.value) requestAnimationFrame(scrollToEnd)
  } catch (failure) {
    error.value = describeError(failure)
  }
}

async function switchKind(next: EngineKind): Promise<void> {
  if (next === kind.value) return
  kind.value = next
  lines.value = []
  cursor.value = 0
  await poll()
}

async function clear(): Promise<void> {
  try {
    await api.engineLogClear(kind.value)
    lines.value = []
    cursor.value = 0
    cleared.value = true
    window.setTimeout(() => (cleared.value = false), 2400)
  } catch (failure) {
    error.value = describeError(failure)
  }
}

async function copyAll(): Promise<void> {
  try {
    await navigator.clipboard.writeText(visible.value.join('\n'))
    copied.value = true
    window.setTimeout(() => (copied.value = false), 2000)
  } catch (failure) {
    error.value = describeError(failure)
  }
}

// A user who scrolled up has said "I am reading"; stop yanking them to the end.
function onScroll(): void {
  const element = viewport.value
  if (!element) return
  const atEnd = element.scrollHeight - element.scrollTop - element.clientHeight < 24
  follow.value = atEnd
}

watch(follow, (on) => {
  if (on) scrollToEnd()
})

watch(quiet, (idle) => {
  if (idle) {
    stopTimer()
    return
  }
  // Read once *now*: the first screen after a reveal must show the lines the
  // kernel wrote while the window was in the tray, not an empty delta.
  void poll()
  startTimer()
})

onMounted(() => {
  kind.value = engine.kind
  // The visibility probe decides whether the first poll is worth sending: from
  // the tray there is no one to read the lines, so it waits for the reveal.
  void initQuiet().then(() => {
    if (disposed || quiet.value) return
    void poll()
    startTimer()
  })
})

onBeforeUnmount(() => {
  disposed = true
  stopTimer()
})
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.logs.title') }}</h1>
        <p class="page-subtitle">{{ t('page.logs.subtitle') }}</p>
      </div>
      <div class="row">
        <span class="badge" :class="engine.running ? 'badge-success' : ''">
          {{ engine.running ? t('logs.following') : t('common.stopped') }}
        </span>
      </div>
    </header>

    <div class="row logs-toolbar">
      <div class="segmented">
        <button
          v-for="option in ['xray', 'singBox'] as EngineKind[]"
          :key="option"
          type="button"
          :class="{ active: kind === option }"
          :aria-pressed="kind === option"
          @click="switchKind(option)"
        >
          {{ option === 'xray' ? 'Xray' : 'sing-box' }}
        </button>
      </div>

      <input
        v-model="filter"
        class="input input-search"
        type="search"
        :placeholder="t('logs.filter')"
        :aria-label="t('logs.filter')"
      />

      <span class="spacer" />
      <span class="muted small logs-count">
        {{ t('logs.visible', { shown: visible.length, total: lines.length }) }}
      </span>
      <button
        class="btn btn-sm"
        type="button"
        :class="{ active: follow }"
        :aria-pressed="follow"
        @click="follow = !follow"
      >
        <AppIcon name="arrow-down" :size="14" />
        {{ t('logs.follow') }}
      </button>
      <button class="btn btn-sm" type="button" :disabled="visible.length === 0" @click="copyAll">
        <AppIcon name="copy" :size="14" />
        {{ copied ? t('logs.copied') : t('logs.copy') }}
      </button>
      <button class="btn btn-sm btn-danger" type="button" @click="clear">
        <AppIcon name="trash" :size="14" />
        {{ cleared ? t('logs.cleared') : t('logs.clear') }}
      </button>
    </div>

    <p v-if="error" class="alert">{{ error }}</p>

    <section class="surface-plain log-shell">
      <EmptyState
        v-if="lines.length === 0"
        icon="file"
        :title="t('logs.empty.title')"
        :body="t('logs.empty.body')"
      />
      <EmptyState
        v-else-if="visible.length === 0"
        icon="file"
        :title="t('logs.noMatch.title')"
        :body="t('logs.noMatch.body')"
      />
      <pre
        v-else
        ref="viewport"
        class="log-view"
        role="log"
        aria-live="polite"
        tabindex="0"
        @scroll="onScroll"
      ><span v-for="(line, index) in visible" :key="index" class="log-line">{{ line }}
</span></pre>
    </section>
  </div>
</template>

<style scoped>
.logs-toolbar {
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.input-search {
  width: 240px;
}

.logs-count {
  font-variant-numeric: tabular-nums;
}

.log-shell {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  overflow: hidden;
}

.log-view {
  flex: 1 1 auto;
  margin: 0;
  padding: 14px 16px;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--text-secondary);
  /* Long JSON from a kernel is one line; wrapping beats a horizontal scrollbar
     that hides the half of the line you need. */
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  tab-size: 2;
}

.log-line {
  display: block;
}

.log-view:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
</style>
