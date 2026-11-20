use std::ptr::null_mut;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_OK, MB_YESNO, MessageBoxW, SW_SHOWNORMAL,
};
use windows_sys::w;

const DOWNLOAD_URL: &str =
    "https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download";

pub(crate) fn ensure_available(interactive: bool) -> bool {
    if tauri::webview_version().is_ok_and(|version| !version.is_empty()) {
        return true;
    }

    if interactive {
        prompt_for_download();
    }
    false
}

fn prompt_for_download() {
    let choice = unsafe {
        MessageBoxW(
            null_mut(),
            w!(
                "engys requires the Microsoft Edge WebView2 Runtime. Open Microsoft's download page?\n\nAfter installing WebView2, launch engys again."
            ),
            w!("WebView2 required"),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };
    if choice != IDYES {
        return;
    }

    let url: Vec<u16> = DOWNLOAD_URL
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            w!("open"),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        let message: Vec<u16> =
            format!("Couldn't open your browser. Download WebView2 from:\n\n{DOWNLOAD_URL}")
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
        unsafe {
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                w!("WebView2 required"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
}
