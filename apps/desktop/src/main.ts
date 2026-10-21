import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from './App.vue'
import { router } from './app/router'
import { useSettingsStore } from './modules/settings/store'
import './style.css'

const app = createApp(App)
app.use(createPinia())
app.use(router)
// 应用主题偏好（含跟随系统）后再挂载，避免首屏闪烁
void useSettingsStore().init()
app.mount('#app')
