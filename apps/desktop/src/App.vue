<script setup lang="ts">
import {
  Cloud,
  HardDrive,
  LayoutDashboard,
  ChartLine,
  Server,
  Settings,
  WandSparkles,
} from '@lucide/vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'

import { Badge } from '@/components/ui/badge'

const route = useRoute()

/** 左侧导航；未交付模块置灰并标注里程碑 */
const navItems = [
  { to: '/dashboard', label: 'Dashboard', icon: LayoutDashboard, enabled: true },
  { to: '/kimi', label: 'Kimi 统计', icon: ChartLine, enabled: true },
  { to: '/gateway', label: '我的 API 平台', icon: Cloud, enabled: false, milestone: '阶段二' },
  { to: '/server', label: '服务器', icon: Server, enabled: false, milestone: '阶段二' },
  { to: '/nas', label: 'NAS', icon: HardDrive, enabled: false, milestone: '阶段二' },
  { to: '/proxy', label: '魔法工具', icon: WandSparkles, enabled: false, milestone: 'S1-3' },
  { to: '/settings', label: '设置', icon: Settings, enabled: true },
]
</script>

<template>
  <div class="flex min-h-screen bg-background text-foreground">
    <aside class="flex w-56 shrink-0 flex-col border-r">
      <div class="flex h-14 items-center border-b px-5">
        <span class="text-lg font-bold tracking-tight">图格 Toogl</span>
      </div>
      <nav class="flex flex-col gap-1 p-3">
        <template v-for="item in navItems" :key="item.to">
          <RouterLink
            v-if="item.enabled"
            :to="item.to"
            class="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors"
            :class="
              route.path === item.to
                ? 'bg-accent text-accent-foreground'
                : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'
            "
          >
            <component :is="item.icon" class="size-4" />
            {{ item.label }}
          </RouterLink>
          <span
            v-else
            class="flex cursor-not-allowed items-center gap-3 rounded-md px-3 py-2 text-sm text-muted-foreground/50"
          >
            <component :is="item.icon" class="size-4" />
            {{ item.label }}
            <Badge variant="outline" class="ml-auto text-muted-foreground/60">
              {{ item.milestone }}
            </Badge>
          </span>
        </template>
      </nav>
    </aside>
    <main class="min-w-0 flex-1 overflow-y-auto">
      <RouterView />
    </main>
  </div>
</template>
