//! Walk a directory of vendored `.fish` files in deterministic order.

use std::path::{Path, PathBuf};

/// One snapshot file: the absolute path on disk and the command name
/// derived from the file's stem (e.g. `curl.fish` → `"curl"`).
#[derive(Debug, Clone)]
pub struct SnapshotFile {
    pub path: PathBuf,
    pub default_command: String,
}

/// List every `*.fish` file in `dir`, sorted by file name.
///
/// The sort makes the build deterministic: the same input directory
/// always yields the same blob byte-for-byte.
pub fn list_fish_files(dir: &Path) -> std::io::Result<Vec<SnapshotFile>> {
    let mut files: Vec<SnapshotFile> = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".fish") else {
            continue;
        };
        files.push(SnapshotFile {
            path: path.clone(),
            default_command: stem.to_owned(),
        });
    }
    files.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    Ok(files)
}
