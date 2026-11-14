use super::*;

#[test]
fn strips_type_annotations() {
    let js = strip_types(
        "interface Cat { name: string }\nconst cat: Cat = { name: \"tom\" as string };\nfunction f(x: number): number { return x + 1; }\n",
        Path::new("main.ts"),
    )
    .unwrap();

    assert!(!js.contains("interface"));
    assert!(!js.contains(": number"));
    assert!(js.contains("const cat = {"));
    assert!(js.contains("function f(x)"));
}

#[test]
fn returns_syntax_errors() {
    let error = strip_types("const = ;", Path::new("main.ts")).unwrap_err();

    assert!(matches!(error, SourceError::Syntax(_)));
}

#[test]
fn refuses_unknown_extensions() {
    let error = strip_types("1", Path::new("main.txt")).unwrap_err();

    assert!(matches!(error, SourceError::SourceType(_)));
}
