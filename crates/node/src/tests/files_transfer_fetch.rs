use super::*;

fn plan(files: usize) -> Plan {
    Plan {
        items: (0..files)
            .map(|n| Item {
                path: PathBuf::from(format!("out/{n}")),
                blob: Some(Hash::new(n.to_string())),
                executable: false,
            })
            .collect(),
        manifest: None,
        total: 0,
        expected: 0,
    }
}

#[test]
fn empty_folders_count_against_the_file_limit() {
    let root = Path::new("out");
    let mut fits = plan(MAX_FILES as usize - 2);
    add_empty_folders(&mut fits, root, &["a".into(), "b/c".into()]).unwrap();
    assert_eq!(fits.items.len() as u64, MAX_FILES);
    assert_eq!(fits.items.last().unwrap().path, Path::new("out/b/c"));

    let mut over = plan(MAX_FILES as usize - 2);
    let folders: Vec<String> = (0..3).map(|n| format!("empty{n}")).collect();
    let refused = add_empty_folders(&mut over, root, &folders).unwrap_err();
    assert!(format!("{refused:#}").contains("more than"), "{refused:#}");
    assert_eq!(over.items.len() as u64, MAX_FILES - 2);
}
