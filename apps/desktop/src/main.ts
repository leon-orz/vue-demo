import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from './App.vue'
import { router } from './app/router'
import { useSettingsStore } from './modules/settings/store'
import './style.css'

const app = createApp(App)
app.use(createPinia())
app.use(router)
// init 同步应用缓存主题后异步读取数据库；不等待 IPC，避免阻塞首屏挂载。
void useSettingsStore().init()
app.mount('#app')
