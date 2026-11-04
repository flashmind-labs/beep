//! `ssh/1` (see `beep_proto::node::ssh`): adds another machine over this
//! machine's own `ssh`, with the user's keys and config. It never prompts:
//! without key access it fails with ssh's own message.

use crate::link::{Protocol, Session};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::node::{
    LinkRx, LinkTx,
    ssh::{Event, PROTOCOL, Request},
};
use std::{ffi::OsString, net::IpAddr, path::Path, process::Stdio, time::Duration};
use tokio::{fs, io::AsyncWriteExt, process::Command, time::timeout};

const INSTALL_SH: &str = "curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh";
const INSTALL_PS1: &str = "irm https://releases.flashmind.com/beep/node/install.ps1 | iex";
/// Reads the token, then the search folder, from stdin, so the token is in
/// no command line and the folder needs no quoting. install.sh puts the
/// binary in ~/.local/bin, which a non-login shell may not have on its PATH.
const LINK_SH: &str = r#"sh -c 'read -r t && read -r s; set -- setup --no-run "$t"; [ -n "$s" ] && set -- "$@" --search "$s"; "$HOME/.local/bin/beep-node" "$@" && "$HOME/.local/bin/beep-node" service install'"#;
const LINK_PS1: &str = r"$t = [Console]::In.ReadLine()
$s = [Console]::In.ReadLine()
$b = Join-Path $env:LOCALAPPDATA 'Programs\beep-node\beep-node.exe'
$a = @('setup', '--no-run', $t)
if ($s) { $a += @('--search', $s) }
& $b @a
if ($LASTEXITCODE) { exit $LASTEXITCODE }
& $b service install
exit $LASTEXITCODE";
const SYSTEM_PS1: &str = r#"Write-Output "Windows $env:PROCESSOR_ARCHITECTURE""#;

const DETECT_TIMEOUT: Duration = Duration::from_secs(30);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const LINK_TIMEOUT: Duration = Duration::from_secs(2 * 60);
/// How much of a failing command's output is sent back.
const TAIL_LINES: usize = 20;
/// The exit code ssh itself fails with, as when it cannot connect.
const SSH_FAILED: i32 = 255;

/// Serves `ssh/1` with the `ssh` on PATH.
pub struct Ssh {
    program: OsString,
}

impl Default for Ssh {
    fn default() -> Self {
        Self {
            program: "ssh".into(),
        }
    }
}

impl Protocol for Ssh {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(self.program.clone(), tx, rx))
    }
}

async fn serve(program: OsString, mut tx: LinkTx, mut rx: LinkRx) -> anyhow::Result<()> {
    let event = match rx.recv::<Request>().await? {
        Some(Request::Hosts {}) => Event::Hosts {
            hosts: match dirs::home_dir() {
                Some(home) => known_hosts(&home.join(".ssh")).await,
                None => Vec::new(),
            },
        },
        Some(Request::Add {
            target,
            token,
            search,
        }) => add(&program, &target, token.expose(), &search).await,
        _ => return Ok(()),
    };
    tx.send(&event).await?;
    tx.finish().await
}

/// `Host` names in `config` without patterns, then unhashed host names in
/// `known_hosts`, each once.
async fn known_hosts(dir: &Path) -> Vec<String> {
    let config = fs::read_to_string(dir.join("config"))
        .await
        .unwrap_or_default();
    let known = fs::read_to_string(dir.join("known_hosts"))
        .await
        .unwrap_or_default();
    let from_config = config.lines().flat_map(|line| {
        let line = line.trim();
        let (key, rest) = line
            .split_once(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or((line, ""));
        let names = if key.eq_ignore_ascii_case("host") {
            rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=')
        } else {
            ""
        };
        names
            .split_whitespace()
            .filter(|name| !name.contains(['*', '?', '!']))
    });
    let from_known = known.lines().flat_map(|line| {
        let mut fields = line.split_whitespace();
        let first = fields.next().unwrap_or("");
        // A marker such as @cert-authority comes before the names.
        let names = if first.starts_with('@') {
            fields.next().unwrap_or("")
        } else {
            first
        };
        names.split(',').filter(|name| {
            !name.is_empty()
                && !name.starts_with(['|', '#', '['])
                && !name.contains(['*', '?'])
                && name.parse::<IpAddr>().is_err()
        })
    });
    let mut hosts: Vec<String> = Vec::new();
    for name in from_config.chain(from_known) {
        if !hosts.iter().any(|host| host == name) {
            hosts.push(name.to_owned());
        }
    }
    hosts
}

/// The system on the other end, as detection read it.
enum System {
    Unix { text: String },
    Windows { text: String },
}

impl System {
    fn text(&self) -> &str {
        match self {
            System::Unix { text } | System::Windows { text } => text,
        }
    }
}

/// One step's command.
struct Remote<'a> {
    command: String,
    stdin: Option<&'a str>,
    limit: Duration,
}

/// What a command printed and how it ended.
struct Ran {
    code: Option<i32>,
    output: String,
}

impl Ran {
    fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// Why `add` stopped.
struct Failure {
    reason: String,
    output: String,
}

async fn add(program: &OsString, target: &str, token: &str, search: &str) -> Event {
    let mut system = String::new();
    // One line each on stdin.
    let input = format!("{token}\n{}", search.replace(['\r', '\n'], ""));
    match steps(program, target, &input, &mut system).await {
        Ok(()) => Event::Added { system },
        Err(Failure { reason, output }) => {
            log::info!("adding a machine over ssh failed; target={target} reason={reason}");
            Event::Failed {
                reason,
                system,
                output: tail(&output.replace(token, "<token>")),
            }
        }
    }
}

/// `input` goes to the link step's stdin: the token, then the search folder.
async fn steps(
    program: &OsString,
    target: &str,
    input: &str,
    system: &mut String,
) -> Result<(), Failure> {
    if target.is_empty() || target.starts_with('-') || target.contains(char::is_whitespace) {
        return Err(Failure {
            reason: format!("{target:?} is not an ssh target such as user@host."),
            output: String::new(),
        });
    }
    let ssh = Target { program, target };
    let found = detect(&ssh).await?;
    *system = found.text().to_owned();
    let (install, link) = match &found {
        System::Unix { text } => {
            if text.starts_with("Darwin") && !text.ends_with("arm64") {
                return Err(Failure {
                    reason: "beep-node runs on Macs with Apple silicon; there is no build for \
                             Intel Macs."
                        .into(),
                    output: String::new(),
                });
            }
            (INSTALL_SH.to_owned(), LINK_SH.to_owned())
        }
        System::Windows { .. } => (powershell(INSTALL_PS1), powershell(LINK_PS1)),
    };
    let installed = ssh
        .run(Remote {
            command: install,
            stdin: None,
            limit: INSTALL_TIMEOUT,
        })
        .await?;
    if !installed.ok() {
        return Err(failed("Installing beep-node", target, installed));
    }
    let linked = ssh
        .run(Remote {
            command: link,
            stdin: Some(input),
            limit: LINK_TIMEOUT,
        })
        .await?;
    if !linked.ok() {
        return Err(failed("Linking", target, linked));
    }
    Ok(())
}

/// `uname -sm`, else PowerShell for Windows, where `uname` is missing or
/// comes from a Unix layer such as Git Bash.
async fn detect(ssh: &Target<'_>) -> Result<System, Failure> {
    let target = ssh.target;
    let uname = ssh
        .run(Remote {
            command: "uname -sm".into(),
            stdin: None,
            limit: DETECT_TIMEOUT,
        })
        .await?;
    if uname.code == Some(SSH_FAILED) {
        return Err(failed("Connecting", target, uname));
    }
    let text = last_line(&uname.output);
    let windowsish = ["MINGW", "MSYS", "CYGWIN", "Windows"]
        .iter()
        .any(|prefix| text.starts_with(prefix));
    if uname.ok() && !text.is_empty() && !windowsish {
        return Ok(System::Unix { text });
    }
    let ps = ssh
        .run(Remote {
            command: powershell(SYSTEM_PS1),
            stdin: None,
            limit: DETECT_TIMEOUT,
        })
        .await?;
    let text = last_line(&ps.output);
    if ps.ok() && text.starts_with("Windows") {
        return Ok(System::Windows { text });
    }
    Err(Failure {
        reason: format!("Could not tell what system {target} runs: uname and PowerShell failed."),
        output: format!("{}\n{}", uname.output, ps.output),
    })
}

/// Runs `script` in Windows PowerShell, encoded so neither cmd.exe nor
/// PowerShell as the login shell reads into it.
fn powershell(script: &str) -> String {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(utf16)
    )
}

/// ssh to one target.
struct Target<'a> {
    program: &'a OsString,
    target: &'a str,
}

impl Target<'_> {
    async fn run(&self, remote: Remote<'_>) -> Result<Ran, Failure> {
        run(self.program, self.target, remote).await
    }
}

async fn run(program: &OsString, target: &str, remote: Remote<'_>) -> Result<Ran, Failure> {
    let mut command = Command::new(program);
    command
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ])
        .arg(target)
        .arg(&remote.command)
        .stdin(if remote.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| Failure {
        reason: format!("Could not run ssh: {e}"),
        output: String::new(),
    })?;
    if let (Some(text), Some(mut stdin)) = (remote.stdin, child.stdin.take()) {
        // A remote that exits without reading closes the pipe; its exit
        // code says what went wrong.
        if let Err(e) = stdin.write_all(format!("{text}\n").as_bytes()).await {
            log::debug!("ssh stdin closed early: {e}");
        }
    }
    let done = timeout(remote.limit, child.wait_with_output())
        .await
        .map_err(|_| Failure {
            reason: format!(
                "{target} did not finish `{}` within {} seconds.",
                remote.command.split_whitespace().next().unwrap_or("ssh"),
                remote.limit.as_secs()
            ),
            output: String::new(),
        })?
        .map_err(|e| Failure {
            reason: format!("ssh failed: {e}"),
            output: String::new(),
        })?;
    let mut output = String::from_utf8_lossy(&done.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&done.stderr));
    Ok(Ran {
        code: done.status.code(),
        output,
    })
}

fn failed(step: &str, target: &str, ran: Ran) -> Failure {
    let why = match ran.code {
        Some(SSH_FAILED) => format!("ssh could not reach {target}"),
        Some(code) => format!("exited with {code}"),
        None => "was stopped".to_owned(),
    };
    Failure {
        reason: format!("{step} failed: {why}."),
        output: ran.output,
    }
}

fn last_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("")
        .to_owned()
}

fn tail(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let skip = lines.len().saturating_sub(TAIL_LINES);
    lines[skip..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const TOKEN: &str = "bk1-test-token";

    /// A stand-in for ssh: logs each call, decodes PowerShell, and plays a
    /// Linux box, a Windows box, an Intel Mac, an unreachable host and a
    /// Linux box whose setup fails.
    const FAKE_SSH: &str = r#"#!/bin/sh
target=$8; cmd=$9
printf '%s|%s\n' "$target" "$cmd" >> "$(dirname "$0")/calls"
case "$cmd" in powershell*) cmd=$(printf %s "${cmd##* }" | base64 -d | tr -d '\000');; esac
case "$target:$cmd" in
  down-box:*) echo "ssh: connect to host down-box port 22: Connection refused" >&2; exit 255;;
  old-mac:uname*) echo "Darwin x86_64";;
  *box:uname*) echo "Linux x86_64";;
  win:uname*) echo "'uname' is not recognized as an internal or external command" >&2; exit 1;;
  *PROCESSOR_ARCHITECTURE*) echo "Windows AMD64";;
  *install.sh*|*install.ps1*) echo "Installed beep-node";;
  *setup*) read -r t; read -r s; printf 'search=%s\n' "$s" >> "$(dirname "$0")/calls"; echo "claiming $t"; [ "$target" = bad-box ] && { echo "token refused" >&2; exit 1; }; [ "$t" = bk1-test-token ] || exit 3;;
  *) exit 9;;
esac
"#;

    #[tokio::test]
    async fn adds_machines_over_a_fake_ssh() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let ssh = dir.path().join("ssh");
        std::fs::write(&ssh, FAKE_SSH).expect("the fake ssh writes");
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755))
            .expect("the fake ssh is executable");
        let program = ssh.into_os_string();

        let linux = add(&program, "user@linux-box", TOKEN, "~/My Projects").await;
        assert_eq!(
            linux,
            Event::Added {
                system: "Linux x86_64".into()
            }
        );
        let windows = add(&program, "win", TOKEN, "").await;
        assert_eq!(
            windows,
            Event::Added {
                system: "Windows AMD64".into()
            }
        );

        let Event::Failed { reason, output, .. } = add(&program, "down-box", TOKEN, "").await
        else {
            panic!("an unreachable host fails");
        };
        assert!(reason.starts_with("Connecting failed"), "{reason}");
        assert!(output[0].contains("Connection refused"), "{output:?}");

        let Event::Failed { reason, system, .. } = add(&program, "old-mac", TOKEN, "").await else {
            panic!("an Intel Mac is refused");
        };
        assert!(reason.contains("Intel"), "{reason}");
        assert_eq!(system, "Darwin x86_64");

        let Event::Failed { reason, output, .. } = add(&program, "bad-box", TOKEN, "").await else {
            panic!("a failed setup fails");
        };
        assert!(reason.starts_with("Linking failed"), "{reason}");
        assert_eq!(output, ["claiming <token>", "token refused"]);

        let calls = std::fs::read_to_string(dir.path().join("calls")).expect("calls logged");
        assert!(!calls.contains(TOKEN), "the token went on a command line");
        assert!(calls.contains("user@linux-box|uname -sm"));
        assert!(calls.contains("user@linux-box|curl -fsSL"));
        assert!(calls.contains("search=~/My Projects\n"));
        assert!(calls.contains("win|powershell -NoProfile"));
        // Nothing past the refusal ran on the Intel Mac.
        assert_eq!(calls.matches("old-mac|").count(), 1);
    }

    #[tokio::test]
    async fn lists_named_hosts_only() {
        let dir = tempfile::tempdir().expect("a temp dir");
        std::fs::write(
            dir.path().join("config"),
            "Host build-box gpu-box\n  HostName 10.0.0.2\nHost *.example.com !skip\nhost=pi-lab\nMatch all\n",
        )
        .expect("config writes");
        std::fs::write(
            dir.path().join("known_hosts"),
            "gpu-box,10.0.0.3 ssh-ed25519 AAAA\n|1|abc=|def= ssh-ed25519 AAAA\n[nas]:2222 ssh-rsa AAAA\n@cert-authority *.example.com ssh-rsa AAAA\nnas.example.com ssh-rsa AAAA\n",
        )
        .expect("known_hosts writes");
        assert_eq!(
            known_hosts(dir.path()).await,
            ["build-box", "gpu-box", "pi-lab", "nas.example.com"]
        );
    }
}
