use super::*;

fn folder(files: &[(&str, usize, u8)]) -> (tempfile::TempDir, Assets) {
    let root = tempfile::tempdir().unwrap();
    for (name, size, fill) in files {
        std::fs::write(root.path().join(name), vec![*fill; *size]).unwrap();
    }
    let assets = Assets::new(root.path().canonicalize().unwrap());

    (root, assets)
}

#[test]
fn a_hash_is_sixty_four_lowercase_hex_digits() {
    let good = "a".repeat(64);
    assert!(good.parse::<AssetHash>().is_ok());

    for bad in ["", "abc", &"A".repeat(64), &"g".repeat(64), &"a".repeat(65)] {
        assert!(bad.parse::<AssetHash>().is_err(), "{bad:?}");
    }
}

#[test]
fn an_image_is_hashed_by_content_and_read_once() {
    let (root, mut assets) = folder(&[("a.png", 10, 1), ("b.png", 10, 1)]);

    let first = assets.load("a.png").unwrap();
    // The file changes on disk, but the plugin's run keeps the bytes it read.
    std::fs::write(root.path().join("a.png"), b"changed").unwrap();
    let again = assets.load("a.png").unwrap();
    let same_bytes = assets.load("b.png").unwrap();

    assert_eq!(first, again);
    assert_eq!(first.hash(), same_bytes.hash());
    assert_eq!(
        first.hash().to_string(),
        blake3::hash(&[1; 10]).to_hex().as_str()
    );
    assert_eq!(first.bytes(), [1; 10]);
    assert_eq!(first.reference().mime.to_string(), "image/png");
}

#[test]
fn one_image_is_limited_and_so_are_all_of_them() {
    let (_root, mut assets) = folder(&[
        ("big.png", usize::try_from(MAX_IMAGE_BYTES).unwrap() + 1, 0),
        ("one.png", 250_000, 1),
        ("two.png", 250_000, 2),
        ("copy.png", 250_000, 1),
    ]);

    assert!(matches!(
        assets.load("big.png"),
        Err(ImageError::TooLarge(_))
    ));

    // 16 distinct files of 250 KB fill the 4 MB total; a copy of one costs nothing.
    let many: Vec<(String, usize, u8)> = (0..17)
        .map(|index| {
            (
                format!("f{index}.png"),
                250_000,
                u8::try_from(index + 10).unwrap(),
            )
        })
        .collect();
    let files: Vec<(&str, usize, u8)> = many.iter().map(|(n, s, f)| (n.as_str(), *s, *f)).collect();
    let (_root, mut assets) = folder(&files);

    for index in 0..16 {
        assets.load(&format!("f{index}.png")).unwrap();
    }
    assert!(matches!(assets.load("f16.png"), Err(ImageError::TooMuch)));
}

#[test]
fn identical_files_count_once_against_the_total() {
    let copies: Vec<(String, usize, u8)> = (0..40)
        .map(|index| (format!("c{index}.png"), 200_000, 7))
        .collect();
    let files: Vec<(&str, usize, u8)> = copies
        .iter()
        .map(|(n, s, f)| (n.as_str(), *s, *f))
        .collect();
    let (_root, mut assets) = folder(&files);

    for index in 0..40 {
        assets.load(&format!("c{index}.png")).unwrap();
    }
}
