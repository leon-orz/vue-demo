import { WandSparkles, Rss, Bot, Gamepad2, Globe, Terminal } from '@lucide/vue'
import { defineAsyncComponent, type Component } from 'vue'
import type { RouteRecordRaw } from 'vue-router'

/** 编译时模块入口：导航、路由、可选看板卡共用同一登记，不承载模块业务。 */
interface ToolModule {
  id: string
  title: string
  icon: Component
  route: RouteRecordRaw
  dashboard?: Component
  /** 规划中模块：路由指向占位页，不提供看板卡。 */
  planned?: string
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
    id: 'feed',
    title: '信息订阅',
    icon: Rss,
    planned: '聚合 RSS 源与 GitHub 动态，统一查看你关心的更新。',
    route: {
      path: '/feed',
      name: 'feed',
      component: () => import('@/modules/planned/PlannedPlaceholder.vue'),
      meta: { title: '信息订阅' },
    },
  },
  {
    id: 'ai-gateway',
    title: 'AI 网关',
    icon: Bot,
    planned: '查看 AI 模型聚合中转平台的用量统计与健康状态，等待平台 API 确定。',
    route: {
      path: '/ai-gateway',
      name: 'ai-gateway',
      component: () => import('@/modules/planned/PlannedPlaceholder.vue'),
      meta: { title: 'AI 网关' },
    },
  },
  {
    id: 'system-tuning',
    title: '系统优化',
    icon: Gamepad2,
    planned: '游戏设置检查与后台占用清理，所有修改先展示范围、逐项确认。',
    route: {
      path: '/system-tuning',
      name: 'system-tuning',
      component: () => import('@/modules/planned/PlannedPlaceholder.vue'),
      meta: { title: '系统优化' },
    },
  },
  {
    id: 'network-tuning',
    title: '网络优化',
    icon: Globe,
    planned: '网络状态检测与修复，修复动作逐项确认后执行。',
    route: {
      path: '/network-tuning',
      name: 'network-tuning',
      component: () => import('@/modules/planned/PlannedPlaceholder.vue'),
      meta: { title: '网络优化' },
    },
  },
  {
    id: 'dev-environment',
    title: '开发环境',
    icon: Terminal,
    planned: '端口冲突检测、残留进程定位与处理等开发环境辅助。',
    route: {
      path: '/dev-environment',
      name: 'dev-environment',
      component: () => import('@/modules/planned/PlannedPlaceholder.vue'),
      meta: { title: '开发环境' },
    },
  },
]

export const dashboardModules = toolModules.filter(
  (module): module is ToolModule & { dashboard: Component } => module.dashboard !== undefined,
)

export const plannedModules = toolModules.filter((module) => module.planned)
