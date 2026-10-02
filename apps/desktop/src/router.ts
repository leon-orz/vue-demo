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
    // Kimi 页含 ECharts，懒加载以拆出独立 chunk
    {
      path: '/kimi',
      name: 'kimi',
      component: () => import('@/views/Kimi.vue'),
      meta: { title: 'Kimi 统计' },
    },
    {
      path: '/server',
      name: 'server',
      component: () => import('@/views/PlaceholderView.vue'),
      meta: { title: '服务器', milestone: '阶段二交付' },
    },
    {
      path: '/nas',
      name: 'nas',
      component: () => import('@/views/PlaceholderView.vue'),
      meta: { title: 'NAS', milestone: '阶段二交付' },
    },
    {
      path: '/proxy',
      name: 'proxy',
      component: () => import('@/views/PlaceholderView.vue'),
      meta: { title: '魔法工具', milestone: 'S1-3 交付' },
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/Settings.vue'),
      meta: { title: '设置' },
    },
  ],
})
