# vue-demo 工具箱

个人使用的 PC 工具软件(Electron + Vue 3),当前包含三个模块:

- **看图**:常规格式浏览,缩放/平移/旋转/文件夹翻页
- **解压**:基于内嵌 7z.exe,支持 zip/7z/rar/tar 等格式,带进度与密码支持
- **压缩**:输出 7z/zip/tar,支持密码与文件名加密(7z)

## 环境要求

- Node.js >= 24
- pnpm(建议通过 corepack 启用,本仓库固定 pnpm@12.4.1)

## 开发

```powershell
pnpm install
pnpm dev    # 启动 electron-vite 开发模式
```

## 构建

```powershell
pnpm build  # 编译主进程/preload/渲染层产物
```

## 结构

```
apps/desktop/
├── src/main/       # Electron 主进程(archive / task / viewer / IPC)
├── src/preload/    # contextBridge API
└── src/renderer/   # Vue 3 三页 UI
resources/bin/      # 内嵌 7z.exe(以独立 exe 分发,7-Zip LGPL)
```

## 依赖说明

- 7-Zip 以未经修改的独立 exe 形式内嵌(LGPL 允许),应用代码与其分离。
