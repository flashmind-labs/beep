use super::*;

#[test]
fn the_config_dir_follows_the_variable_then_home() {
    let home = Some(PathBuf::from("/home/user"));
    let usual = Some(PathBuf::from("/home/user/.claude"));
    assert_eq!(config_dir_in(None, home.clone()), usual);
    assert_eq!(config_dir_in(Some("".into()), home.clone()), usual);
    let set = config_dir_in(Some("/srv/claude".into()), home);
    assert_eq!(set, Some(PathBuf::from("/srv/claude")));
}

#[test]
fn folders_slug_like_claude_code() {
    assert_eq!(
        slug(Path::new("/srv/my_project/.worktrees/v2")),
        "-srv-my-project--worktrees-v2"
    );
}
