<script setup lang="ts">
import { RouterView } from 'vue-router'
import AppSidebar from '@/app/components/AppSidebar.vue'
</script>

<template>
  <div class="flex h-dvh min-h-0 bg-background text-foreground">
    <AppSidebar />
    <main class="min-h-0 min-w-0 flex-1 overflow-y-auto" aria-label="页面内容">
      <RouterView />
    </main>
  </div>
</template>
