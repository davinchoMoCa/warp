use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::is_safe_relative_cache_dir;
use crate::spacectl::Mount;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePathUsage {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_framework: Option<String>,
    pub mount_target: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CacheMetadataError {
    #[error("cache metadata is malformed")]
    Malformed,
    #[error("cache metadata version is unsupported")]
    UnsupportedVersion,
    #[error("cache metadata location is a symlink")]
    Symlink,
    #[error("cache metadata path is not volume-relative")]
    InvalidPath,
    #[error("cache metadata could not be read or written")]
    Io,
}

/// Merge cache usage into Namespace's metadata document, preserving unrelated entries and fields.
pub fn write_cache_metadata(
    cache_root: &Path,
    mounts: &[Mount],
    additional: impl IntoIterator<Item = (PathBuf, CachePathUsage)>,
) -> Result<(), CacheMetadataError> {
    let entries = usage_entries(cache_root, mounts, additional)?;
    let directory = cache_root.join(".ns");
    reject_symlink(&directory)?;
    fs::create_dir_all(&directory).map_err(|_| CacheMetadataError::Io)?;
    reject_symlink(&directory)?;
    let path = directory.join("cache-metadata.json");
    reject_symlink(&path)?;
    let mut document = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Map<String, Value>>(&bytes)
            .map_err(|_| CacheMetadataError::Malformed)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Map::from_iter([("version".to_owned(), json!(1))])
        }
        Err(_) => return Err(CacheMetadataError::Io),
    };
    if document.get("version") != Some(&json!(1)) {
        return Err(CacheMetadataError::UnsupportedVersion);
    }
    let map_name = match (
        document.contains_key("userRequest"),
        document.contains_key("user_request"),
    ) {
        (true, true) => return Err(CacheMetadataError::Malformed),
        (false, true) => "user_request",
        (true, false) | (false, false) => "userRequest",
    };
    let user_request = document
        .entry(map_name)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(CacheMetadataError::Malformed)?;
    for (path, usage) in entries {
        let entry = user_request.entry(path).or_insert_with(|| json!({}));
        let entry = entry.as_object_mut().ok_or(CacheMetadataError::Malformed)?;
        entry.remove("cache_framework");
        entry.remove("mount_target");
        entry.remove("cacheFramework");
        entry.extend(
            serde_json::to_value(usage)
                .map_err(|_| CacheMetadataError::Malformed)?
                .as_object()
                .ok_or(CacheMetadataError::Malformed)?
                .clone(),
        );
    }
    document.remove("updated_at");
    document.insert(
        "updatedAt".to_owned(),
        json!(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)),
    );
    let mut temporary =
        tempfile::NamedTempFile::new_in(&directory).map_err(|_| CacheMetadataError::Io)?;
    serde_json::to_writer_pretty(&mut temporary, &document).map_err(|_| CacheMetadataError::Io)?;
    temporary.flush().map_err(|_| CacheMetadataError::Io)?;
    reject_symlink(&path)?;
    temporary
        .persist(&path)
        .map_err(|_| CacheMetadataError::Io)?;
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), CacheMetadataError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CacheMetadataError::Symlink),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CacheMetadataError::Io),
    }
}

fn relative_key(path: &Path) -> Result<String, CacheMetadataError> {
    if !is_safe_relative_cache_dir(path) {
        return Err(CacheMetadataError::InvalidPath);
    }
    path.to_str()
        .map(|path| path.replace(std::path::MAIN_SEPARATOR, "/"))
        .ok_or(CacheMetadataError::InvalidPath)
}

fn usage_entries(
    cache_root: &Path,
    mounts: &[Mount],
    additional: impl IntoIterator<Item = (PathBuf, CachePathUsage)>,
) -> Result<BTreeMap<String, CachePathUsage>, CacheMetadataError> {
    let cache_root = std::path::absolute(cache_root).map_err(|_| CacheMetadataError::Io)?;
    let mut grouped = BTreeMap::<String, (BTreeSet<&str>, BTreeSet<String>)>::new();
    for mount in mounts {
        // Older spacectl responses may omit paths; they cannot be attributed to a volume entry.
        if mount.cache_path.as_os_str().is_empty() || mount.mount_path.as_os_str().is_empty() {
            continue;
        }
        let relative = mount
            .cache_path
            .strip_prefix(&cache_root)
            .map_err(|_| CacheMetadataError::InvalidPath)?;
        let key = relative_key(relative)?;
        let (modes, targets) = grouped.entry(key).or_default();
        modes.insert(&mount.mode);
        targets.insert(
            mount
                .mount_path
                .to_str()
                .ok_or(CacheMetadataError::InvalidPath)?
                .to_owned(),
        );
    }
    let mut entries = grouped
        .into_iter()
        .map(|(path, (modes, targets))| {
            let framework = if modes.len() == 1 {
                modes.first().filter(|mode| !mode.is_empty())
            } else {
                None
            };
            (
                path,
                CachePathUsage {
                    source: "warp".to_owned(),
                    cache_framework: framework.map(|mode| (*mode).to_owned()),
                    mount_target: targets.into_iter().collect(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (path, usage) in additional {
        entries.insert(relative_key(&path)?, usage);
    }
    Ok(entries)
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
