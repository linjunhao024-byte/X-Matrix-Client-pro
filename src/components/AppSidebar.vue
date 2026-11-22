<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import AppIcon, { type IconName } from './AppIcon.vue'
import { t } from '../i18n'
import { useUiStore } from '../stores/ui'

const ui = useUiStore()
const route = useRoute()

interface NavItem {
  name: string
  /** Dictionary key, resolved in the template so it follows a language switch. */
  labelKey: string
  icon: IconName
}

const items: NavItem[] = [
  { name: 'home', labelKey: 'nav.home', icon: 'home' },
  { name: 'nodes', labelKey: 'nav.nodes', icon: 'nodes' },
  { name: 'routing', labelKey: 'nav.routing', icon: 'routing' },
  { name: 'groups', labelKey: 'nav.groups', icon: 'layers' },
  { name: 'ruleSets', labelKey: 'nav.ruleSets', icon: 'inbox' },
  { name: 'subscriptions', labelKey: 'nav.subscriptions', icon: 'cloud' },
  { name: 'connections', labelKey: 'nav.connections', icon: 'connections' },
  { name: 'logs', labelKey: 'nav.logs', icon: 'logs' },
  { name: 'settings', labelKey: 'nav.settings', icon: 'settings' },
]

const collapsed = computed(() => ui.sidebarCollapsed)
const isDark = computed(() => ui.resolvedTheme === 'dark')
</script>

<template>
  <aside class="sidebar glass" :class="{ collapsed }">
    <div class="brand">
      <img class="brand-mark" src="/icon.ico" alt="" width="28" height="28" />
      <span v-if="!collapsed" class="brand-name">X-Matrix</span>
    </div>

    <nav class="nav">
      <RouterLink
        v-for="item in items"
        :key="item.name"
        :to="{ name: item.name }"
        class="nav-item"
        :class="{ active: route.name === item.name }"
        :title="collapsed ? t(item.labelKey) : undefined"
      >
        <AppIcon :name="item.icon" :size="18" />
        <span v-if="!collapsed" class="nav-label">{{ t(item.labelKey) }}</span>
      </RouterLink>
    </nav>

    <div class="sidebar-footer">
      <RouterLink
        :to="{ name: 'about' }"
        class="nav-item"
        :class="{ active: route.name === 'about' }"
        :title="collapsed ? t('nav.about') : undefined"
      >
        <AppIcon name="info" :size="18" />
        <span v-if="!collapsed" class="nav-label">{{ t('nav.about') }}</span>
      </RouterLink>
      <button
        type="button"
        class="nav-item as-button"
        :title="isDark ? t('sidebar.toLight') : t('sidebar.toDark')"
        @click="ui.toggleTheme()"
      >
        <AppIcon :name="isDark ? 'sun' : 'moon'" :size="18" />
        <span v-if="!collapsed" class="nav-label">{{ isDark ? t('sidebar.light') : t('sidebar.dark') }}</span>
      </button>
      <button
        type="button"
        class="nav-item as-button"
        :title="collapsed ? t('sidebar.expand') : t('sidebar.collapse')"
        @click="ui.toggleSidebar()"
      >
        <AppIcon name="sidebar" :size="18" />
        <span v-if="!collapsed" class="nav-label">{{ t('sidebar.collapseShort') }}</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  gap: 18px;
  width: var(--sidebar-width);
  padding: 16px 12px;
  flex: none;
  border-radius: var(--radius-card);
  transition: width var(--duration-base) var(--ease-out);
}

.sidebar.collapsed {
  width: var(--sidebar-width-collapsed);
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 2px 6px 0;
  min-height: 32px;
}

.brand-mark {
  border-radius: 8px;
  flex: none;
}

.brand-name {
  font-size: 14px;
  font-weight: 650;
  letter-spacing: -0.01em;
  white-space: nowrap;
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 36px;
  padding: 0 10px;
  border: none;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 550;
  cursor: pointer;
  white-space: nowrap;
  transition:
    background var(--duration-fast) var(--ease-out),
    color var(--duration-fast) var(--ease-out);
}

.nav-item:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--accent-soft);
  color: var(--accent);
}

.as-button {
  width: 100%;
  text-align: left;
}

.sidebar-footer {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-top: 10px;
  border-top: 1px solid var(--border-subtle);
}

.collapsed .nav-item {
  justify-content: center;
  padding: 0;
}
</style>
