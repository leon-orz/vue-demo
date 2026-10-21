<script setup lang="ts">
import NetworkPanel from './components/NetworkPanel.vue'
import { useNetworkDiagnostics } from './useNetworkDiagnostics'

const {
  diagnostic,
  repair,
  diagnosticError,
  repairError,
  diagnosing,
  flushingDns,
  diagnose,
  flushDns,
} = useNetworkDiagnostics()
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold">网络诊断</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        检查固定目标的 DNS 和 TCP 连接，按需刷新 DNS 缓存。
      </p>
    </header>
    <NetworkPanel
      :diagnostic="diagnostic"
      :repair="repair"
      :diagnostic-error="diagnosticError"
      :repair-error="repairError"
      :diagnosing="diagnosing"
      :repairing="flushingDns"
      @diagnose="diagnose"
      @repair="flushDns"
    />
  </div>
</template>
