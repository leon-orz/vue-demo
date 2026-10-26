<script setup lang="ts">
import { dashboardModules } from '@/app/modules'
import ModuleCardBoundary from './components/ModuleCardBoundary.vue'
import PlannedModulesCard from './components/PlannedModulesCard.vue'
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">看板</h1>
      <p class="mt-1 text-sm text-muted-foreground">工具状态摘要，点击进入对应模块。</p>
    </header>
    <section class="grid grid-cols-1 items-start gap-4 lg:grid-cols-2 xl:grid-cols-3">
      <!-- 单卡渲染/异步加载异常只降级该卡，不影响其余模块 -->
      <ModuleCardBoundary v-for="module in dashboardModules" :key="module.id" :title="module.title">
        <component :is="module.dashboard" />
      </ModuleCardBoundary>
      <PlannedModulesCard />
    </section>
  </div>
</template>
