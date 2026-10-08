// 防止 Windows 发布版弹出多余控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebView2 缺失时强行启动只会在建窗阶段失败且无提示（发布版无控制台），
    // 启动前先探测运行时，缺失则弹原生对话框引导安装并直接退出
    #[cfg(windows)]
    if let Err(e) = tauri::webview_version() {
        eprintln!("WebView2 运行时不可用：{e}");
        prompt_install_webview2();
        std::process::exit(1);
    }
    toogl_desktop_lib::run()
}

/// 原生警告框（不依赖 WebView，运行时缺失也能显示），用户确认后打开官方下载页
#[cfg(windows)]
fn prompt_install_webview2() {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONWARNING, MB_YESNO,
    };

    const DOWNLOAD_URL: &str = "https://developer.microsoft.com/zh-cn/microsoft-edge/webview2/";
    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let text = wide(
        "未检测到 Microsoft Edge WebView2 运行时，Toogl 无法启动。\n\n请安装 WebView2 后重新打开。是否现在打开官方下载页面？",
    );
    let caption = wide("Toogl - 缺少 WebView2 运行时");
    let choice = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_YESNO | MB_ICONWARNING,
        )
    };
    if choice == IDYES {
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", DOWNLOAD_URL])
            .spawn();
    }
}
