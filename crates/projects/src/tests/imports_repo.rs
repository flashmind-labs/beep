use super::*;

fn forge() -> Vec<ForgejoUrl> {
    vec![
        "https://git.example.com".parse().unwrap(),
        "https://code.example.org/forge".parse().unwrap(),
    ]
}

fn parse(input: &str) -> Result<RepoRef, ImportError> {
    RepoRef::parse(input, &forge())
}

#[test]
fn github_urls_in_any_common_form() {
    for input in [
        "https://github.com/n0-computer/iroh",
        "https://github.com/n0-computer/iroh.git",
        "https://github.com/n0-computer/iroh/",
        "https://github.com/n0-computer/iroh/tree/main/iroh",
        "https://www.github.com/n0-computer/iroh",
        "github.com/n0-computer/iroh",
        "git@github.com:n0-computer/iroh.git",
        "ssh://git@github.com/n0-computer/iroh.git",
    ] {
        let repo = parse(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(repo.host, Host::GitHub, "{input}");
        assert_eq!(repo.project(), "n0-computer/iroh", "{input}");
    }
}

#[test]
fn forgejo_urls_match_a_saved_server() {
    let repo = parse("https://git.example.com/team/app").unwrap();
    assert_eq!(
        repo.host,
        Host::Forgejo("https://git.example.com".parse().unwrap())
    );
    assert_eq!(repo.clone_url(), "https://git.example.com/team/app.git");
    assert_eq!(repo.host_name(), "git.example.com");
    let sub = parse("https://code.example.org/forge/team/app.git").unwrap();
    assert_eq!(sub.web_url(), "https://code.example.org/forge/team/app");
    assert_eq!(parse("git@git.example.com:team/app.git").unwrap(), repo);
}

#[test]
fn unknown_hosts_and_bad_names_are_refused() {
    assert_eq!(
        parse("https://gitlab.com/team/app"),
        Err(ImportError::UnknownHost("gitlab.com".into()))
    );
    // Another port on a saved server's host is another server.
    assert_eq!(
        parse("https://git.example.com:8443/team/app"),
        Err(ImportError::UnknownHost("git.example.com".into()))
    );
    assert!(parse("ssh://git@git.example.com:2222/team/app.git").is_ok());
    for bad in [
        "",
        "iroh",
        "a/b/c",
        "org/..",
        "org/-rm",
        "org/re po",
        "org/x'y",
        "https://github.com/onlyorg",
        "ftp://github.com/a/b",
    ] {
        assert_eq!(parse(bad), Err(ImportError::Malformed), "{bad}");
    }
}

#[test]
fn an_imported_url_matches_its_checkouts_in_any_form() {
    let url = "https://github.com/acme/widget";
    for origin in [
        "https://github.com/acme/widget",
        "https://github.com/acme/widget.git",
        "https://GitHub.com/Acme/Widget.git/",
        "git@github.com:acme/widget.git",
        "git@github.com:ACME/widget",
        "ssh://git@github.com/acme/widget.git",
        "ssh://git@github.com:22/acme/widget",
        "https://user@github.com/acme/widget",
    ] {
        assert!(same_repo(url, origin, &forge()), "{origin}");
    }
    for origin in [
        "https://github.com/acme/widget-docs",
        "https://github.com/other/widget",
        "https://git.example.com/acme/widget",
        "/srv/mirrors/acme/widget.git",
        "",
    ] {
        assert!(!same_repo(url, origin, &forge()), "{origin}");
    }
    // A Forgejo server under a sub-path, reached over ssh from the root.
    let sub = "https://code.example.org/forge/team/app";
    assert!(same_repo(
        sub,
        "git@code.example.org:team/app.git",
        &forge()
    ));
    assert!(same_repo(
        sub,
        "https://code.example.org/forge/team/app.git",
        &[]
    ));
    assert!(!same_repo(
        sub,
        "git@code.example.org:team/other.git",
        &forge()
    ));
}
