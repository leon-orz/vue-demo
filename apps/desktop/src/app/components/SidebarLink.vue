<script setup lang="ts">
import type { Component } from 'vue'
import { RouterLink } from 'vue-router'

withDefaults(
  defineProps<{
    to: string
    label: string
    icon: Component
    active: boolean
    badge?: string
    planned?: boolean
  }>(),
  { badge: undefined, planned: false },
)
</script>

<template>
  <RouterLink
    :to="to"
    class="flex h-10 items-center gap-3 rounded-md border-l-2 px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
    :class="
      active
        ? 'border-primary bg-accent text-accent-foreground'
        : 'border-transparent text-muted-foreground hover:bg-accent/60 hover:text-foreground'
    "
    :aria-current="active ? 'page' : undefined"
  >
    <component :is="icon" class="size-4 shrink-0" aria-hidden="true" />
    <span class="min-w-0 flex-1 truncate" :class="planned ? 'text-muted-foreground/70' : ''">{{
      label
    }}</span>
    <span
      v-if="planned"
      class="rounded-sm border border-dashed px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground"
    >
      规划中
    </span>
    <span
      v-else-if="badge"
      class="rounded-sm bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary"
    >
      {{ badge }}
    </span>
  </RouterLink>
</template>
