import { createRouter, createWebHashHistory } from 'vue-router'

// Tauri 桌面端无服务端路由，用 hash history 避免 WebView 路径问题
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/dashboard' },
    {
      path: '/dashboard',
      name: 'dashboard',
      component: () => import('@/views/Dashboard.vue'),
      meta: { title: 'Dashboard' },
    },
    // AI 用量页含 ECharts，懒加载以拆出独立 chunk；/kimi 为 S1-2 旧路径，重定向保留兼容
    {
      path: '/usage',
      name: 'usage',
      component: () => import('@/views/Usage.vue'),
      meta: { title: 'AI 用量' },
    },
    { path: '/kimi', redirect: '/usage' },
    {
      path: '/proxy',
      name: 'proxy',
      component: () => import('@/views/Proxy.vue'),
      meta: { title: '魔法工具' },
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/Settings.vue'),
      meta: { title: '设置' },
    },
  ],
})
