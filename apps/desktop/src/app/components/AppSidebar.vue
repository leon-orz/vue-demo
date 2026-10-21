<script setup lang="ts">
import { LayoutDashboard, Settings } from '@lucide/vue'
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import logoUrl from '@/assets/logo.svg'
import { toolModules } from '@/app/modules'
import SidebarLink from './SidebarLink.vue'

const route = useRoute()
const coreModule = toolModules.find((module) => module.id === 'proxy')
const utilityModules = toolModules.filter((module) => module.id !== 'proxy')
const dashboardActive = computed(() => route.path === '/dashboard')
const settingsActive = computed(() => route.path === '/settings')
const isActive = (path: string) => route.path === path
</script>

<template>
  <aside class="flex h-full w-60 shrink-0 flex-col border-r bg-muted/20" aria-label="应用侧栏">
    <div class="flex h-16 shrink-0 items-center gap-3 border-b px-5">
      <img :src="logoUrl" alt="" class="size-8 rounded-lg" />
      <div class="min-w-0">
        <p class="truncate text-sm font-semibold">图格 Toogl</p>
        <p class="text-xs text-muted-foreground">Windows 工具箱</p>
      </div>
    </div>

    <nav aria-label="主导航" class="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-3 py-5">
      <section class="flex flex-col gap-1">
        <p class="px-3 pb-1 text-[11px] font-semibold text-muted-foreground">工作台</p>
        <SidebarLink
          to="/dashboard"
          label="首页看板"
          :icon="LayoutDashboard"
          :active="dashboardActive"
        />
      </section>

      <section v-if="coreModule" class="flex flex-col gap-1">
        <p class="px-3 pb-1 text-[11px] font-semibold text-muted-foreground">核心工具</p>
        <SidebarLink
          :to="coreModule.route.path"
          :label="coreModule.title"
          :icon="coreModule.icon"
          :active="isActive(coreModule.route.path)"
          badge="核心"
        />
      </section>

      <section class="flex flex-col gap-1">
        <p class="px-3 pb-1 text-[11px] font-semibold text-muted-foreground">系统工具</p>
        <SidebarLink
          v-for="module in utilityModules"
          :key="module.id"
          :to="module.route.path"
          :label="module.title"
          :icon="module.icon"
          :active="isActive(module.route.path)"
        />
      </section>
    </nav>

    <div class="shrink-0 border-t p-3">
      <SidebarLink to="/settings" label="设置" :icon="Settings" :active="settingsActive" />
      <p class="px-3 pt-3 text-[11px] text-muted-foreground">个人 Windows 工具箱</p>
    </div>
  </aside>
</template>
