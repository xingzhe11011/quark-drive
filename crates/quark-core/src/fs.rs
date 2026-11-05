use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::{CoreError, Result};

#[derive(Debug, Clone)]
pub struct LocalEntry {
    pub relative: PathBuf,
    pub absolute: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

pub fn collect_local_tree(root: &Path) -> Result<Vec<LocalEntry>> {
    if !root.is_dir() {
        return Err(CoreError::InvalidInput(format!(
            "不是文件夹: {}",
            root.display()
        )));
    }
    let mut entries = Vec::new();
    for item in WalkDir::new(root).min_depth(1).follow_links(false) {
        let item = item.map_err(|err| CoreError::Other(err.to_string()))?;
        let file_type = item.file_type();
        if file_type.is_symlink() {
            continue;
        }
        let relative = item
            .path()
            .strip_prefix(root)
            .map_err(|err| CoreError::Other(err.to_string()))?
            .to_path_buf();
        let is_dir = file_type.is_dir();
        let size = if is_dir {
            0
        } else {
            item.metadata()
                .map_err(|err| CoreError::Other(err.to_string()))?
                .len()
        };
        entries.push(LocalEntry {
            relative,
            absolute: item.path().to_path_buf(),
            is_dir,
            size,
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.relative.cmp(&right.relative))
    });
    Ok(entries)
}

pub fn sanitize_file_name(name: &str) -> String {
    let mut cleaned = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch == '/' || ch == '\\' || ch == '\0' || ch.is_control() {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() {
        return "未命名".to_string();
    }
    if cleaned.len() <= 240 {
        return cleaned;
    }
    let mut end = 240;
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    cleaned[..end].to_string()
}

pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 1..10_000 {
        let name = match extension {
            Some(ext) if !ext.is_empty() => format!("{stem} ({index}).{ext}"),
            _ => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-{}", uuid::Uuid::new_v4()))
}
