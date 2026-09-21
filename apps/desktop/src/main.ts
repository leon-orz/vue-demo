import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from './App.vue'
import { router } from './router'
import { useSettingsStore } from './stores/settings'
import './style.css'

const app = createApp(App)
app.use(createPinia())
app.use(router)
// 应用主题偏好（含跟随系统）后再挂载，避免首屏闪烁
useSettingsStore().init()
app.mount('#app')
