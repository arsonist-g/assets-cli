// 发布版按 Windows GUI 子系统链接：否则从快捷方式双击时，会先跟着弹出一个黑色控制台窗口。
// 调试版仍保留控制台，方便看 println! 与 panic 回溯。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod app;

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
