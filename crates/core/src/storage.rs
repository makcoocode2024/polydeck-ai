//! Atomic file storage with backup and rollback.
//!
//! Combines MoveFileExW approach with tempfile+fsync and retry fallbacks for robust Windows operation.

use crate::error::{AppError, AppResult};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Atomically replace a file: write to temp -> fsync -> rename -> .bak backup.
pub fn atomic_replace(path: &Path, data: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Create .bak backup of existing file
    if path.exists() {
        let backup = backup_path(path);
        let _ = fs::copy(path, &backup);
    }

    // Write to temp file in same directory (same filesystem for atomic rename)
    let dir = path.parent().unwrap_or(Path::new("."));
    let temp = tempfile::NamedTempFile::new_in(dir)
        .map_err(|e| AppError::Storage(format!("创建临时文件失败：{e}")))?;

    let (mut file, temp_path) = temp.into_parts();
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);

    // Atomic rename with platform specific support
    #[cfg(windows)]
    {
        windows_atomic_rename(&temp_path, path)?;
    }

    #[cfg(not(windows))]
    {
        fs::rename(&temp_path, path)?;
    }

    // Prevent temp_path from trying to delete the file on drop since it was moved
    let _ = temp_path.keep();

    Ok(())
}

/// Alias for atomic_replace.
pub fn atomic_write(path: &Path, data: &[u8]) -> AppResult<()> {
    atomic_replace(path, data)
}

/// Atomically serialize and write data as JSON.
pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| AppError::Storage(format!("JSON 序列化失败：{e}")))?;
    atomic_replace(path, &bytes)
}

/// Read a file, falling back to .bak if the primary is missing or corrupt.
pub fn read_with_fallback(path: &Path) -> AppResult<Vec<u8>> {
    match fs::read(path) {
        Ok(data) => Ok(data),
        Err(_) => {
            let backup = backup_path(path);
            if backup.exists() {
                tracing::warn!("主文件不可读，使用备份：{}", backup.display());
                fs::read(&backup).map_err(|e| {
                    AppError::Storage(format!("主文件和备份均不可读：{e}"))
                })
            } else {
                Err(AppError::Storage(format!(
                    "文件不存在且无备份：{}",
                    path.display()
                )))
            }
        }
    }
}

/// Generate timestamped backup path.
pub fn timestamped_backup(path: &Path) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let name = if ext.is_empty() {
        format!("{stem}.{ts}.bak")
    } else {
        format!("{stem}.{ts}.bak.{ext}")
    };
    path.with_file_name(name)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    PathBuf::from(bak)
}

#[cfg(windows)]
fn windows_atomic_rename(from: &Path, to: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    let wide_from: Vec<u16> = from.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let wide_to: Vec<u16> = to.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(20 * attempt));
        }
        let result = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                wide_from.as_ptr(),
                wide_to.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        };
        if result != 0 {
            return Ok(());
        }
    }

    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if std::fs::copy(from, to).is_ok() {
        let _ = std::fs::remove_file(from);
        return Ok(());
    }

    Err(AppError::Storage(format!(
        "MoveFileExW 失败：{}",
        std::io::Error::last_os_error()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_replace_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");

        // Write initial
        atomic_replace(&path, b"v1").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "v1");

        // Replace - should create .bak
        atomic_replace(&path, b"v2").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2");
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), "v1");
    }

    #[test]
    fn read_with_fallback_uses_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        let bak = backup_path(&path);

        fs::write(&bak, b"backup_data").unwrap();
        let data = read_with_fallback(&path).unwrap();
        assert_eq!(data, b"backup_data");
    }
}