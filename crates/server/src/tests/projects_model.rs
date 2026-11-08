use super::*;

#[test]
fn a_start_kept_before_bases_reads_from_the_default_branch() {
    let kept: StartRequest =
        serde_json::from_str(r#"{"machine":null,"agent":"claude","persona":null,"inputs":[]}"#)
            .expect("a start kept before bases");
    assert_eq!(kept.base, None);
    assert_eq!(kept.agent, Some(WorkerAgent::Claude));
}

#[test]
fn a_repo_has_a_memory_scope_by_its_name() {
    let name = "acme/widget".parse::<RepoName>().expect("a valid name");
    assert_eq!(
        memory_scope(&name).expect("a valid scope"),
        Scope::project("widget").expect("a valid scope")
    );
}
