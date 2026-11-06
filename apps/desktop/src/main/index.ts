/**
 * 主进程入口:窗口创建、IPC 通道注册
 */
import { app, BrowserWindow, ipcMain, dialog, shell } from 'electron'
import path from 'path'
import {
  isArchiveFile, extractArchive, compressFiles, cancelTask, CompressRequest
} from './archive'
import { listTasks } from './task'
import { isImageFile, listImagesInDir, readImageFile } from './viewer'

/** 单实例锁定:重复启动时聚焦已有窗口 */
if (!app.requestSingleInstanceLock()) {
  app.quit()
}

let mainWindow: BrowserWindow | null = null

function createMainWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1100,
    height: 720,
    minWidth: 800,
    minHeight: 560,
    show: false,
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  })
  mainWindow.on('ready-to-show', () => mainWindow?.show())
  // 开发环境由 electron-vite 的 dev server 提供页面;生产加载产物
  if (process.env.ELECTRON_RENDERER_URL) {
    mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else {
    mainWindow.loadFile(path.join(__dirname, '../renderer/index.html'))
  }
}

app.whenReady().then(() => {
  createMainWindow()
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore()
      mainWindow.focus()
    }
  })
})

app.on('window-all-closed', () => {
  app.quit()
})

// ---------- IPC 通道 ----------

/** 拖拽/打开的文件分流:图片 or 压缩包 */
ipcMain.handle('file:classify', async (_e, paths: string[]) => {
  const images: string[] = []
  const archives: string[] = []
  for (const p of paths) {
    if (isImageFile(p)) images.push(p)
    else if (isArchiveFile(p)) archives.push(p)
  }
  return { images, archives }
})

/** 列出目录内图片 */
ipcMain.handle('viewer:listDir', (_e, dir: string) => listImagesInDir(dir))

/** 读取图片内容(Buffer → 渲染层转 blob) */
ipcMain.handle('viewer:read', (_e, file: string) => readImageFile(file))

/** 解压 */
ipcMain.handle('archive:extract', (_e, archivePath: string, destDir: string, password?: string) =>
  extractArchive(archivePath, destDir, password ?? null))

/** 压缩 */
ipcMain.handle('archive:compress', (_e, req: CompressRequest) => compressFiles(req))

/** 取消任务 */
ipcMain.handle('task:cancel', (_e, taskId: string) => cancelTask(taskId))

/** 任务快照 */
ipcMain.handle('task:list', () => listTasks())

/** 选择图片文件 */
ipcMain.handle('dialog:pickImage', async () => {
  if (!mainWindow) return ''
  const r = await dialog.showOpenDialog(mainWindow, {
    title: '选择图片',
    properties: ['openFile'],
    filters: [{ name: '图片', extensions: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif'] }]
  })
  return r.canceled ? '' : r.filePaths[0]
})

/** 选择压缩包文件 */
ipcMain.handle('dialog:pickArchive', async () => {
  if (!mainWindow) return ''
  const r = await dialog.showOpenDialog(mainWindow, {
    title: '选择压缩包',
    properties: ['openFile'],
    filters: [{ name: '压缩包', extensions: ['zip', '7z', 'rar', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst', 'iso', 'cab'] }]
  })
  return r.canceled ? '' : r.filePaths[0]
})

/** 选择目录(压缩输出位置等) */
ipcMain.handle('dialog:pickDir', async () => {
  if (!mainWindow) return ''
  const r = await dialog.showOpenDialog(mainWindow, {
    title: '选择文件夹',
    properties: ['openDirectory']
  })
  return r.canceled ? '' : r.filePaths[0]
})

/** 在资源管理器中显示 */
ipcMain.handle('shell:showInFolder', (_e, p: string) => shell.showItemInFolder(p))
