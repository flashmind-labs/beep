use super::{pack, packed_size, unpack};
use std::fs;
use tempfile::TempDir;

#[test]
fn a_folder_arrives_without_what_other_sessions_bring_and_without_links() {
    let dir = TempDir::new().expect("a temp folder");
    let chat = dir.path().join("ab12");
    fs::create_dir_all(chat.join("research/deep")).unwrap();
    fs::create_dir_all(chat.join("ended")).unwrap();
    fs::write(chat.join("notes.md"), "open: pricing").unwrap();
    fs::write(chat.join("research/deep/found.md"), "theirs").unwrap();
    fs::write(chat.join("ended/found.md"), "kept").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/hosts", chat.join("link")).unwrap();

    let mut bytes = Vec::new();
    pack(&chat, &[chat.join("research")], &mut bytes).expect("it packs");
    let there = dir.path().join("there/ab12");
    fs::create_dir_all(&there).unwrap();
    fs::write(there.join("notes.md"), "older").unwrap();
    unpack(bytes.as_slice(), &there).expect("it unpacks");

    assert_eq!(
        fs::read_to_string(there.join("notes.md")).unwrap(),
        "open: pricing"
    );
    assert_eq!(
        fs::read_to_string(there.join("ended/found.md")).unwrap(),
        "kept"
    );
    assert!(!there.join("research").exists());
    assert!(fs::symlink_metadata(there.join("link")).is_err());
}

#[test]
fn a_folder_that_is_gone_packs_as_nothing() {
    let dir = TempDir::new().expect("a temp folder");
    let mut bytes = Vec::new();
    pack(&dir.path().join("gone"), &[], &mut bytes).expect("it packs");
    unpack(bytes.as_slice(), &dir.path().join("there")).expect("it unpacks");
    assert_eq!(fs::read_dir(dir.path().join("there")).unwrap().count(), 0);
}

#[test]
fn a_file_that_names_a_place_outside_the_folder_is_refused() {
    let dir = TempDir::new().expect("a temp folder");
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o644);
    // `set_path` refuses `..`, so the name is written as bytes.
    header.as_gnu_mut().unwrap().name[..9].copy_from_slice(b"../escape");
    header.set_cksum();
    tar.append(&header, &b"x"[..]).unwrap();
    let bytes = tar.into_inner().unwrap();

    let there = dir.path().join("there");
    assert!(unpack(bytes.as_slice(), &there).is_err());
    assert!(!dir.path().join("escape").exists());
}

#[test]
fn the_size_said_up_front_is_what_the_archive_takes() {
    let dir = TempDir::new().expect("a temp folder");
    let chat = dir.path().join("ab12");
    fs::create_dir_all(chat.join("research")).unwrap();
    fs::write(chat.join("notes.md"), "open: pricing").unwrap();
    fs::write(chat.join("big.bin"), vec![7u8; 70 * 1024]).unwrap();
    fs::write(chat.join("research/found.md"), "theirs").unwrap();
    let apart = [chat.join("research")];

    let mut bytes = Vec::new();
    pack(&chat, &apart, &mut bytes).expect("it packs");

    assert_eq!(
        packed_size(&chat, &apart).expect("it measures"),
        bytes.len() as u64
    );
}
