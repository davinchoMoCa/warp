use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{SecondsFormat, Utc};
use serde::Serialize;

const METADATA_FILE: &str = "cache-metadata.json";
static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

/// Configured usage of a cache-volume path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheUsage {
    /// Path relative to the cache-volume root.
    pub path: PathBuf,
    pub cache_framework: Option<String>,
    pub mount_target: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheMetadata {
    version: u32,
    updated_at: String,
    user_request: BTreeMap<String, CachePathUsage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CachePathUsage {
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_framework: Option<String>,
    mount_target: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CacheMetadataError {
    #[error("cache metadata directory is a symlink")]
    Symlink,
    #[error("cache metadata path is not volume-relative")]
    InvalidPath,
    #[error("cache metadata could not be written")]
    Io,
}

/// Replace Namespace usage metadata with a complete snapshot for this instance.
///
/// Pathname operations do not protect against concurrent parent-directory replacement.
pub fn write_cache_metadata(
    cache_root: &Path,
    usages: impl IntoIterator<Item = CacheUsage>,
) -> Result<(), CacheMetadataError> {
    let document = CacheMetadata {
        version: 1,
        updated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        user_request: usage_entries(usages)?,
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(|_| CacheMetadataError::Io)?;
    let directory = normalized_cache_root(cache_root)?.join(".ns");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CacheMetadataError::Symlink);
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(CacheMetadataError::Io),
    }
    fs::create_dir_all(&directory).map_err(|_| CacheMetadataError::Io)?;

    for _ in 0..100 {
        let id = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = directory.join(format!(".cache-metadata-{}-{id}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(CacheMetadataError::Io),
        };
        let result = file.write_all(&bytes).and_then(|()| file.flush());
        drop(file);
        let result = result.and_then(|()| fs::rename(&temporary, directory.join(METADATA_FILE)));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result.map_err(|_| CacheMetadataError::Io);
    }
    Err(CacheMetadataError::Io)
}

fn relative_key(path: &Path) -> Result<String, CacheMetadataError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                parts.push(value.to_str().ok_or(CacheMetadataError::InvalidPath)?);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CacheMetadataError::InvalidPath);
            }
        }
    }
    if parts.is_empty() {
        return Err(CacheMetadataError::InvalidPath);
    }
    Ok(parts.join("/"))
}

pub(crate) fn normalized_cache_root(path: &Path) -> Result<PathBuf, CacheMetadataError> {
    let mut normalized = PathBuf::new();
    for component in std::path::absolute(path)
        .map_err(|_| CacheMetadataError::Io)?
        .components()
    {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

fn usage_entries(
    usages: impl IntoIterator<Item = CacheUsage>,
) -> Result<BTreeMap<String, CachePathUsage>, CacheMetadataError> {
    let mut grouped = BTreeMap::<String, (BTreeSet<Option<String>>, BTreeSet<String>)>::new();
    for usage in usages {
        let key = relative_key(&usage.path)?;
        let (frameworks, targets) = grouped.entry(key).or_default();
        frameworks.insert(usage.cache_framework.filter(|mode| !mode.is_empty()));
        targets.extend(usage.mount_target);
    }
    Ok(grouped
        .into_iter()
        .map(|(path, (frameworks, targets))| {
            let framework = if frameworks.len() == 1 {
                frameworks.into_iter().next().flatten()
            } else {
                None
            };
            (
                path,
                CachePathUsage {
                    source: "warp",
                    cache_framework: framework,
                    mount_target: targets.into_iter().collect(),
                },
            )
        })
        .collect())
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
