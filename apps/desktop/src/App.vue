<script setup lang="ts">
import { LayoutDashboard, ChartLine, Settings, WandSparkles } from '@lucide/vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'

const route = useRoute()

/** 左侧导航 */
const navItems = [
  { to: '/dashboard', label: 'Dashboard', icon: LayoutDashboard },
  { to: '/usage', label: 'AI 用量', icon: ChartLine },
  { to: '/proxy', label: '魔法工具', icon: WandSparkles },
  { to: '/settings', label: '设置', icon: Settings },
]
</script>

<template>
  <div class="flex min-h-screen bg-background text-foreground">
    <aside class="flex w-56 shrink-0 flex-col border-r">
      <div class="flex h-14 items-center border-b px-5">
        <span class="text-lg font-bold tracking-tight">图格 Toogl</span>
      </div>
      <nav class="flex flex-col gap-1 p-3">
        <RouterLink
          v-for="item in navItems"
          :key="item.to"
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
      </nav>
    </aside>
    <main class="min-w-0 flex-1 overflow-y-auto">
      <RouterView />
    </main>
  </div>
</template>
