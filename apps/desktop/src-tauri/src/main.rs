// 防止 Windows 发布版弹出多余控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    toogl_desktop_lib::run()
}
