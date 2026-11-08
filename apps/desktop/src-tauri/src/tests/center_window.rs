use super::*;

#[test]
fn a_project_label_reads_back_as_its_name() {
    for name in ["site", "org/repo", "my project", "café.rs", "a-b_c:d"] {
        let label = project_label(name);
        let hex = label
            .strip_prefix(PROJECT_PREFIX)
            .expect("it has the prefix");
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()), "{label}");
        assert_eq!(label_project(&label).as_deref(), Some(name));
    }
    assert_eq!(label_project(LABEL), None);
    assert_eq!(label_project("research-1"), None);
    assert_eq!(label_project("project-"), None);
    assert_eq!(label_project("project-abc"), None);
    assert_eq!(label_project("project-zz"), None);
}

#[test]
fn a_project_open_in_a_window_is_focused() {
    let site = project_label("site");
    let open = [LABEL, "bar", site.as_str(), "research-1"];
    assert_eq!(pick("site", open), Pick::Focus(site));
}

#[test]
fn a_project_without_a_window_gets_a_new_one() {
    assert_eq!(
        pick("site", [LABEL, "bar"]),
        Pick::Open {
            label: project_label("site"),
            cascade: 0,
        }
    );
}

#[test]
fn a_new_window_steps_past_the_other_project_windows() {
    let docs = project_label("docs");
    let api = project_label("api");
    let open = [LABEL, docs.as_str(), "research-1", api.as_str()];
    assert_eq!(
        pick("site", open),
        Pick::Open {
            label: project_label("site"),
            cascade: 2,
        }
    );
}

#[test]
fn similar_names_get_their_own_windows() {
    let site = project_label("site");
    assert!(matches!(pick("site2", [site.as_str()]), Pick::Open { .. }));
    assert!(matches!(pick("sit", [site.as_str()]), Pick::Open { .. }));
}
