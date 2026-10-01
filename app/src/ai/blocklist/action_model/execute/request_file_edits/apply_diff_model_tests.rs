use std::io::Write as _;

use async_io::block_on;
use tempfile::NamedTempFile;

use super::*;

#[test]
fn test_read_local_file_returns_not_found_for_missing_file() {
    let result = block_on(read_local_file(
        "/nonexistent/path/that/should/not/exist.txt",
    ));
    assert!(matches!(result, FileReadResult::NotFound));
}

#[test]
fn test_read_local_file_at_limit_returns_content() {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temporary file");
    let content = vec![b'a'; MAX_DIFF_READ_BYTES as usize];
    temp_file
        .write_all(&content)
        .expect("Failed to write temporary file");
    let path = temp_file.path().to_string_lossy().to_string();

    match block_on(read_local_file(&path)) {
        FileReadResult::Found(read_content) => assert_eq!(read_content.len(), content.len()),
        FileReadResult::NotFound => panic!("Expected Found, got NotFound"),
        FileReadResult::ReadError(err) => panic!("Expected Found, got ReadError: {err}"),
    }
}

#[test]
fn test_read_local_file_over_limit_returns_read_error_without_partial_content() {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temporary file");
    let content = vec![b'a'; MAX_DIFF_READ_BYTES as usize + 1];
    temp_file
        .write_all(&content)
        .expect("Failed to write temporary file");
    let path = temp_file.path().to_string_lossy().to_string();

    match block_on(read_local_file(&path)) {
        FileReadResult::ReadError(err) => {
            assert!(err.contains(&MAX_DIFF_READ_BYTES.to_string()));
        }
        FileReadResult::Found(_) => panic!("Expected ReadError, got Found with partial content"),
        FileReadResult::NotFound => panic!("Expected ReadError, got NotFound"),
    }
}
