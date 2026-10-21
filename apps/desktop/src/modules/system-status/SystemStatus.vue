<script setup lang="ts">
import { onMounted } from 'vue'

import SystemInformationPanel from './components/SystemInformationPanel.vue'
import { useSystemInformation } from './useSystemInformation'

const { information, error, loading, load } = useSystemInformation()
onMounted(() => void load())
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold">系统状态</h1>
      <p class="mt-1 text-sm text-muted-foreground">查看 Windows 与设备基础信息。</p>
    </header>
    <SystemInformationPanel
      :information="information"
      :error="error"
      :loading="loading"
      @refresh="load"
    />
  </div>
</template>
