use super::*;

#[test]
fn verbatim_paths_become_plain() {
    assert_eq!(plain(Path::new(r"\\?\C:\a")), Path::new(r"C:\a"));
    assert_eq!(
        plain(Path::new(r"\\?\UNC\srv\share\a")),
        Path::new(r"\\srv\share\a")
    );
    assert_eq!(plain(Path::new("/home/me/a")), Path::new("/home/me/a"));
    assert_eq!(plain(Path::new(r"C:\a")), Path::new(r"C:\a"));
    assert_eq!(
        plain(Path::new(r"\\?\Volume{abc}\a")),
        Path::new(r"\\?\Volume{abc}\a")
    );
    let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
    assert_eq!(plain(Path::new(&long)), Path::new(&long));
}
