//! Where an imported project goes on a machine, and which machine takes it by default.

use super::{ImportError, repo::RepoRef};

/// What the server knows of a machine's folders.
#[derive(Debug, Clone, Copy)]
pub struct Folders<'a> {
    /// `linux`, `macos` or `windows`; `None` before its first report.
    pub os: Option<&'a str>,
    pub home: Option<&'a str>,
    /// The user's files and code, on the user's own Linux environment.
    pub workspace: Option<&'a str>,
}

/// `<workspace or home>/projects/<org>/<repo>` on Linux, else `~/Projects/<org>/<repo>`; `None` without a home.
pub fn checkout_folder(folders: Folders<'_>, repo: &RepoRef) -> Option<String> {
    project_folder(folders, &[repo.org(), repo.name()])
}

/// `parts` under the machine's projects folder; `None` without a home.
pub fn project_folder(folders: Folders<'_>, parts: &[&str]) -> Option<String> {
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
    let mut all = vec![base.trim_end_matches(sep), projects];
    all.extend_from_slice(parts);
    Some(all.join(&sep.to_string()))
}

/// What the chooser knows of one of the user's machines.
#[derive(Debug)]
pub struct Candidate<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub online: bool,
    /// Online and runs commands.
    pub shell: bool,
    /// The user's environment, with a workspace.
    pub workspace: bool,
}

/// The machine to import onto, by index: `wanted`, else the environment, else the only online one.
pub fn choose(candidates: &[Candidate<'_>], wanted: Option<&str>) -> Result<usize, ImportError> {
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
#[path = "../tests/imports_place.rs"]
mod tests;
