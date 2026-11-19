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
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    let metadata = fs::metadata(path)?;
    if metadata.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, Permissions::from_mode(0o700))?;
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
}
