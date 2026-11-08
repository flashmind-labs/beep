use super::*;
use std::fs;

#[test]
fn image_paths_are_found_however_they_are_written() {
    let text = "Saved the chart to `out/chart.png`. See ![plot](plots/a.SVG), \
        /srv/project/shot.jpeg and file:///srv/project/b.webp: done. \
        Not https://example.com/c.png, notes.txt or ~/d.png";
    let found: Vec<&str> = named_in(text).collect();
    assert_eq!(
        found,
        [
            "out/chart.png",
            "plots/a.SVG",
            "/srv/project/shot.jpeg",
            "/srv/project/b.webp"
        ]
    );
}

#[test]
fn only_real_images_inside_the_folder_are_read() {
    let root = std::env::temp_dir().join(format!("beep-image-files-{}", std::process::id()));
    let cwd = root.join("project");
    fs::create_dir_all(cwd.join("out")).unwrap();
    let png = b"\x89PNG\r\n\x1a\nchart".to_vec();
    fs::write(cwd.join("out/chart.png"), &png).unwrap();
    fs::write(cwd.join("fake.png"), b"not a picture").unwrap();
    fs::write(cwd.join("logo.svg"), b"<?xml version=\"1.0\"?><svg/>").unwrap();
    fs::write(root.join("outside.png"), &png).unwrap();
    let mut big = png.clone();
    big.resize(MAX_FILE + 1, 0);
    fs::write(cwd.join("big.png"), &big).unwrap();
    let paths: Vec<PathBuf> = [
        "out/chart.png",
        "fake.png",
        "logo.svg",
        "../outside.png",
        "big.png",
        "missing.png",
    ]
    .iter()
    .filter_map(|path| resolve(&cwd, path))
    .collect();
    let read = read(&cwd, &paths);
    assert_eq!(read.len(), 2, "{read:?}");
    assert_eq!(read[0].data, png);
    assert_eq!(read[0].mime, "image/png");
    assert_eq!(read[1].mime, "image/svg+xml");
    fs::remove_dir_all(&root).unwrap();
}
