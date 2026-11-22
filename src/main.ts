import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'
import { initLocale } from './i18n'
import { describeError } from './services/contract'
import { useUiStore } from './stores/ui'
import './styles/tokens.css'
import './styles/global.css'

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
// Resolve the theme and the language before the first paint, so there is no
// light/dark flash and no flash of the wrong language.
const ui = useUiStore(pinia)
ui.init()
initLocale()

// The net under every store: anything that escapes a component or a promise
// still gets said out loud once instead of dying in the devtools console.
app.config.errorHandler = (error) => {
  console.error(error)
  ui.reportError(describeError(error))
}
window.addEventListener('unhandledrejection', (event) => {
  console.error(event.reason)
  ui.reportError(describeError(event.reason))
})

app.use(router)
app.mount('#app')
