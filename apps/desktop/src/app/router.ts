import { createRouter, createWebHashHistory } from 'vue-router'

import { toolModules } from './modules'

// 桌面 WebView 使用 hash 路由；模块配置由 query 定位。
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/dashboard' },
    {
      path: '/dashboard',
      name: 'dashboard',
      component: () => import('@/modules/dashboard/Dashboard.vue'),
      meta: { title: '看板' },
    },
    ...toolModules.map((module) => module.route),
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/modules/settings/Settings.vue'),
      meta: { title: '设置' },
    },
  ],
})
