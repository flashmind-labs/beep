use super::*;

#[test]
fn the_home_folder_and_projects_are_left_out() {
    for path in [
        "~/work",
        "~/Projects/work",
        "~/projects/work/",
        "/home/user/work",
        "/home/user/Projects/work",
        "/Users/someone/Projects/work",
        "/Users/someone/work/",
        "C:\\Users\\someone\\Projects\\work",
        "  ~/work  ",
    ] {
        assert_eq!(project_group_path(path), "work", "{path}");
    }
    assert_eq!(project_group_path("~/Projects/acme/widget"), "acme/widget");
    assert_eq!(project_group_path("~/work/Projects/x"), "work/Projects/x");
}

#[test]
fn a_home_folder_is_empty_and_other_folders_stay_absolute() {
    for path in [
        "~",
        "~/",
        "/home/user",
        "/Users/someone/",
        "~/Projects",
        "",
        "  ",
    ] {
        assert_eq!(project_group_path(path), "", "{path}");
    }
    assert_eq!(project_group_path("/srv/project/"), "/srv/project");
    assert_eq!(project_group_path("/home/"), "/home");
    assert_eq!(project_group_path("/"), "/");
    assert_eq!(project_group_path("relative/x"), "relative/x");
}
