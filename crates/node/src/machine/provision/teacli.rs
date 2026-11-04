//! teacli's config: `configure` writes the node's own file with Beep's Forgejo
//! servers and the user's, plus a shim first on `PATH` that points teacli at
//! it.

use crate::files::fsat;
use anyhow::{Context, Result};
use beep_proto::node::ForgejoLogin;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// The shim's file name: a batch file on Windows, which finds it by its
/// extension.
const SHIM: &str = if cfg!(windows) {
    "teacli.cmd"
} else {
    "teacli"
};

/// The node's teacli config, under its own folder.
pub(super) fn config_path(dir: &Path) -> PathBuf {
    dir.join("teacli").join("teacli.ini")
}

/// The folder the shim lives in, first on `PATH` while it exists.
pub(super) fn shims_dir(dir: &Path) -> PathBuf {
    dir.join("shims")
}

/// The user's own teacli config.
pub(super) fn user_config(home: &Path) -> PathBuf {
    home.join(".config/teacli/teacli.ini")
}

/// Writes the config and the shim for `logins`, run by the teacli at
/// `teacli`; with no logins or no teacli, removes both so teacli reads the
/// user's config as usual. Gives whether the shim is in place. teacli takes
/// tokens from no env var, so the config holds them, readable by the owner
/// only.
pub(super) fn configure(
    dir: &Path,
    logins: &[ForgejoLogin],
    user_ini: Option<&str>,
    teacli: Option<&Path>,
) -> Result<bool> {
    let config = config_path(dir);
    let shim = shims_dir(dir).join(SHIM);
    let Some(teacli) = teacli.filter(|_| !logins.is_empty()) else {
        remove(&shim)?;
        remove(&config)?;
        return Ok(false);
    };
    write_private(&config, render(logins, user_ini).as_bytes())?;
    let script = if cfg!(windows) {
        batch_shim(teacli, &config)
    } else {
        sh_shim(teacli, &config, &shims_dir(dir))
    };
    let shims = shims_dir(dir);
    fs::create_dir_all(&shims).with_context(|| format!("creating {}", shims.display()))?;
    let partial = shims.join(".teacli.beep");
    fs::write(&partial, script)?;
    fsat::set_mode(&partial, 0o755)?;
    fs::rename(&partial, &shim).with_context(|| format!("writing {}", shim.display()))?;
    Ok(true)
}

/// One `[name]` section of an ini file.
struct Section {
    name: String,
    /// Its lines after the header, as they were.
    lines: Vec<String>,
    url: Option<String>,
}

/// The sections of `text` after its `[global]` one, and its default server.
fn parse(text: &str) -> (Vec<Section>, Option<String>) {
    let mut sections: Vec<Section> = Vec::new();
    let mut default = None;
    let mut in_global = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_global = name.trim() == "global";
            if !in_global {
                sections.push(Section {
                    name: name.trim().to_owned(),
                    lines: Vec::new(),
                    url: None,
                });
            }
            continue;
        }
        let value = |key: &str| {
            let (k, v) = trimmed.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_owned())
        };
        if in_global {
            if let Some(name) = value("default_server") {
                default = Some(name);
            }
        } else if let Some(section) = sections.last_mut() {
            if let Some(url) = value("url") {
                section.url = Some(url.trim_end_matches('/').to_owned());
            }
            section.lines.push(line.to_owned());
        }
    }
    (sections, default)
}

/// The config: Beep's servers, each named after its host or after the
/// user's section for the same URL, then the user's other servers.
fn render(logins: &[ForgejoLogin], user_ini: Option<&str>) -> String {
    let (mut theirs, default) = user_ini.map(parse).unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    let mut out = String::new();
    for login in logins {
        let url = login.url.trim_end_matches('/');
        let same = theirs
            .iter()
            .position(|section| section.url.as_deref() == Some(url));
        let mut name = match same {
            Some(at) => theirs.remove(at).name,
            None => host_of(url),
        };
        while names.contains(&name) || theirs.iter().any(|s| s.name == name) {
            name.push_str("-beep");
        }
        out.push_str(&format!(
            "\n[{name}]\nurl = {url}\ntoken = {}\n",
            login.token.expose()
        ));
        names.push(name);
    }
    for section in &theirs {
        out.push_str(&format!("\n[{}]\n", section.name));
        for line in &section.lines {
            out.push_str(line);
            out.push('\n');
        }
        names.push(section.name.clone());
    }
    let default = default
        .filter(|name| names.contains(name))
        .or_else(|| names.first().cloned());
    let mut head = String::from(
        "# Written by beep-node from the Forgejo servers in Beep's settings and\n\
         # ~/.config/teacli/teacli.ini. It is rewritten when either changes.\n",
    );
    if let Some(default) = default {
        head.push_str(&format!("[global]\ndefault_server = {default}\n"));
    }
    head + &out
}

/// `git.example.com` from `https://git.example.com:3000/forge`, with the
/// port when there is one.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or(rest);
    host.replace(':', "-")
}

/// The shim as a sh script.
fn sh_shim(teacli: &Path, config: &Path, shims: &Path) -> String {
    // A teacli moved since (say to ~/.local/bin) is looked up on PATH,
    // past this folder.
    format!(
        "#!/bin/sh\n# Written by beep-node: teacli with the Forgejo servers from Beep.\n\
         teacli={}\n\
         if [ ! -x \"$teacli\" ]; then\n\
         \x20 teacli=$(IFS=:; for d in $PATH; do [ \"$d\" = {} ] || [ ! -x \"$d/teacli\" ] || {{ echo \"$d/teacli\"; break; }}; done)\n\
         \x20 [ -n \"$teacli\" ] || {{ echo 'teacli: not found' >&2; exit 127; }}\n\
         fi\n\
         exec \"$teacli\" --config {} \"$@\"\n",
        quote(&teacli.to_string_lossy()),
        quote(&shims.to_string_lossy()),
        quote(&config.to_string_lossy()),
    )
}

/// The shim as a Windows batch file. A Windows path holds no `"`, and `%`
/// is doubled so the batch file does not expand it.
fn batch_shim(teacli: &Path, config: &Path) -> String {
    let quote = |path: &Path| format!("\"{}\"", path.to_string_lossy().replace('%', "%%"));
    format!(
        "@echo off\r\nrem Written by beep-node: teacli with the Forgejo servers from Beep.\r\n\
         {} --config {} %*\r\n",
        quote(teacli),
        quote(config),
    )
}

/// `text` in single quotes for sh.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Writes `bytes` to `path` so only its owner can read it, in a folder only
/// the owner can open.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("the config has a folder")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fsat::set_mode(parent, 0o700)?;
    let partial = parent.join(".teacli.ini.beep");
    remove(&partial)?;
    let mut file = fsat::private(OpenOptions::new().write(true).create_new(true))
        .open(&partial)
        .with_context(|| format!("creating {}", partial.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&partial, path).with_context(|| format!("writing {}", path.display()))
}

fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::SecretText;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    fn login(url: &str, token: &str) -> ForgejoLogin {
        ForgejoLogin {
            url: url.into(),
            token: SecretText::new(token),
        }
    }

    #[test]
    fn beeps_servers_join_the_users_own() {
        let user = "[global]\ndefault_server = work\n\n[work]\nurl = https://git.example.com/\ntoken = old\n\n\
                    [home]\n# mine\nurl = http://code.example.org\ntoken = mine\n";
        let text = render(
            &[
                login("https://git.example.com", "fresh"),
                login("https://forge.example.net:3000", "other"),
            ],
            Some(user),
        );
        // The user's section for the same URL keeps its name, with Beep's token.
        assert!(
            text.contains("[work]\nurl = https://git.example.com\ntoken = fresh\n"),
            "{text}"
        );
        assert!(!text.contains("token = old"), "{text}");
        assert!(
            text.contains(
                "[forge.example.net-3000]\nurl = https://forge.example.net:3000\ntoken = other\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("[home]\n# mine\nurl = http://code.example.org\ntoken = mine\n"),
            "{text}"
        );
        assert!(text.contains("[global]\ndefault_server = work\n"), "{text}");
    }

    #[test]
    fn the_config_is_private_and_goes_with_the_last_login() {
        let dir = std::env::temp_dir().join(format!("beep-node-teacli-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let teacli = Path::new("/opt/it's/teacli");
        assert!(
            configure(
                &dir,
                &[login("https://git.example.com", "t")],
                None,
                Some(teacli)
            )
            .unwrap()
        );
        let config = config_path(&dir);
        #[cfg(unix)]
        assert_eq!(fs::metadata(&config).unwrap().mode() & 0o777, 0o600);
        let shim = fs::read_to_string(shims_dir(&dir).join(SHIM)).unwrap();
        if cfg!(windows) {
            assert!(shim.contains("\"/opt/it's/teacli\" --config \""), "{shim}");
        } else {
            assert!(
                shim.contains("teacli='/opt/it'\\''s/teacli'") && shim.contains("--config '"),
                "{shim}"
            );
        }

        assert!(!configure(&dir, &[], None, Some(teacli)).unwrap());
        assert!(!config.exists());
        assert!(!shims_dir(&dir).join(SHIM).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
