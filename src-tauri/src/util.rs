use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::Result;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write a file atomically (write to temp file, then rename) so a crash never
/// leaves a half-written settings/profile file behind.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!(".write-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    let written = file.write_all(contents);
    drop(file);
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    use std::fs::{self, DirBuilder, Permissions};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

    DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    let symlink = fs::symlink_metadata(path)?.is_symlink();
    let flags = libc::O_DIRECTORY | if symlink { 0 } else { libc::O_NOFOLLOW };
    let directory = OpenOptions::new().read(true).custom_flags(flags).open(path)?;
    let metadata = directory.metadata()?;
    let shared = metadata.permissions().mode() & 0o077 != 0;
    // SAFETY: geteuid takes no arguments and returns the process's effective user ID.
    let owned = metadata.uid() == unsafe { libc::geteuid() };
    if !owned || (symlink && shared) {
        let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        return Err(crate::error::Error::msg(format!(
            "App directory {} must point to a directory owned by the current user with mode 0700. Resolved path: {}",
            path.display(),
            target.display()
        )));
    }
    if shared {
        directory.set_permissions(Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn atomic_writes_keep_credentials_private() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let path = dir.join("settings.json");
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);

        for mode in [0o600, 0o644] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            write_atomic(&path, b"replacement").unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"replacement");
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn private_directory_protects_existing_files() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        ensure_private_dir(&dir).unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.join("runtime.yaml");
        fs::write(&path, b"existing").unwrap();
        ensure_private_dir(&dir).unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(fs::read(&path).unwrap(), b"existing");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn private_directory_preserves_symlink_target_permissions() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        ensure_private_dir(&dir).unwrap();
        let target = dir.join("shared");
        let link = dir.join("app-data");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o750)).unwrap();
        symlink(&target, &link).unwrap();

        let outcome = ensure_private_dir(&link);
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        let private_outcome = ensure_private_dir(&link);
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(mode, 0o750);
        let error = outcome.unwrap_err().to_string();
        assert!(error.contains(&target.display().to_string()), "{error}");
        private_outcome.unwrap();
    }
}
