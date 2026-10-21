import { Cpu, HardDrive, Network, WandSparkles } from '@lucide/vue'
import { defineAsyncComponent, type Component } from 'vue'
import type { RouteRecordRaw } from 'vue-router'

/** 编译时模块入口：导航、路由、可选看板卡共用同一登记，不承载模块业务。 */
interface ToolModule {
  id: string
  title: string
  icon: Component
  route: RouteRecordRaw
  dashboard?: Component
}

export const toolModules: readonly ToolModule[] = [
  {
    id: 'proxy',
    title: '魔法工具',
    icon: WandSparkles,
    route: {
      path: '/proxy',
      name: 'proxy',
      component: () => import('@/modules/proxy/Proxy.vue'),
      meta: { title: '魔法工具' },
    },
    dashboard: defineAsyncComponent(() => import('@/modules/proxy/components/OverviewCard.vue')),
  },
  {
    id: 'network-diagnostics',
    title: '网络诊断',
    icon: Network,
    route: {
      path: '/network-diagnostics',
      name: 'network-diagnostics',
      component: () => import('@/modules/network-diagnostics/NetworkDiagnostics.vue'),
      meta: { title: '网络诊断' },
    },
  },
  {
    id: 'system-status',
    title: '系统状态',
    icon: Cpu,
    route: {
      path: '/system-status',
      name: 'system-status',
      component: () => import('@/modules/system-status/SystemStatus.vue'),
      meta: { title: '系统状态' },
    },
  },
  {
    id: 'storage',
    title: '空间维护',
    icon: HardDrive,
    route: {
      path: '/storage',
      name: 'storage',
      component: () => import('@/modules/storage/Storage.vue'),
      meta: { title: '空间维护' },
    },
  },
]

export const dashboardModules = toolModules.filter((module) => module.dashboard)
