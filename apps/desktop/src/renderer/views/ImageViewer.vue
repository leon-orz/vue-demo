<script setup lang="ts">
/**
 * 看图页:文件夹浏览、缩放/平移/旋转/翻页、适应窗口
 */
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRoute } from 'vue-router'

const route = useRoute()

const currentFile = ref('')
const dirFiles = ref<string[]>([])
const scale = ref(1)
const offsetX = ref(0)
const offsetY = ref(0)
const rotation = ref(0)
const fitMode = ref(true)

const index = computed(() => dirFiles.value.findIndex((f) => f === currentFile.value))
const fileName = computed(() => currentFile.value.split(/[\\/]/).pop() ?? '')

async function openFile(file: string): Promise<void> {
  currentFile.value = file
  resetView()
  // 加载同目录列表用于翻页
  const dir = file.replace(/[\\/][^\\/]+$/, '')
  dirFiles.value = await window.api.listImagesInDir(dir)
}

function resetView(): void {
  scale.value = 1
  offsetX.value = 0
  offsetY.value = 0
  rotation.value = 0
  fitMode.value = true
}

function step(delta: number): void {
  if (dirFiles.value.length === 0) return
  let next = (index.value + delta + dirFiles.value.length) % dirFiles.value.length
  currentFile.value = dirFiles.value[next]
  resetView()
}

function zoomAt(deltaY: number): void {
  fitMode.value = false
  const factor = deltaY < 0 ? 1.15 : 1 / 1.15
  scale.value = Math.min(20, Math.max(0.05, scale.value * factor))
}

function rotate(): void {
  rotation.value = (rotation.value + 90) % 360
}

// 拖拽平移
let dragging = false
let lastX = 0
let lastY = 0
function onMouseDown(e: MouseEvent): void {
  dragging = true
  lastX = e.clientX
  lastY = e.clientY
}
function onMouseMove(e: MouseEvent): void {
  if (!dragging) return
  fitMode.value = false
  offsetX.value += e.clientX - lastX
  offsetY.value += e.clientY - lastY
  lastX = e.clientX
  lastY = e.clientY
}
function onMouseUp(): void {
  dragging = false
}

function onKey(e: KeyboardEvent): void {
  if (e.key === 'ArrowLeft') step(-1)
  else if (e.key === 'ArrowRight') step(1)
  else if (e.key === 'r' || e.key === 'R') rotate()
}

// 拖入图片直接打开
function onViewerDrop(e: DragEvent): void {
  e.preventDefault()
  const file = e.dataTransfer?.files[0]
  if (file?.path) openFile(file.path)
}

watch(() => route.query.file, (f) => {
  if (typeof f === 'string' && f) openFile(f)
}, { immediate: true })

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <div
    class="viewer"
    @wheel.prevent="zoomAt($event.deltaY)"
    @mousedown="onMouseDown"
    @mousemove="onMouseMove"
    @mouseup="onMouseUp"
    @dragover.prevent
    @drop="onViewerDrop"
  >
    <div v-if="!currentFile" class="empty">拖入图片,或直接把图片拖到窗口任意位置</div>
    <img
      v-else
      :src="'file:///' + currentFile.replace(/\\/g, '/')"
      :style="{
        transform: `translate(${offsetX}px, ${offsetY}px) rotate(${rotation}deg) scale(${scale})`
      }"
      draggable="false"
    />
    <div v-if="currentFile" class="statusbar">
      <span>{{ fileName }}</span>
      <span>{{ index + 1 }} / {{ dirFiles.length }}</span>
      <span>{{ Math.round(scale * 100) }}%</span>
      <span class="hint">← → 翻页 · 滚轮缩放 · 拖动平移 · R 旋转</span>
    </div>
  </div>
</template>

<style scoped>
.viewer {
  flex: 1;
  position: relative;
  overflow: hidden;
  background: #121316;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
}
.viewer:active { cursor: grabbing; }
.viewer img {
  max-width: 100%;
  max-height: 100%;
  user-select: none;
  transition: none;
}
.empty { color: #666; font-size: 15px; }
.statusbar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  display: flex;
  gap: 24px;
  padding: 6px 16px;
  background: rgba(23, 24, 26, 0.85);
  font-size: 12px;
  color: #999;
}
.hint { margin-left: auto; }
</style>
