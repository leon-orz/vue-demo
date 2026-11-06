/**
 * 渲染层入口:路由三页(看图/解压/压缩)
 */
import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/viewer' },
    { path: '/viewer', component: () => import('./views/ImageViewer.vue') },
    { path: '/extract', component: () => import('./views/ArchiveExtract.vue') },
    { path: '/compress', component: () => import('./views/Compress.vue') }
  ]
})

createApp(App).use(router).mount('#app')
