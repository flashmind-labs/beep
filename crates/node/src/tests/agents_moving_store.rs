use super::*;
use tempfile::TempDir;

fn stored(bytes: &[u8]) -> Stored {
    Stored {
        bytes: bytes.to_vec(),
        path: PathBuf::from("s.jsonl"),
    }
}

#[test]
fn every_replacement_is_kept_under_a_name_of_its_own() {
    let dir = TempDir::new().expect("a temp folder");
    let path = dir.path().join("s.jsonl");
    fs::write(&path, b"first\n").expect("a transcript");
    write(&path, &stored(b"second\n")).expect("the first replacement");
    write(&path, &stored(b"third\n")).expect("the second replacement");
    assert_eq!(fs::read(&path).unwrap(), b"third\n");
    let mut aside: Vec<Vec<u8>> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().contains(".replaced-"))
        .map(|f| fs::read(f.path()).unwrap())
        .collect();
    aside.sort();
    assert_eq!(aside, [b"first\n".to_vec(), b"second\n".to_vec()]);
}

#[test]
fn a_transcript_the_new_one_extends_is_replaced_without_a_copy() {
    let dir = TempDir::new().expect("a temp folder");
    let path = dir.path().join("s.jsonl");
    fs::write(&path, b"first\n").expect("a transcript");
    write(&path, &stored(b"first\nmore\n")).expect("an extension");
    assert_eq!(fs::read(&path).unwrap(), b"first\nmore\n");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
