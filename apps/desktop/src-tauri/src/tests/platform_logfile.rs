use super::{FILES_KEPT, Rolling, path};
use std::{env, fs, io::Write};
use uuid::Uuid;

#[test]
fn rolling_keeps_three_files_under_the_size() {
    let dir = env::temp_dir().join(format!("beep-log-{}", Uuid::new_v4()));
    let mut log = Rolling::open(&dir, 10).expect("the temp dir is writable");

    for line in ["aaaaaa\n", "bbbbbb\n", "cccccc\n", "dddddd\n"] {
        log.write_all(line.as_bytes())
            .expect("the file is writable");
    }

    let read = |n| fs::read_to_string(path(&dir, n)).expect("the file was written");
    assert_eq!(read(0), "dddddd\n");
    assert_eq!(read(1), "cccccc\n");
    assert_eq!(read(2), "bbbbbb\n");
    assert!(!path(&dir, FILES_KEPT).exists());
    fs::remove_dir_all(&dir).expect("the temp dir is removable");
}
