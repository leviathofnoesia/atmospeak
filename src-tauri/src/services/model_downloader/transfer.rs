use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) fn write_verified_stream<R, F>(
    mut reader: R,
    destination: &Path,
    expected_sha256: &str,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<()>
where
    R: Read,
    F: FnMut(u64),
{
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("model destination has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let temp_path = parent.join(format!(
        ".{}.download-{}.tmp",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model"),
        Uuid::new_v4()
    ));
    let result = (|| {
        let mut temp = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .context("failed to create temporary model file")?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut downloaded = 0_u64;

        loop {
            if cancel.load(Ordering::Relaxed) {
                bail!("model download cancelled");
            }
            let read = reader
                .read(&mut buffer)
                .context("failed while reading the model download")?;
            if read == 0 {
                break;
            }
            temp.write_all(&buffer[..read])
                .context("failed while writing the model download")?;
            hasher.update(&buffer[..read]);
            downloaded += read as u64;
            on_progress(downloaded);
        }

        temp.flush()?;
        temp.sync_all()?;
        drop(temp);
        if cancel.load(Ordering::Relaxed) {
            bail!("model download cancelled");
        }

        let actual_sha256 = format!("{:x}", hasher.finalize());
        if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
            bail!("checksum verification failed: expected {expected_sha256}, got {actual_sha256}");
        }
        atomic_replace(&temp_path, destination)
            .context("failed to install the verified model atomically")
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(target_os = "windows")]
fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(Into::into)
    }
}

#[cfg(not(target_os = "windows"))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;

    #[test]
    fn checksum_rejection_removes_temp_and_preserves_installed_model() {
        let dir = tempdir().expect("temp dir");
        let destination = dir.path().join("model.bin");
        fs::write(&destination, b"known-good").expect("seed model");

        let result = write_verified_stream(
            Cursor::new(b"corrupt download"),
            &destination,
            &"0".repeat(64),
            &AtomicBool::new(false),
            |_| {},
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read(&destination).expect("installed model"),
            b"known-good"
        );
        assert_eq!(fs::read_dir(dir.path()).expect("dir entries").count(), 1);
    }

    #[test]
    fn verified_download_atomically_replaces_the_target() {
        let dir = tempdir().expect("temp dir");
        let destination = dir.path().join("model.bin");
        fs::write(&destination, b"old model").expect("seed model");
        let replacement = b"complete replacement";
        let expected = format!("{:x}", Sha256::digest(replacement));

        write_verified_stream(
            Cursor::new(replacement),
            &destination,
            &expected,
            &AtomicBool::new(false),
            |_| {},
        )
        .expect("verified replacement");

        assert_eq!(
            fs::read(&destination).expect("installed model"),
            replacement
        );
        assert_eq!(fs::read_dir(dir.path()).expect("dir entries").count(), 1);
    }
}
