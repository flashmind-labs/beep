//! Where an imported project goes on a machine, and which machine takes it by default.

use super::{ImportError, repo::RepoRef};

/// What is known of a machine's folders.
#[derive(Debug, Clone, Copy)]
pub struct Folders<'a> {
    pub home: Option<&'a str>,
    /// Its "Projects in" folder as written, absolute or under `~`.
    pub projects_in: Option<&'a str>,
}

/// The folder new projects go under: the machine's "Projects in" folder with `~` expanded, else
/// its home; `None` when that needs a home it has not reported.
pub fn projects_root(folders: Folders<'_>) -> Option<String> {
    let Some(projects_in) = folders.projects_in.map(str::trim).filter(|f| !f.is_empty()) else {
        return folders.home.map(str::to_owned);
    };
    let Some(rest) = projects_in.strip_prefix('~') else {
        return Some(projects_in.to_owned());
    };

    if !rest.is_empty() && !rest.starts_with(['/', '\\']) {
        return Some(projects_in.to_owned());
    }

    let home = folders.home?;
    let rest = rest.trim_start_matches(['/', '\\']);

    if rest.is_empty() {
        return Some(home.to_owned());
    }

    Some(join(home, &[rest]))
}

/// `<projects root>/<org>/<repo>`; `None` without a home.
pub fn checkout_folder(folders: Folders<'_>, repo: &RepoRef) -> Option<String> {
    project_folder(folders, &[repo.org(), repo.name()])
}

/// `parts` under the machine's projects root; `None` without a home.
pub fn project_folder(folders: Folders<'_>, parts: &[&str]) -> Option<String> {
    Some(join(&projects_root(folders)?, parts))
}

/// `parts` under `base`, with the separator `base` uses.
pub fn join(base: &str, parts: &[&str]) -> String {
    let sep = if base.contains('\\') && !base.contains('/') {
        '\\'
    } else {
        '/'
    };
    let mut all = vec![base.trim_end_matches(sep)];
    all.extend_from_slice(parts);
    all.join(&sep.to_string())
}

/// What the chooser knows of one of the user's machines.
#[derive(Debug)]
pub struct Candidate<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub online: bool,
    /// Online and runs commands.
    pub shell: bool,
}

/// The machine to import onto, by index: `wanted`, else the only online one.
pub fn choose(candidates: &[Candidate<'_>], wanted: Option<&str>) -> Result<usize, ImportError> {
    if let Some(id) = wanted.map(str::trim)
        && !id.is_empty()
    {
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
