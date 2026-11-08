use super::{file_name, free_name, image_mime, picture_name};
use std::{env, fs};
use uuid::Uuid;

#[test]
fn image_mime_takes_picture_types_only() {
    assert!(image_mime("image/jpeg"));
    assert!(image_mime("image/svg+xml"));
    assert!(!image_mime("image/"));
    assert!(!image_mime("text/html"));
    assert!(!image_mime("image/png;base64,x"));
}

#[test]
fn free_name_numbers_a_taken_name() {
    let dir = env::temp_dir().join(format!("beep-free-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).expect("the temp folder is writable");
    assert_eq!(free_name(&dir, "chart.png"), dir.join("chart.png"));
    fs::write(dir.join("chart.png"), b"x").expect("the temp folder is writable");
    assert_eq!(free_name(&dir, "chart.png"), dir.join("chart 2.png"));
    fs::write(dir.join("notes"), b"x").expect("the temp folder is writable");
    assert_eq!(free_name(&dir, "notes"), dir.join("notes 2"));
    fs::remove_dir_all(&dir).expect("the temp folder is removable");
}

#[test]
fn file_name_stays_in_its_folder() {
    assert_eq!(file_name("Pass.pdf"), "Pass.pdf");
    assert_eq!(file_name("../../etc/passwd"), "passwd");
    assert_eq!(file_name(".hidden"), "hidden");
    assert_eq!(file_name(".."), "file");
    assert_eq!(file_name(""), "file");
}

#[test]
fn a_saved_picture_gets_its_types_extension() {
    assert_eq!(picture_name("image-0123", "image/png"), "image-0123.png");
    assert_eq!(picture_name("photo", "image/jpeg"), "photo.jpg");
    assert_eq!(picture_name("shot.png", "image/png"), "shot.png");
    assert_eq!(picture_name("../x", "image/webp"), "x.webp");
}
