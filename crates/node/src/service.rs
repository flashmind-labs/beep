//! `beep-node service`: runs the node as a systemd user service, so it starts
//! with the machine and comes back when it stops.

use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const UNIT: &str = "beep-node.service";

pub fn install(dir: &Path) -> Result<()> {
    require_linux()?;
    let exe = env::current_exe().context("finding this binary")?;
    let dir = dir.canonicalize().with_context(|| {
        format!(
            "{} does not exist; run `beep-node link` first",
            dir.display()
        )
    })?;
    let path = unit_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let search = env::var("PATH").unwrap_or_default();
    fs::write(&path, unit(&exe, &dir, &search))
        .with_context(|| format!("writing {}", path.display()))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", UNIT])?;
    systemctl(&["restart", UNIT])?;
    println!("Installed {} and started it.", path.display());
    println!("Logs: journalctl --user -u beep-node -f");
    if !lingering() {
        println!();
        println!("It stops when you log out. To keep it running, run once:");
        println!("  sudo loginctl enable-linger $USER");
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    require_linux()?;
    let path = unit_path()?;
    if !path.exists() {
        println!("{} is not installed.", UNIT);
        return Ok(());
    }
    systemctl(&["disable", "--now", UNIT])?;
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    systemctl(&["daemon-reload"])?;
    println!("Stopped and removed {}.", path.display());
    Ok(())
}

fn require_linux() -> Result<()> {
    if env::consts::OS != "linux" {
        bail!("`beep-node service` sets up systemd, which only Linux has");
    }
    Ok(())
}

fn unit_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("this system has no config folder")?
        .join("systemd/user")
        .join(UNIT))
}

/// The unit file. It keeps the installing shell's `PATH`, since a user
/// service starts with a bare one and would not find tools like `claude` or
/// `gh` in the user's own folders.
fn unit(exe: &Path, dir: &Path, search: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Beep node\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         ExecStart={} --dir {} run\n\
         Restart=always\n\
         RestartSec=5\n\
         Environment={}\n\
         Environment=RUST_LOG=info\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        quote(&exe.display().to_string()),
        quote(&dir.display().to_string()),
        quote(&format!("PATH={search}")),
    )
}

/// A value quoted for a unit file: `%` starts a specifier there, so it is doubled.
fn quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

fn systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("running systemctl")?;
    if !status.success() {
        bail!("systemctl --user {} failed", args.join(" "));
    }
    Ok(())
}

/// Whether the user's services keep running after they log out.
fn lingering() -> bool {
    let Ok(user) = env::var("USER") else {
        return false;
    };
    Command::new("loginctl")
        .args(["show-user", &user, "--property=Linger", "--value"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unit_runs_the_node_with_its_folder_and_path() {
        let unit = unit(
            Path::new("/opt/beep node/beep-node"),
            Path::new("/srv/node"),
            "/usr/bin:/home/user/50%/bin",
        );
        assert!(unit.contains("ExecStart=\"/opt/beep node/beep-node\" --dir \"/srv/node\" run\n"));
        assert!(unit.contains("Environment=\"PATH=/usr/bin:/home/user/50%%/bin\"\n"));
        assert!(unit.contains("WantedBy=default.target"));
    }
}
