use super::*;

#[test]
fn worktree_roots_and_siblings_move_even_when_the_session_is_in_a_subfolder() {
    for target in ["/srv/target/project", "/srv/source/project"] {
        let tree = Path::new(target).join(".worktrees/fix");
        let folders = Folders {
            old: PathBuf::from("/srv/source/project/.worktrees/fix/src"),
            new: tree.join("src"),
            workspace: Some(Workspace {
                checkout: PathBuf::from("/srv/source/project"),
                old: PathBuf::from("/srv/source/project/.worktrees/fix"),
                new: tree.clone(),
            }),
        };
        for (old, relative) in [
            ("/srv/source/project", ""),
            ("/srv/source/project/.worktrees/fix", ""),
            ("/srv/source/project/.worktrees/fix/src", "src"),
            ("/srv/source/project/.worktrees/fix/assets", "assets"),
        ] {
            assert_eq!(
                PathBuf::from(folders.remap(old).unwrap()),
                tree.join(relative)
            );
        }
        assert!(folders.remap("/srv/other").is_none());
    }
}
