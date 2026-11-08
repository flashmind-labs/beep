use super::*;
use std::fs;
use tempfile::TempDir;

fn folder_with(name: &str, data: &[u8]) -> (TempDir, String) {
    let dir = TempDir::new().expect("a temp folder can be made");
    let path = dir.path().join(name);
    fs::write(&path, data).expect("the temp folder is writable");
    let path = path.to_str().expect("temp paths are UTF-8").to_owned();
    (dir, path)
}

#[test]
fn reads_a_chunk_at_an_offset() {
    let (_dir, path) = folder_with("clip.mp4", b"0123456789");
    let chunk = read_chunk(&path, 4, None).unwrap().unwrap();
    assert_eq!(
        chunk,
        Chunk {
            size: 10,
            mime: "video/mp4",
            data: b"456789".to_vec(),
        }
    );
}

#[test]
fn refuses_other_types() {
    let (_dir, path) = folder_with("notes.txt", b"secret");
    let e = read_chunk(&path, 0, None).unwrap_err();
    assert!(e.to_string().contains("is not a picture"), "{e:#}");
}

#[test]
fn refuses_relative_paths() {
    let e = read_chunk("out/clip.mp4", 0, None).unwrap_err();
    assert!(e.to_string().contains("not an absolute path"), "{e:#}");
}
