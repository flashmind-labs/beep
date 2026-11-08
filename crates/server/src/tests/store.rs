use super::MIGRATIONS;
use std::fs;

#[test]
fn every_migration_file_is_listed() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");
    let mut files: Vec<i64> = fs::read_dir(dir)
        .expect("the migrations folder is in the crate")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".sql")?.split('_').next()?.parse().ok()
        })
        .collect();
    files.sort_unstable();
    let listed: Vec<i64> = MIGRATIONS.iter().map(|(version, ..)| *version).collect();
    assert_eq!(listed, files);
}
