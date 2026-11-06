/**
 * window.api 的类型声明(preload contextBridge 暴露的 API)
 */
export {}

declare global {
  interface Window {
    api: {
      getFilePath: (file: File) => string
      classifyFiles: (paths: string[]) => Promise<{ images: string[]; archives: string[] }>
      listImagesInDir: (dir: string) => Promise<string[]>
      readImage: (file: string) => Promise<Buffer>
      extract: (archivePath: string, destDir: string, password?: string) => Promise<string>
      compress: (req: {
        targetPath: string
        sources: string[]
        format: '7z' | 'zip' | 'tar.gz'
        level?: number
        password?: string
        encryptNames?: boolean
      }) => Promise<string>
      cancelTask: (taskId: string) => Promise<void>
      listTasks: () => Promise<unknown[]>
      pickImage: () => Promise<string>
      pickArchive: () => Promise<string>
      pickDir: () => Promise<string>
      showInFolder: (p: string) => Promise<void>
      onTaskCreated: (cb: (t: unknown) => void) => () => void
      onTaskProgress: (cb: (p: { taskId: string; percent: number }) => void) => () => void
      onTaskFinished: (cb: (t: unknown) => void) => () => void
    }
  }
}
