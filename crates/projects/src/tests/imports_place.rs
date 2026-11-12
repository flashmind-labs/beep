use super::*;

fn candidate(id: &str, online: bool, shell: bool) -> Candidate<'_> {
    Candidate {
        id,
        name: id,
        online,
        shell,
    }
}

#[test]
fn auto_takes_the_only_machine_online() {
    let one = [candidate("box", false, false), candidate("mac", true, true)];
    assert_eq!(choose(&one, None), Ok(1));
    assert_eq!(choose(&one, Some(" ")), Ok(1));
    let two = [
        candidate("mac", true, true),
        candidate("studio", true, true),
    ];
    assert_eq!(
        choose(&two, None),
        Err(ImportError::PickOne("mac, studio".into()))
    );
    assert_eq!(
        choose(&[candidate("box", false, false)], None),
        Err(ImportError::NoneOnline)
    );
}

#[test]
fn projects_go_in_the_projects_folder_else_the_home() {
    let folders = |home, projects_in| Folders { home, projects_in };
    let root = |home, projects_in| projects_root(folders(home, projects_in));
    assert_eq!(root(Some("/home/me"), None), Some("/home/me".into()));
    assert_eq!(root(Some("/home/me"), Some("~/")), Some("/home/me".into()));
    assert_eq!(
        root(Some("/home/me"), Some("~/code")),
        Some("/home/me/code".into())
    );
    assert_eq!(root(None, Some("/srv/code")), Some("/srv/code".into()));
    assert_eq!(root(None, Some("~/code")), None);
    assert_eq!(
        root(Some(r"C:\Users\me"), Some("~")),
        Some(r"C:\Users\me".into())
    );
    let repo = RepoRef::parse("acme/widget", &[]).expect("org/repo parses");
    assert_eq!(
        checkout_folder(folders(Some(r"C:\Users\me"), None), &repo),
        Some(r"C:\Users\me\acme\widget".into())
    );
}
