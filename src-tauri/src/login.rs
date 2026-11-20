//! Native, per-user login registration. Windows deliberately uses a highest-
//! privilege interactive task, not the ordinary unelevated Run registry key.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub enabled: bool,
    pub supported: bool,
    pub needs_repair: bool,
    pub detail: String,
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::path::PathBuf;

    fn path() -> Result<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or_else(|| Error::msg("Cannot find the user configuration directory."))?;
        Ok(config.join("autostart/top.vulpeace.engys.desktop"))
    }

    fn executable() -> Result<PathBuf> {
        // current_exe points inside the temporary AppImage mount; APPIMAGE is stable.
        Ok(std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or(std::env::current_exe()?))
    }

    pub(super) fn desktop_entry(executable: &str) -> Result<String> {
        if executable.contains(['\n', '\r', '\0']) {
            return Err(Error::msg("The executable path contains unsupported characters."));
        }
        // Exec quoting has two layers: desktop string escaping, then Exec escaping.
        let quoted = executable
            .chars()
            .map(|c| match c {
                '\\' => "\\\\\\\\".into(),
                '"' => "\\\\\"".into(),
                '$' => "\\\\$".into(),
                '`' => "\\\\`".into(),
                '%' => "%%".into(),
                _ => c.to_string(),
            })
            .collect::<String>();
        Ok(format!(
            "[Desktop Entry]\nType=Application\nName=engys\nExec=\"{quoted}\" --autostart\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        ))
    }

    pub fn status() -> Result<LoginStatus> {
        let entry = desktop_entry(&executable()?.to_string_lossy())?;
        let contents = match std::fs::read_to_string(path()?) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let enabled = contents.as_ref().is_some_and(|text| {
            !text
                .lines()
                .any(|l| matches!(l.trim(), "Hidden=true" | "X-GNOME-Autostart-enabled=false"))
        });
        Ok(LoginStatus {
            enabled,
            supported: true,
            needs_repair: contents.as_ref().is_some_and(|c| c != &entry),
            detail: "Starts in the background and restores the previous core state.".into(),
        })
    }

    pub fn set(enabled: bool) -> Result<LoginStatus> {
        let path = path()?;
        if enabled {
            crate::util::write_atomic(&path, desktop_entry(&executable()?.to_string_lossy())?.as_bytes())?;
        } else if let Err(e) = std::fs::remove_file(path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(e.into());
            }
        }
        status()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn desktop_paths_are_quoted_and_field_codes_escaped() {
            let text = desktop_entry("/a b/100%/$app`\"\\").unwrap();
            assert!(text.contains("100%%/\\\\$app\\\\`\\\\\"\\\\\\\\\" --autostart"));
            assert!(desktop_entry("/tmp/bad\nExec=other").is_err());
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use base64::Engine;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    fn literal(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    fn powershell(mode: &str, expected_sid: Option<&str>) -> Result<String> {
        let exe = std::env::current_exe()?;
        let script = format!(
            "$Mode = {}; $Executable = {}; $ExpectedSid = {};\n{}",
            literal(mode),
            literal(&exe.to_string_lossy()),
            literal(expected_sid.unwrap_or("")),
            include_str!("../windows/login-task.ps1")
        );
        let bytes = script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        // Use the system binary, not a program found through the user's PATH.
        let system = std::env::var_os("SystemRoot").ok_or_else(|| Error::msg("SystemRoot is missing."))?;
        let out = Command::new(std::path::PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"))
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .creation_flags(0x0800_0000)
            .output()?;
        if !out.status.success() {
            return Err(Error::msg(format!(
                "Windows login task failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .trim_start_matches('\u{feff}')
            .trim()
            .into())
    }

    pub fn status() -> Result<LoginStatus> {
        Ok(serde_json::from_str(&powershell("status", None)?)?)
    }

    pub fn set(enabled: bool) -> Result<LoginStatus> {
        let sid = powershell("sid", None)?;
        if crate::privilege::is_elevated() {
            configure(enabled, &sid)?;
        } else {
            // Only fixed operations and a SID are accepted by the elevated helper.
            crate::privilege::run_elevated(
                &format!(
                    "--configure-login {} {}",
                    if enabled { "enable" } else { "disable" },
                    sid
                ),
                true,
            )?;
        }
        let actual = status()?;
        if actual.enabled != enabled || (enabled && actual.needs_repair) {
            return Err(Error::msg("Windows did not apply the login task change."));
        }
        Ok(actual)
    }

    pub fn configure(enabled: bool, sid: &str) -> Result<()> {
        powershell(if enabled { "enable" } else { "disable" }, Some(sid))?;
        Ok(())
    }

    pub fn cleanup() -> Result<()> {
        // No approval is needed if this installation has no tasks to remove.
        if powershell("has-tasks", None)? != "true" {
            return Ok(());
        }
        if crate::privilege::is_elevated() {
            powershell("cleanup", None)?;
            Ok(())
        } else {
            crate::privilege::run_elevated("--remove-login-tasks", true)
        }
    }
    pub fn run() -> Result<()> {
        powershell("run", None)?;
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;
    pub fn status() -> Result<LoginStatus> {
        Ok(LoginStatus {
            enabled: false,
            supported: false,
            needs_repair: false,
            detail: "Login startup is supported on Windows and Linux.".into(),
        })
    }
    pub fn set(_: bool) -> Result<LoginStatus> {
        Err(Error::msg("Login startup is unsupported on this platform."))
    }
}

#[cfg(windows)]
pub use platform::{cleanup, configure, run};
pub use platform::{set, status};

/// Helpers run before Tauri/instance locking and never start a core or webview.
/// Returns an exit code only when a maintenance operation was requested.
pub fn maintenance(args: &[String]) -> Option<i32> {
    #[cfg(windows)]
    {
        let result = if args.get(1).map(String::as_str) == Some("--configure-login") {
            match (args.get(2).map(String::as_str), args.get(3)) {
                (Some(mode @ ("enable" | "disable")), Some(sid))
                    if sid.starts_with("S-1-") && sid.chars().all(|c| c.is_ascii_digit() || c == 'S' || c == '-') =>
                {
                    configure(mode == "enable", sid)
                }
                _ => Err(Error::msg("Invalid login task operation.")),
            }
        } else if args.get(1).map(String::as_str) == Some("--remove-login-tasks") {
            cleanup()
        } else {
            return None;
        };
        return Some(match result {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        None
    }
}
