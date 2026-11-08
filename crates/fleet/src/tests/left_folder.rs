use super::*;

#[test]
fn only_a_folder_that_holds_just_the_project_is_deleted() {
    let none: Vec<String> = Vec::new();
    assert_eq!(why_kept("/srv/widget", &["/home/user"], &none), None);
    assert_eq!(why_kept(r"\\?\C:\Users\me\widget", &[], &none), None);
    assert!(why_kept("/srv", &[], &none).is_some());
    assert!(why_kept(r"C:\", &[], &none).is_some());
    assert!(why_kept("/home/user/", &["/home/user"], &none).is_some());
    assert!(why_kept(r"C:\Users\Me", &[r"c:\users\me"], &none).is_some());
    let others = vec!["/srv/widget/tools".to_owned()];
    assert!(why_kept("/srv/widget", &[], &others).is_some());
    let others = vec!["/srv".to_owned(), "/srv/widget-old".to_owned()];
    assert!(why_kept("/srv/widget", &[], &others).is_some());
    let others = vec!["/srv/widget-old".to_owned()];
    assert_eq!(why_kept("/srv/widget", &[], &others), None);
}
