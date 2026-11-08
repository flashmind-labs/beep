use super::*;

#[test]
fn only_full_paths_are_read() {
    let dir = std::env::temp_dir();
    let home = dir.as_path();
    assert_eq!(
        full_path("~/a.png", Some(home)).expect("a home path is full"),
        home.join("a.png")
    );
    assert_eq!(
        full_path(home.join("a.png").to_str().expect("UTF-8 test path"), None)
            .expect("an absolute path is full"),
        home.join("a.png")
    );
    assert!(full_path("a.png", Some(home)).is_err());
    assert!(full_path("~/a.png", None).is_err());
}

#[test]
fn only_pictures_are_read() {
    assert_eq!(ImageType::of(Path::new("/x/A.JPG")), Some(ImageType::Jpeg));
    assert_eq!(ImageType::of(Path::new("/x/a.webp")), Some(ImageType::Webp));
    assert_eq!(ImageType::of(Path::new("/x/notes.txt")), None);
    assert!(read(Path::new("/x/notes.txt")).is_err());
}

#[test]
fn a_picture_is_read_with_its_name_and_type() {
    let dir = env::temp_dir().join(format!("beep-pasted-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("the temp folder is writable");
    let file = dir.join("shot.png");
    fs::write(&file, b"png").expect("the temp folder is writable");
    let got = read(&file).expect("the picture was just written");
    fs::remove_dir_all(&dir).expect("the temp folder was just made");
    assert_eq!(got.name, "shot.png");
    assert_eq!(got.mime, ImageType::Png);
    assert_eq!(got.data, "cG5n");
}
