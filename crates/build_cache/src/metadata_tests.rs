use std::fs;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use serde_json::{Value, json};

use super::{CacheMetadataError, CachePathUsage, write_cache_metadata};
use crate::spacectl::Mount;

fn mount(root: &Path, path: &str, mode: &str, target: &str) -> Mount {
    Mount {
        mode: mode.to_owned(),
        cache_path: root.join(path),
        mount_path: PathBuf::from(target),
        cache_hit: false,
    }
}

fn metadata_path(root: &Path) -> PathBuf {
    root.join(".ns/cache-metadata.json")
}

fn existing_document(root: &Path, contents: &[u8]) {
    fs::create_dir(root.join(".ns")).unwrap();
    fs::write(metadata_path(root), contents).unwrap();
}

fn read_document(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(metadata_path(root)).unwrap()).unwrap()
}

fn git_usage() -> (PathBuf, CachePathUsage) {
    (
        PathBuf::from("git-mirrors"),
        CachePathUsage {
            source: "warp".to_owned(),
            cache_framework: Some("git".to_owned()),
            mount_target: Vec::new(),
        },
    )
}

#[test]
fn merges_relative_mount_usage_without_losing_unknown_fields() {
    let root = tempfile::tempdir().unwrap();
    existing_document(
        root.path(),
        br#"{
            "version": 1,
            "future": {"enabled": true},
            "userRequest": {
                "third-party": {"source": "other", "future": 7},
                "repos/key/target": {"future": "keep", "cacheFramework": "old"}
            }
        }"#,
    );
    let mounts = [
        mount(root.path(), "repos/key/target", "rust", "/work/z/target"),
        mount(root.path(), "repos/key/target", "rust", "/work/a/target"),
        mount(root.path(), "repos/key/target", "rust", "/work/z/target"),
    ];

    write_cache_metadata(root.path(), &mounts, [git_usage()]).unwrap();

    let document = read_document(root.path());
    assert_eq!(document["version"], 1);
    assert_eq!(document["future"], json!({"enabled": true}));
    assert_eq!(
        document["userRequest"]["third-party"],
        json!({"source": "other", "future": 7})
    );
    assert_eq!(
        document["userRequest"]["repos/key/target"],
        json!({
            "source": "warp",
            "cacheFramework": "rust",
            "mountTarget": ["/work/a/target", "/work/z/target"],
            "future": "keep"
        })
    );
    assert_eq!(
        document["userRequest"]["git-mirrors"],
        json!({"source": "warp", "cacheFramework": "git", "mountTarget": []})
    );
    let updated_at = document["updatedAt"].as_str().unwrap();
    assert!(updated_at.ends_with('Z'));
    assert_eq!(
        DateTime::parse_from_rfc3339(updated_at)
            .unwrap()
            .offset()
            .local_minus_utc(),
        0
    );
    assert_eq!(fs::read_dir(root.path().join(".ns")).unwrap().count(), 1);
}

#[test]
fn preserves_snake_case_map_without_creating_a_second_map() {
    let root = tempfile::tempdir().unwrap();
    existing_document(
        root.path(),
        br#"{"version":1,"updated_at":"old","user_request":{"other":{"source":"third-party"}}}"#,
    );

    write_cache_metadata(root.path(), &[], [git_usage()]).unwrap();

    let document = read_document(root.path());
    assert!(document.get("userRequest").is_none());
    assert!(document.get("updated_at").is_none());
    assert!(document["updatedAt"].is_string());
    assert_eq!(
        document["user_request"]["other"],
        json!({"source": "third-party"})
    );
    assert_eq!(
        document["user_request"]["git-mirrors"],
        json!({"source": "warp", "cacheFramework": "git", "mountTarget": []})
    );
}

#[test]
fn ambiguous_or_empty_modes_omit_framework_and_sort_targets() {
    let root = tempfile::tempdir().unwrap();
    existing_document(
        root.path(),
        br#"{"version":1,"userRequest":{"shared/cache":{"cache_framework":"old","mount_target":[]}}}"#,
    );
    let mounts = [
        mount(root.path(), "shared/cache", "rust", "/work/z"),
        mount(root.path(), "shared/cache", "go", "/work/a"),
        mount(root.path(), "shared/manual", "", "/work/manual"),
    ];

    write_cache_metadata(root.path(), &mounts, []).unwrap();

    let document = read_document(root.path());
    assert_eq!(
        document["userRequest"]["shared/cache"],
        json!({"source": "warp", "mountTarget": ["/work/a", "/work/z"]})
    );
    assert_eq!(
        document["userRequest"]["shared/manual"],
        json!({"source": "warp", "mountTarget": ["/work/manual"]})
    );
}

#[test]
fn malformed_document_remains_unchanged() {
    let root = tempfile::tempdir().unwrap();
    existing_document(root.path(), b"{not json");

    assert!(matches!(
        write_cache_metadata(root.path(), &[], [git_usage()]),
        Err(CacheMetadataError::Malformed)
    ));
    assert_eq!(fs::read(metadata_path(root.path())).unwrap(), b"{not json");
}

#[test]
fn unsupported_version_remains_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let contents = br#"{"version":2,"userRequest":{}}"#;
    existing_document(root.path(), contents);

    assert!(matches!(
        write_cache_metadata(root.path(), &[], [git_usage()]),
        Err(CacheMetadataError::UnsupportedVersion)
    ));
    assert_eq!(fs::read(metadata_path(root.path())).unwrap(), contents);
}

#[test]
fn invalid_map_remains_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let contents = br#"{"version":1,"userRequest":[]}"#;
    existing_document(root.path(), contents);

    assert!(matches!(
        write_cache_metadata(root.path(), &[], [git_usage()]),
        Err(CacheMetadataError::Malformed)
    ));
    assert_eq!(fs::read(metadata_path(root.path())).unwrap(), contents);
}

#[test]
fn volume_escape_mount_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mounts = [Mount {
        cache_path: root.path().join("../outside"),
        ..mount(root.path(), "unused", "rust", "/work/target")
    }];

    assert!(matches!(
        write_cache_metadata(root.path(), &mounts, []),
        Err(CacheMetadataError::InvalidPath)
    ));
    assert!(!metadata_path(root.path()).exists());
}
#[test]
fn absolute_mount_outside_volume_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mounts = [mount(outside.path(), "target", "rust", "/work/target")];

    assert!(matches!(
        write_cache_metadata(root.path(), &mounts, []),
        Err(CacheMetadataError::InvalidPath)
    ));
    assert!(!metadata_path(root.path()).exists());
}

#[test]
fn blocked_metadata_directory_returns_a_nonfatal_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".ns"), b"not a directory").unwrap();

    assert!(matches!(
        write_cache_metadata(root.path(), &[], [git_usage()]),
        Err(CacheMetadataError::Io)
    ));
    assert_eq!(
        fs::read(root.path().join(".ns")).unwrap(),
        b"not a directory"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_document_is_not_followed_or_replaced() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), br#"{"version":1}"#).unwrap();
    fs::create_dir(root.path().join(".ns")).unwrap();
    std::os::unix::fs::symlink(outside.path(), metadata_path(root.path())).unwrap();

    assert!(matches!(
        write_cache_metadata(root.path(), &[], [git_usage()]),
        Err(CacheMetadataError::Symlink)
    ));
    assert!(
        fs::symlink_metadata(metadata_path(root.path()))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(outside.path()).unwrap(), br#"{"version":1}"#);
}

#[cfg(unix)]
#[test]
fn symlinked_metadata_directory_is_not_followed() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".ns")).unwrap();

    assert!(matches!(
        write_cache_metadata(root.path(), &[], [git_usage()]),
        Err(CacheMetadataError::Symlink)
    ));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}
