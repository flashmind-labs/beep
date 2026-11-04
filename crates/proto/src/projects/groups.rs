//! Project groups: folders the user picks to list projects under, matched
//! on every machine whatever its home folder is called.

/// A folder as project groups compare it: `\` read as `/`, the home folder
/// (`~/`, `/home/<user>/`, `/Users/<user>/`, `C:/Users/<user>/`) and then a
/// leading `Projects/` (any case) left out, with no trailing `/`. So
/// `~/Projects/work` on a Mac and `/home/user/work` on
/// Linux are both `work`. A folder outside the home folder stays
/// absolute; the home folder itself, or its `Projects`, is empty.
pub fn project_group_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let Some(rest) = home_relative(&path) else {
        let trimmed = path.trim_end_matches('/');
        return if trimmed.is_empty() && path.starts_with('/') {
            "/".to_owned()
        } else {
            trimmed.to_owned()
        };
    };
    let rest = rest.trim_matches('/');
    let rest = match rest.split_once('/') {
        Some((first, after)) if first.eq_ignore_ascii_case("projects") => after,
        None if rest.eq_ignore_ascii_case("projects") => "",
        _ => rest,
    };
    rest.trim_matches('/').to_owned()
}

/// What follows the home folder in `path`, or `None` outside it.
fn home_relative(path: &str) -> Option<&str> {
    if path == "~" {
        return Some("");
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Some(rest);
    }
    let bytes = path.as_bytes();
    let unrooted = if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":/" {
        &path[2..]
    } else {
        path
    };
    for root in ["/home/", "/Users/"] {
        let Some(rest) = unrooted.strip_prefix(root) else {
            continue;
        };
        // The user's folder, then what is in it.
        return match rest.split_once('/') {
            Some((user, rest)) if !user.is_empty() => Some(rest),
            None if !rest.is_empty() => Some(""),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
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
}
