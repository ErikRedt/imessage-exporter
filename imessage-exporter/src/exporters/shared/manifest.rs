//! Archive-integrity manifest written after each export.
//!
//! The manifest deliberately hashes only files under `attachments/`: these are
//! the source material a user needs before deleting messages from Messages.

use std::{
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::app::{compatibility::attachment_manager::AttachmentManagerMode, error::RuntimeError};

/// Stable filename placed at the root of every export.
pub const MANIFEST_FILENAME: &str = "archive-manifest.json";

#[derive(Serialize)]
struct Manifest<'a> {
    schema_version: u8,
    complete: bool,
    format: &'a str,
    attachment_copy_mode: String,
    standalone_attachment_archive: bool,
    export_error_count: u64,
    attachments: Attachments,
}

#[derive(Serialize)]
struct Attachments {
    copied_count: usize,
    missing_count: u64,
    files: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct ManifestFile {
    path: String,
    bytes: u64,
    sha256: String,
}

/// Write an atomically-replaced integrity manifest after transcript writers
/// have flushed. A failed write fails the export rather than silently claiming
/// an archive can be verified.
pub fn write_manifest(
    export_path: &Path,
    format: &str,
    attachment_mode: &AttachmentManagerMode,
    export_error_count: u64,
    missing_count: u64,
) -> Result<(), RuntimeError> {
    let files = attachment_files(&export_path.join("attachments"), export_path)?;
    let copying_enabled = !matches!(attachment_mode, AttachmentManagerMode::Disabled);
    let complete = export_error_count == 0;
    let manifest = Manifest {
        schema_version: 1,
        complete,
        format,
        attachment_copy_mode: attachment_mode.to_string(),
        standalone_attachment_archive: copying_enabled && complete && missing_count == 0,
        export_error_count,
        attachments: Attachments {
            copied_count: files.len(),
            missing_count,
            files,
        },
    };

    let mut temp = tempfile::NamedTempFile::new_in(export_path)?;
    serde_json::to_writer_pretty(&mut temp, &manifest).map_err(std::io::Error::other)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(export_path.join(MANIFEST_FILENAME))
        .map_err(|error| RuntimeError::DiskError(error.error))?;
    Ok(())
}

fn attachment_files(
    attachment_root: &Path,
    export_path: &Path,
) -> Result<Vec<ManifestFile>, RuntimeError> {
    let metadata = match fs::symlink_metadata(attachment_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RuntimeError::DiskError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "attachment archive root must be a directory, not a symlink: {}",
                attachment_root.display()
            ),
        )));
    }

    let mut paths = Vec::new();
    collect_files(attachment_root, &mut paths)?;
    paths.sort();
    paths
        .iter()
        .map(|path| hash_file(path, export_path))
        .collect()
}

fn collect_files(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), RuntimeError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_symlink() {
            return Err(RuntimeError::DiskError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "refusing to hash symlink in attachment archive: {}",
                    path.display()
                ),
            )));
        }
        if file_type.is_dir() {
            collect_files(&path, paths)?;
        } else if file_type.is_file() {
            paths.push(path);
        }
    }
    Ok(())
}

fn hash_file(path: &Path, export_path: &Path) -> Result<ManifestFile, RuntimeError> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        bytes += read as u64;
    }
    let relative = path.strip_prefix(export_path).map_err(|_| {
        RuntimeError::DiskError(std::io::Error::other(
            "attachment is outside the export directory",
        ))
    })?;
    Ok(ManifestFile {
        path: manifest_path(relative),
        bytes,
        sha256: digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}

fn manifest_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;

    use super::{MANIFEST_FILENAME, write_manifest};
    use crate::app::compatibility::attachment_manager::AttachmentManagerMode;

    #[test]
    fn manifest_hashes_each_attachment_and_reports_missing_files() {
        let temp = tempfile::tempdir().unwrap();
        let attachment = temp.path().join("attachments/42/photo.jpg");
        fs::create_dir_all(attachment.parent().unwrap()).unwrap();
        fs::write(&attachment, b"archive me").unwrap();

        write_manifest(temp.path(), "html", &AttachmentManagerMode::Clone, 1, 1).unwrap();

        let manifest: Value =
            serde_json::from_slice(&fs::read(temp.path().join(MANIFEST_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(manifest["complete"], false);
        assert_eq!(manifest["attachments"]["copied_count"], 1);
        assert_eq!(manifest["attachments"]["missing_count"], 1);
        assert_eq!(
            manifest["attachments"]["files"][0]["path"],
            "attachments/42/photo.jpg"
        );
        assert_eq!(
            manifest["attachments"]["files"][0]["sha256"],
            "46a072ffae872e7f69b3a25152f8685d2762232422df6cd5ecac2787d8ab6e63"
        );
    }

    #[cfg(unix)]
    #[test]
    fn manifest_rejects_a_symlinked_attachment_root() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), temp.path().join("attachments")).unwrap();

        assert!(write_manifest(temp.path(), "html", &AttachmentManagerMode::Clone, 0, 0).is_err());
    }
}
