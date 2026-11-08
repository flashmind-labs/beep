use super::*;

#[test]
fn an_edit_shows_its_changed_lines_with_context() {
    let old = "a\nb\nc\nold\nd\ne\nf";
    let new = "a\nb\nc\nnew\nmore\nd\ne\nf";
    assert_eq!(
        diff_text(
            Path::new("/srv/project"),
            Path::new("/srv/project/src/x.rs"),
            Some(old),
            new
        ),
        "[diff] src/x.rs +2 -1\n b\n c\n-old\n+new\n+more\n d\n e"
    );
}

#[test]
fn an_edit_at_the_edges_keeps_what_context_there_is() {
    assert_eq!(
        diff_text(Path::new("/"), Path::new("/f"), Some("x\ny"), "z\ny"),
        "[diff] f +1 -1\n-x\n+z\n y"
    );
}
