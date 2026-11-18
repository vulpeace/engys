//! Desktop hooks intentionally have no Android lifecycle ownership.
use tauri::AppHandle;
pub fn setup(_: &AppHandle) -> crate::error::Result<()> {
    Ok(())
}
pub fn show(_: &AppHandle) {}
pub fn window_event(_: &tauri::Window, _: &tauri::WindowEvent) {}
