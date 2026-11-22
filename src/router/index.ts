import { watch } from 'vue'
import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router'

import { locale, t } from '../i18n'

/**
 * Hash history on purpose: the packaged app is served from the `tauri://`
 * protocol, where path-based history has no server to fall back on.
 *
 * `meta.title` / `meta.subtitle` hold dictionary keys, not sentences: the
 * window title is resolved through `t()` so a language switch re-titles it.
 */
const routes: RouteRecordRaw[] = [
  {
    path: '/',
    name: 'home',
    component: () => import('../views/HomeView.vue'),
    meta: { title: 'page.home.title', subtitle: 'page.home.subtitle' },
  },
  {
    path: '/nodes',
    name: 'nodes',
    component: () => import('../views/NodesView.vue'),
    meta: { title: 'page.nodes.title', subtitle: 'page.nodes.subtitle' },
  },
  {
    path: '/routing',
    name: 'routing',
    component: () => import('../views/RoutingView.vue'),
    meta: { title: 'page.routing.title', subtitle: 'page.routing.subtitle' },
  },
  {
    path: '/groups',
    name: 'groups',
    component: () => import('../views/GroupsView.vue'),
    meta: { title: 'page.groups.title', subtitle: 'page.groups.subtitle' },
  },
  {
    path: '/rule-sets',
    name: 'ruleSets',
    component: () => import('../views/RuleSetsView.vue'),
    meta: { title: 'page.ruleSets.title', subtitle: 'page.ruleSets.subtitle' },
  },
  {
    path: '/subscriptions',
    name: 'subscriptions',
    component: () => import('../views/SubscriptionsView.vue'),
    meta: { title: 'page.subscriptions.title', subtitle: 'page.subscriptions.subtitle' },
  },
  {
    path: '/connections',
    name: 'connections',
    component: () => import('../views/ConnectionsView.vue'),
    meta: { title: 'page.connections.title', subtitle: 'page.connections.subtitle' },
  },
  {
    path: '/logs',
    name: 'logs',
    component: () => import('../views/LogsView.vue'),
    meta: { title: 'page.logs.title', subtitle: 'page.logs.subtitle' },
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('../views/SettingsView.vue'),
    meta: { title: 'page.settings.title', subtitle: 'page.settings.subtitle' },
  },
  {
    path: '/about',
    name: 'about',
    component: () => import('../views/AboutView.vue'),
    meta: { title: 'page.about.title', subtitle: 'page.about.subtitle' },
  },
  { path: '/:pathMatch(.*)*', redirect: '/' },
]

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

function applyDocumentTitle(metaTitle: unknown): void {
  const title = typeof metaTitle === 'string' ? t(metaTitle) : ''
  document.title = title ? `${title} · X-Matrix` : 'X-Matrix'
}

router.afterEach((to) => applyDocumentTitle(to.meta.title))

// Re-title the window when the language changes, not only when the route does.
watch(locale, () => applyDocumentTitle(router.currentRoute.value.meta.title))
