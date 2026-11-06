<script setup lang="ts">
/**
 * 应用外壳:侧栏三模块切换 + 全局拖拽接收(分流到看图/解压)
 */
import { ref, onMounted, onUnmounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'

const router = useRouter()
const route = useRoute()

const menus = [
  { path: '/viewer', label: '看图', icon: '🖼' },
  { path: '/extract', label: '解压', icon: '📦' },
  { path: '/compress', label: '压缩', icon: '🗜' }
]

const dragging = ref(false)
let dragDepth = 0

// window 级 dragenter/dragleave 计数,避免子元素抖动
function onDragEnter(e: DragEvent): void {
  e.preventDefault()
  dragDepth++
  dragging.value = true
}
function onDragLeave(e: DragEvent): void {
  e.preventDefault()
  dragDepth--
  if (dragDepth <= 0) dragging.value = false
}
function onDragOver(e: DragEvent): void {
  e.preventDefault()
}
async function onDrop(e: DragEvent): Promise<void> {
  e.preventDefault()
  dragDepth = 0
  dragging.value = false
  const paths = [...(e.dataTransfer?.files ?? [])].map((f) => f.path)
  if (paths.length === 0) return
  const { images, archives } = await window.api.classifyFiles(paths)
  if (archives.length > 0) {
    router.push({ path: '/extract', query: { file: archives[0] } })
  } else if (images.length > 0) {
    router.push({ path: '/viewer', query: { file: images[0] } })
  }
}

onMounted(() => {
  window.addEventListener('dragenter', onDragEnter)
  window.addEventListener('dragleave', onDragLeave)
  window.addEventListener('dragover', onDragOver)
  window.addEventListener('drop', onDrop)
})
onUnmounted(() => {
  window.removeEventListener('dragenter', onDragEnter)
  window.removeEventListener('dragleave', onDragLeave)
  window.removeEventListener('dragover', onDragOver)
  window.removeEventListener('drop', onDrop)
})
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <h1 class="logo">工具箱</h1>
      <nav>
        <router-link
          v-for="m in menus"
          :key="m.path"
          :to="m.path"
          class="menu-item"
          :class="{ active: route.path === m.path }"
        >
          <span class="icon">{{ m.icon }}</span>{{ m.label }}
        </router-link>
      </nav>
    </aside>
    <main class="content">
      <router-view />
    </main>
    <div v-if="dragging" class="drop-overlay">松开以打开文件</div>
  </div>
</template>

<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body, #app { height: 100%; }
body {
  font-family: 'Microsoft YaHei', system-ui, sans-serif;
  background: #1e1f22;
  color: #ddd;
  overflow: hidden;
}
.app-shell { display: flex; height: 100%; }
.sidebar {
  width: 160px;
  background: #17181a;
  padding: 16px 0;
  flex-shrink: 0;
}
.logo { font-size: 16px; padding: 0 20px 16px; color: #7ab8ff; }
.menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 20px;
  color: #aaa;
  text-decoration: none;
  font-size: 14px;
}
.menu-item:hover { background: #232428; color: #fff; }
.menu-item.active { background: #2a2c31; color: #7ab8ff; }
.content { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.drop-overlay {
  position: fixed;
  inset: 0;
  background: rgba(122, 184, 255, 0.12);
  border: 2px dashed #7ab8ff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  pointer-events: none;
  z-index: 999;
}
</style>
