use super::*;

#[test]
fn windows_paths_lose_the_verbatim_prefix() {
    assert_eq!(plain_path(r"\\?\C:\Users\me\games"), r"C:\Users\me\games");
    assert_eq!(plain_path(r"\\?\UNC\nas\share\x"), r"\\nas\share\x");
    assert_eq!(plain_path(r"\\?\Volume{abc}\x"), r"\\?\Volume{abc}\x");
    assert_eq!(plain_path("/home/me/code"), "/home/me/code");
    assert_eq!(plain_path(r"C:\code"), r"C:\code");
}

#[test]
fn git_sh_paths_become_windows_paths() {
    assert_eq!(
        windows_path("/c/Users/me/games").as_deref(),
        Some(r"C:\Users\me\games")
    );
    assert_eq!(windows_path("/d").as_deref(), Some(r"D:\"));
    assert_eq!(windows_path("/home/me"), None);
    assert_eq!(windows_path("/"), None);
    assert_eq!(windows_path("relative/x"), None);
}

#[test]
fn repo_names_parse() {
    let name = RepoName::from_str("acme/widget").expect("a valid name");
    assert_eq!(name.to_string(), "acme/widget");
    assert_eq!(name.repo(), "widget");
    for bad in [
        "acme", "a/b/c", "../x", "acme/", "ac me/x", "./x", "acme/..",
    ] {
        assert!(RepoName::from_str(bad).is_err(), "{bad} should not parse");
    }
}

#[test]
fn machine_paths_are_absolute_and_clean() {
    for bad in ["/srv/../etc", "srv/project", "/srv/project/", "", "/srv//x"] {
        assert!(
            MachinePath::from_str(bad).is_err(),
            "{bad} should not parse"
        );
    }
}

#[test]
fn branch_names_git_refuses_are_refused() {
    assert_eq!(branch_name(" beep/login "), Some("beep/login"));
    for bad in [
        "", "-x", "a b", "a..b", "a:b", "a~1", "/a", "a/", "x.lock", "a\nb",
    ] {
        assert_eq!(branch_name(bad), None, "{bad:?}");
    }
}

#[test]
fn forgejo_urls_are_made_canonical() {
    let url: ForgejoUrl = " https://git.example.com/api/v1/?x=1 ".parse().unwrap();
    assert_eq!(url.as_ref(), "https://git.example.com");
    let url: ForgejoUrl = "https://code.example.org:3000/forge/".parse().unwrap();
    assert_eq!(url.as_ref(), "https://code.example.org:3000/forge");
    assert_eq!(url.host(), "code.example.org:3000");
    let url: ForgejoUrl = "http://forge.local:3000".parse().unwrap();
    assert_eq!(url.as_ref(), "http://forge.local:3000");
    // Over a VPN a server named like one on the internet can be plain http.
    let url: ForgejoUrl = "http://git.example.com:3000".parse().unwrap();
    assert_eq!(url.as_ref(), "http://git.example.com:3000");
    assert!("ftp://git.example.com".parse::<ForgejoUrl>().is_err());
    assert!("git.example.com".parse::<ForgejoUrl>().is_err());
    assert!(
        "https://me:pw@git.example.com"
            .parse::<ForgejoUrl>()
            .is_err()
    );
}
