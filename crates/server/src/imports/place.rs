//! Where an imported project goes on a machine, and which machine takes it by default.

use super::{ImportError, repo::RepoRef};

/// What the server knows of a machine's folders.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Folders<'a> {
    /// `linux`, `macos` or `windows`; `None` before its first report.
    pub(crate) os: Option<&'a str>,
    pub(crate) home: Option<&'a str>,
    /// The user's files and code, on the user's own Linux environment.
    pub(crate) workspace: Option<&'a str>,
}

/// `<workspace or home>/projects/<org>/<repo>` on Linux, else `~/Projects/<org>/<repo>`; `None` without a home.
pub(crate) fn checkout_folder(folders: Folders<'_>, repo: &RepoRef) -> Option<String> {
    let linux = match folders.os {
        Some(os) => os == "linux",
        // Only a Linux node runs with a workspace.
        None => folders.workspace.is_some(),
    };
    let (base, projects) = if linux {
        (folders.workspace.or(folders.home)?, "projects")
    } else {
        (folders.home?, "Projects")
    };
    let sep = if base.contains('\\') && !base.contains('/') {
        '\\'
    } else {
        '/'
    };
    let base = base.trim_end_matches(sep);
    Some(
        [base, projects, repo.org(), repo.name()]
            .join(&sep.to_string())
            .to_string(),
    )
}

/// What the chooser knows of one of the user's machines.
#[derive(Debug)]
pub(crate) struct Candidate<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
    pub(crate) online: bool,
    /// Online and runs commands.
    pub(crate) shell: bool,
    /// The user's environment, with a workspace.
    pub(crate) workspace: bool,
}

/// The machine to import onto, by index: `wanted`, else the environment, else the only online one.
pub(crate) fn choose(
    candidates: &[Candidate<'_>],
    wanted: Option<&str>,
) -> Result<usize, ImportError> {
    if let Some(id) = wanted.map(str::trim).filter(|id| !id.is_empty()) {
        let at = candidates
            .iter()
            .position(|c| c.id == id)
            .ok_or(ImportError::NoSuchMachine)?;
        let found = &candidates[at];
        if !found.online {
            return Err(ImportError::Offline(found.name.to_owned()));
        }
        if !found.shell {
            return Err(ImportError::NoShell(found.name.to_owned()));
        }
        return Ok(at);
    }
    let able: Vec<usize> = (0..candidates.len())
        .filter(|&at| candidates[at].shell)
        .collect();
    if let Some(&at) = able.iter().find(|&&at| candidates[at].workspace) {
        return Ok(at);
    }
    match able[..] {
        [] => Err(ImportError::NoneOnline),
        [only] => Ok(only),
        _ => Err(ImportError::PickOne(
            able.iter()
                .map(|&at| candidates[at].name)
                .collect::<Vec<_>>()
                .join(", "),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> RepoRef {
        RepoRef::parse("n0-computer/iroh", &[]).unwrap()
    }

    fn folders<'a>(
        os: Option<&'a str>,
        home: Option<&'a str>,
        workspace: Option<&'a str>,
    ) -> Folders<'a> {
        Folders {
            os,
            home,
            workspace,
        }
    }

    #[test]
    fn linux_uses_the_workspace_then_home() {
        let repo = repo();
        assert_eq!(
            checkout_folder(
                folders(Some("linux"), Some("/home/u"), Some("/srv/work/")),
                &repo
            )
            .as_deref(),
            Some("/srv/work/projects/n0-computer/iroh")
        );
        assert_eq!(
            checkout_folder(folders(Some("linux"), Some("/home/u"), None), &repo).as_deref(),
            Some("/home/u/projects/n0-computer/iroh")
        );
        // Not reported yet, but a workspace means the Linux environment.
        assert_eq!(
            checkout_folder(folders(None, None, Some("/srv/work")), &repo).as_deref(),
            Some("/srv/work/projects/n0-computer/iroh")
        );
    }

    #[test]
    fn macos_and_windows_use_projects_in_home() {
        let repo = repo();
        assert_eq!(
            checkout_folder(folders(Some("macos"), Some("/Users/u"), None), &repo).as_deref(),
            Some("/Users/u/Projects/n0-computer/iroh")
        );
        assert_eq!(
            checkout_folder(folders(Some("windows"), Some(r"C:\Users\u"), None), &repo).as_deref(),
            Some(r"C:\Users\u\Projects\n0-computer\iroh")
        );
        assert_eq!(
            checkout_folder(folders(Some("macos"), None, None), &repo),
            None
        );
    }

    fn candidate<'a>(id: &'a str, online: bool, shell: bool, workspace: bool) -> Candidate<'a> {
        Candidate {
            id,
            name: id,
            online,
            shell,
            workspace,
        }
    }

    #[test]
    fn a_named_machine_must_be_online_and_run_commands() {
        let machines = [
            candidate("box", true, true, true),
            candidate("mac", false, false, false),
            candidate("tablet", true, false, false),
        ];
        assert_eq!(choose(&machines, Some("box")), Ok(0));
        assert_eq!(
            choose(&machines, Some("mac")),
            Err(ImportError::Offline("mac".into()))
        );
        assert_eq!(
            choose(&machines, Some("tablet")),
            Err(ImportError::NoShell("tablet".into()))
        );
        assert_eq!(
            choose(&machines, Some("phone")),
            Err(ImportError::NoSuchMachine)
        );
    }

    #[test]
    fn auto_prefers_the_environment_then_the_only_machine() {
        let machines = [
            candidate("mac", true, true, false),
            candidate("box", true, true, true),
        ];
        assert_eq!(choose(&machines, None), Ok(1));
        assert_eq!(choose(&machines, Some(" ")), Ok(1));
        let one = [
            candidate("box", false, false, true),
            candidate("mac", true, true, false),
        ];
        assert_eq!(choose(&one, None), Ok(1));
        let two = [
            candidate("mac", true, true, false),
            candidate("studio", true, true, false),
        ];
        assert_eq!(
            choose(&two, None),
            Err(ImportError::PickOne("mac, studio".into()))
        );
        assert_eq!(
            choose(&[candidate("box", false, false, true)], None),
            Err(ImportError::NoneOnline)
        );
    }
}
