use super::*;

#[test]
fn compiled_javascript_loads_without_rewriting() {
    let root = tempfile::tempdir().unwrap();
    let source = "export const cat = { name: 'tom' };\n";
    std::fs::write(root.path().join("main.js"), source).unwrap();
    let mut files = Sources::new(root.path().canonicalize().unwrap());

    assert_eq!(files.read("main.js").unwrap(), source);
}

#[test]
fn source_files_must_be_built_before_loading() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("main.ts"), "const cat: string = 'tom';").unwrap();
    let mut files = Sources::new(root.path().canonicalize().unwrap());

    let error = files.read("main.ts").unwrap_err();

    assert!(
        error.contains("build plugin sources to JavaScript"),
        "{error}"
    );
}
