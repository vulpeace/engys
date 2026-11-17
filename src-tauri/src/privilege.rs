//! Privileges required for TUN mode.
//!
//! - Linux: the core needs CAP_NET_ADMIN (create the TUN device, routes) and
//!   CAP_NET_RAW (bind sockets to the outbound interface). We grant them once
//!   as file capabilities on the core binary via `pkexec setcap`, so the core
//!   runs as the normal user and the GUI never runs as root. Capabilities are
//!   dropped automatically whenever the binary file is replaced (e.g. updated).
//! - Windows: creating the Wintun adapter requires an elevated process. The
//!   core inherits the GUI's token, so we offer to relaunch the app elevated.
//! - macOS: requires root; not supported yet.

use std::path::Path;

use serde::Serialize;

#[cfg(target_os = "linux")]
use crate::error::Error;
use crate::error::Result;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunPermission {
    pub granted: bool,
    /// "root" | "capabilities" | "administrator" | "unsupported"
    pub method: &'static str,
    /// Whether `grant` can do something on this platform.
    pub can_grant: bool,
    /// Human-readable explanation for the UI.
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
const CAP_NET_ADMIN: u32 = 12;
#[cfg(target_os = "linux")]
const CAP_NET_RAW: u32 = 13;
#[cfg(target_os = "linux")]
pub const LINUX_CAPS: &str = "cap_net_admin,cap_net_raw,cap_net_bind_service+ep";

/// Parse the `security.capability` xattr (struct vfs_cap_data, v1–v3) and
/// return (effective flag, permitted capability bitmask).
#[cfg(target_os = "linux")]
fn parse_vfs_cap(data: &[u8]) -> Option<(bool, u64)> {
    let word = |i: usize| -> Option<u32> {
        data.get(i * 4..i * 4 + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let magic_etc = word(0)?;
    let effective = magic_etc & 0x1 != 0;
    let lo = word(1)? as u64; // permitted[0]
    // v2/v3 carry a second 32-bit word: permitted[1] at index 3.
    let hi = if magic_etc & 0xFF00_0000 >= 0x0200_0000 { word(3).unwrap_or(0) as u64 } else { 0 };
    Some((effective, lo | (hi << 32)))
}

#[cfg(target_os = "linux")]
fn has_net_caps(bin: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let Ok(path) = CString::new(bin.as_os_str().as_bytes()) else { return false };
    let name = CString::new("security.capability").unwrap();
    let mut buf = [0u8; 64];
    // SAFETY: valid NUL-terminated strings and a correctly sized buffer.
    let n = unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    if n <= 0 {
        return false;
    }
    match parse_vfs_cap(&buf[..n as usize]) {
        Some((effective, permitted)) => {
            let need = (1u64 << CAP_NET_ADMIN) | (1u64 << CAP_NET_RAW);
            effective && permitted & need == need
        }
        None => false,
    }
}

#[cfg(target_os = "linux")]
pub fn check(bin: &Path) -> TunPermission {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return TunPermission {
            granted: true,
            method: "root",
            can_grant: false,
            detail: "The app is running as root.".into(),
        };
    }
    let bin = bin.canonicalize().unwrap_or_else(|_| bin.to_path_buf());
    if has_net_caps(&bin) {
        TunPermission {
            granted: true,
            method: "capabilities",
            can_grant: true,
            detail: format!("Network capabilities are granted on {}.", bin.display()),
        }
    } else {
        TunPermission {
            granted: false,
            method: "capabilities",
            can_grant: true,
            detail: format!(
                "TUN needs network capabilities on the core binary ({}). Granting asks for your password once; \
                 it must be repeated after the binary is updated.",
                bin.display()
            ),
        }
    }
}

/// Grant capabilities with a polkit password prompt.
#[cfg(target_os = "linux")]
pub fn grant(bin: &Path) -> Result<()> {
    let bin = bin.canonicalize()?;
    let pkexec = which::which("pkexec").map_err(|_| {
        Error::msg(format!(
            "pkexec (polkit) is not available. Run this in a terminal instead:\n  sudo setcap {LINUX_CAPS} {}",
            bin.display()
        ))
    })?;
    let setcap = which::which("setcap")
        .map_err(|_| Error::msg("`setcap` not found. Install libcap (e.g. the `libcap` package)."))?;
    let out = std::process::Command::new(pkexec).arg(setcap).arg(LINUX_CAPS).arg(&bin).output()?;
    match out.status.code() {
        Some(0) => {}
        // pkexec: 126 = dialog dismissed, 127 = not authorized.
        Some(126) | Some(127) => return Err(Error::msg("Authorization was cancelled or denied.")),
        _ => {
            return Err(Error::msg(format!(
                "setcap failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    if !has_net_caps(&bin) {
        return Err(Error::msg(format!(
            "Capabilities were set but are not visible on {}. The filesystem may be mounted with `nosuid`; \
             move the core binary elsewhere (e.g. /usr/local/bin).",
            bin.display()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: standard token query on our own process; handle is closed.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(windows)]
pub fn check(_bin: &Path) -> TunPermission {
    if is_elevated() {
        TunPermission {
            granted: true,
            method: "administrator",
            can_grant: false,
            detail: "The app is running as administrator.".into(),
        }
    } else {
        TunPermission {
            granted: false,
            method: "administrator",
            can_grant: true,
            detail: "TUN needs administrator rights. The app will restart elevated (the core is stopped first).".into(),
        }
    }
}

/// Relaunch this executable elevated (UAC prompt). The caller exits the
/// current instance on success.
#[cfg(windows)]
pub fn grant(_bin: &Path) -> Result<()> {
    run_elevated("--elevated-handoff", false)
}

/// Start only this executable elevated. Maintenance callers wait for completion;
/// handoffs return immediately so the current process can release its lock.
#[cfg(windows)]
pub fn run_elevated(arguments: &str, wait: bool) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe = std::env::current_exe()?;
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let verb = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    let args = wide(arguments.as_ref());
    // SAFETY: valid UTF-16 buffers, initialized struct, and owned process handle.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = args.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(crate::error::Error::msg("Administrator approval was cancelled or failed."));
        }
        if info.hProcess.is_null() {
            return Err(crate::error::Error::msg("Windows did not return the elevated process handle."));
        }
        let mut code = 0;
        let result = if wait {
            WaitForSingleObject(info.hProcess, INFINITE);
            if GetExitCodeProcess(info.hProcess, &mut code) == 0 || code != 0 {
                Err(crate::error::Error::msg("Login task setup failed. Use an administrator account with the same Windows identity; registering with another account is not supported."))
            } else { Ok(()) }
        } else { Ok(()) };
        CloseHandle(info.hProcess);
        result
    }
}

// ---------------------------------------------------------------------------
// Other platforms
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "linux", windows)))]
pub fn check(_bin: &Path) -> TunPermission {
    TunPermission {
        granted: false,
        method: "unsupported",
        can_grant: false,
        detail: "TUN mode is not supported on this platform yet.".into(),
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn grant(_bin: &Path) -> Result<()> {
    Err(crate::error::Error::msg("TUN mode is not supported on this platform yet."))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn parses_v2_caps() {
        // magic v2 | effective, permitted lo = NET_ADMIN|NET_RAW|NET_BIND_SERVICE(10)
        let permitted: u32 = (1 << 12) | (1 << 13) | (1 << 10);
        let mut data = Vec::new();
        data.extend_from_slice(&(0x0200_0000u32 | 1).to_le_bytes());
        data.extend_from_slice(&permitted.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let (eff, perm) = parse_vfs_cap(&data).unwrap();
        assert!(eff);
        assert_eq!(perm, permitted as u64);
    }
}
