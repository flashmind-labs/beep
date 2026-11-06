//! Auto picks a session's machine: `needs` and the folder are required, prompt hints only steer.

use super::{Exec, agent_name, exec, sessions};
use crate::PeerClient;
use beep_proto::{AgentHost, ApiError, MachineCapability, StartAgentSession, node::agent::Status};
use futures::{StreamExt, stream};
use std::time::Duration;
use tokio::time::timeout;

/// How long one request to a node may take.
const NODE_LIMIT: Duration = Duration::from_secs(3);
/// Nodes asked at once.
const IN_FLIGHT: usize = 4;

/// Lowercase words of the prompt and folder that hint at a capability.
const CALLS_FOR: &[(MachineCapability, &[&str])] = &[
    (
        MachineCapability::Xcode {},
        &[
            "xcode",
            "xcodebuild",
            "ios",
            "iphone",
            "ipad",
            "swiftui",
            "testflight",
            "watchos",
            "visionos",
        ],
    ),
    (
        MachineCapability::Android {},
        &["android", "adb", "apk", "gradle", "avd"],
    ),
    (MachineCapability::Blender {}, &["blender"]),
    (MachineCapability::Unreal {}, &["unreal", "ue5", "uasset"]),
    (MachineCapability::Cuda {}, &["cuda", "nvcc"]),
    (
        MachineCapability::Gpu {},
        &["gpu", "vram", "finetune", "fine tune", "lora"],
    ),
    (MachineCapability::Docker {}, &["docker", "dockerfile"]),
];

/// The capabilities `text` calls for, in `CALLS_FOR` order.
pub fn called_for(text: &str) -> Vec<MachineCapability> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let pairs: Vec<String> = words
        .windows(2)
        .map(|pair| format!("{} {}", pair[0], pair[1]))
        .collect();
    CALLS_FOR
        .iter()
        .filter(|(_, calls)| {
            calls.iter().any(|call| {
                if call.contains(' ') {
                    pairs.iter().any(|pair| pair == call)
                } else {
                    words.contains(call)
                }
            })
        })
        .map(|(capability, _)| *capability)
        .collect()
}

/// What Auto knows of one online machine.
#[derive(Debug)]
pub struct Candidate<'a> {
    pub name: &'a str,
    /// It offers the agent asked for, or any agent when none was.
    pub offers: bool,
    pub capabilities: &'a [MachineCapability],
    /// The user's environment.
    pub workspace: bool,
    /// The folder asked for is there, or none was asked for.
    pub has_folder: bool,
    /// Sessions running on it now; only read to break a tie.
    pub running: u32,
}

impl Candidate<'_> {
    pub fn has(&self, needs: &[MachineCapability]) -> bool {
        needs.iter().all(|need| self.capabilities.contains(need))
    }
}

/// Why no machine fits a start.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum AutoError {
    #[error("None of your machines online offers {agent}.")]
    NoAgent { agent: String },
    #[error("No machine online with {agent} has {needs}. With {agent}: {offering}.")]
    Missing {
        agent: String,
        needs: String,
        offering: String,
    },
    #[error("There is no folder {folder} on {checked}.")]
    NoFolder { folder: String, checked: String },
}

/// What a start asks of its machine.
pub struct Wants<'a> {
    /// The agent's name for messages, such as "Claude Code".
    pub agent: &'a str,
    /// Asked for by the app: a machine must have all of them.
    pub needs: &'a [MachineCapability],
    /// Mentioned in the prompt or folder: preferred, not required.
    pub hints: &'a [MachineCapability],
    pub folder: Option<&'a str>,
}

/// The machines Auto picks among, by index, and the hints none of them met.
#[derive(Debug, PartialEq)]
pub struct Pool {
    pub at: Vec<usize>,
    pub missed: Vec<MachineCapability>,
}

/// The machines with the agent, every need and the folder, narrowed by hints when some match.
pub fn pool(candidates: &[Candidate<'_>], wants: &Wants<'_>) -> Result<Pool, AutoError> {
    let agent = wants.agent.to_owned();
    let names = |at: &[usize]| list(at.iter().map(|&at| candidates[at].name));
    let offering: Vec<usize> = (0..candidates.len())
        .filter(|&at| candidates[at].offers)
        .collect();
    if offering.is_empty() {
        return Err(AutoError::NoAgent { agent });
    }
    let able: Vec<usize> = offering
        .iter()
        .copied()
        .filter(|&at| candidates[at].has(wants.needs))
        .collect();
    if able.is_empty() {
        return Err(AutoError::Missing {
            agent,
            needs: list(wants.needs.iter().map(|need| need.label())),
            offering: names(&offering),
        });
    }
    let there: Vec<usize> = able
        .iter()
        .copied()
        .filter(|&at| candidates[at].has_folder)
        .collect();
    if there.is_empty() {
        return Err(AutoError::NoFolder {
            folder: wants.folder.unwrap_or_default().to_owned(),
            checked: names(&able),
        });
    }
    let (met, missed): (Vec<MachineCapability>, Vec<MachineCapability>) =
        wants.hints.iter().copied().partition(|hint| {
            there
                .iter()
                .any(|&at| candidates[at].capabilities.contains(hint))
        });
    let hinted: Vec<usize> = there
        .iter()
        .copied()
        .filter(|&at| candidates[at].has(&met))
        .collect();
    Ok(if hinted.is_empty() {
        // No one machine has every hint some machine has.
        Pool {
            at: there,
            missed: wants.hints.to_vec(),
        }
    } else {
        Pool { at: hinted, missed }
    })
}

/// Whether picking needs running-session counts: several fit and none is the user's environment.
pub fn needs_load(candidates: &[Candidate<'_>], pool: &Pool) -> bool {
    pool.at.len() > 1 && !pool.at.iter().any(|&at| candidates[at].workspace)
}

/// The machine Auto picks and why, said to the user.
#[derive(Debug, PartialEq)]
pub struct Choice {
    pub at: usize,
    pub why: String,
}

/// Picks the user's environment, else the least busy machine, then by name; `pool` is never empty.
pub fn choose(candidates: &[Candidate<'_>], pool: &Pool, wants: &Wants<'_>) -> Choice {
    let wanted: Vec<&str> = wants
        .needs
        .iter()
        .chain(
            wants
                .hints
                .iter()
                .filter(|hint| !pool.missed.contains(hint)),
        )
        .map(|capability| capability.label())
        .collect();
    let has = list(wanted.iter().copied());
    let missed = if pool.missed.is_empty() {
        String::new()
    } else {
        let missed = list(pool.missed.iter().map(|hint| hint.label()));
        format!(" No machine has {missed}, which the task mentions.")
    };
    let workspace = pool.at.iter().copied().find(|&at| candidates[at].workspace);
    let (at, why) = match workspace {
        Some(at) if has.is_empty() => (at, "It is your environment.".to_owned()),
        Some(at) => (at, format!("It has {has} and is your environment.")),
        None => {
            let at = pool
                .at
                .iter()
                .copied()
                .min_by(|&a, &b| {
                    let (a, b) = (&candidates[a], &candidates[b]);
                    a.running.cmp(&b.running).then_with(|| a.name.cmp(b.name))
                })
                .unwrap_or_default();
            let why = match (has.is_empty(), pool.at.len()) {
                (true, 1) => format!("It is the only machine with {}.", wants.agent),
                (true, _) => "It runs the fewest sessions.".to_owned(),
                (false, 1) => format!("It is the only one with {has}."),
                (false, _) => format!("It has {has} and runs the fewest sessions."),
            };
            (at, why)
        }
    };
    Choice {
        at,
        why: why + &missed,
    }
}

/// "a, b and c".
fn list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let items: Vec<&str> = items.collect();
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// A machine Auto may pick, with the way to ask it things.
pub struct AutoHost {
    pub host: AgentHost,
    pub client: PeerClient,
}

/// The machine Auto picks for `start`, by index in `hosts`, and why.
pub async fn pick(
    hosts: &[AutoHost],
    start: &StartAgentSession,
    prompt: &str,
) -> Result<Choice, ApiError> {
    let mut text = prompt.to_owned();
    if let Some(cwd) = &start.cwd {
        text.push('\n');
        text.push_str(cwd);
    }
    let hints: Vec<MachineCapability> = called_for(&text)
        .into_iter()
        .filter(|hint| !start.needs.contains(hint))
        .collect();
    let offers = |auto: &AutoHost| {
        start
            .agent
            .as_deref()
            .is_none_or(|agent| auto.host.agents.iter().any(|offered| offered == agent))
    };
    let folder = start.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty());
    let mut candidates: Vec<Candidate<'_>> = hosts
        .iter()
        .map(|auto| Candidate {
            name: &auto.host.machine_name,
            offers: offers(auto),
            capabilities: &auto.host.capabilities,
            workspace: auto.host.workspace.is_some(),
            has_folder: true,
            running: 0,
        })
        .collect();
    if let Some(folder) = folder {
        let asks: Vec<_> = (0..hosts.len())
            .filter(|&at| candidates[at].offers && candidates[at].has(&start.needs))
            .map(|at| has_folder(at, &hosts[at], folder))
            .collect();
        let found: Vec<(usize, bool)> = stream::iter(asks)
            .buffer_unordered(IN_FLIGHT)
            .collect()
            .await;
        for (at, there) in found {
            candidates[at].has_folder = there;
        }
    }
    let agent = start.agent.as_deref().map_or("a coding agent", agent_name);
    let wants = Wants {
        agent,
        needs: &start.needs,
        hints: &hints,
        folder,
    };
    let chosen = pool(&candidates, &wants).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if needs_load(&candidates, &chosen) {
        let asks: Vec<_> = chosen
            .at
            .iter()
            .map(|&at| counted(at, &hosts[at].client))
            .collect();
        let counts: Vec<(usize, u32)> = stream::iter(asks)
            .buffer_unordered(IN_FLIGHT)
            .collect()
            .await;
        for (at, count) in counts {
            candidates[at].running = count;
        }
    }
    Ok(choose(&candidates, &chosen, &wants))
}

/// Whether `folder` is on the machine. Unasked, a folder under another system's home root is missing.
async fn has_folder(at: usize, auto: &AutoHost, folder: &str) -> (usize, bool) {
    let home = auto.host.home.as_deref();
    let command = if home.is_some_and(windows_home) {
        Some(windows_folder_test(folder))
    } else {
        folder_test(folder)
    };
    let Some(command) = command else {
        return (at, plausible(folder, home));
    };
    let asked = timeout(NODE_LIMIT, exec(&auto.client, &command, NODE_LIMIT)).await;
    let there = match asked {
        Ok(Ok(Exec {
            exit_code: Some(code),
            ..
        })) => code == 0,
        Ok(Ok(_)) | Ok(Err(_)) | Err(_) => plausible(folder, home),
    };
    (at, there)
}

/// `test -d` for `folder`, with a leading `~` left to the node's shell.
pub fn folder_test(folder: &str) -> Option<String> {
    let quoted = |path: &str| {
        shlex::try_quote(path)
            .ok()
            .map(|quoted| quoted.into_owned())
    };
    let path = if folder == "~" {
        "\"$HOME\"".to_owned()
    } else if let Some(rest) = folder.strip_prefix("~/") {
        format!("\"$HOME\"/{}", quoted(rest)?)
    } else {
        quoted(folder)?
    };
    Some(format!("test -d {path}"))
}

/// Whether `home` is a Windows folder, whose node runs commands in PowerShell.
fn windows_home(home: &str) -> bool {
    home.as_bytes().get(1) == Some(&b':') || home.starts_with(r"\\")
}

/// The PowerShell form of [`folder_test`]: fails when `folder` is no folder. It sets the code
/// rather than calling `exit`, which would end the node's shell.
fn windows_folder_test(folder: &str) -> String {
    // Every quote that ends a single-quoted PowerShell string is doubled.
    let literal = |text: &str| {
        let mut quoted = String::from("'");
        for c in text.chars() {
            if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
                quoted.push(c);
            }
            quoted.push(c);
        }
        quoted.push('\'');
        quoted
    };
    let path = if folder == "~" {
        "$HOME".to_owned()
    } else if let Some(rest) = folder.strip_prefix("~/") {
        format!("(Join-Path $HOME {})", literal(rest))
    } else {
        literal(folder)
    };
    format!(
        "if (-not (Test-Path -LiteralPath {path} -PathType Container)) {{ $global:LASTEXITCODE = 1 }}"
    )
}

/// Whether `folder` could be on a machine whose home is `home`.
fn plausible(folder: &str, home: Option<&str>) -> bool {
    let Some(home) = home else {
        return true;
    };
    ["/Users/", "/home/"]
        .iter()
        .all(|root| !folder.starts_with(root) || home.starts_with(root.trim_end_matches('/')))
}

/// Sessions running on a machine; one that does not answer counts as busy.
async fn counted(at: usize, client: &PeerClient) -> (usize, u32) {
    let listed = timeout(NODE_LIMIT, sessions::list(client)).await;
    let running = match listed {
        Ok(Ok(sessions)) => {
            let live = sessions
                .iter()
                .filter(|info| info.state.status != Status::Ended)
                .count();
            u32::try_from(live).unwrap_or(u32::MAX)
        }
        _ => u32::MAX,
    };
    (at, running)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XCODE: MachineCapability = MachineCapability::Xcode {};
    const GPU: MachineCapability = MachineCapability::Gpu {};
    const CUDA: MachineCapability = MachineCapability::Cuda {};

    fn candidate<'a>(
        name: &'a str,
        capabilities: &'a [MachineCapability],
        workspace: bool,
        running: u32,
    ) -> Candidate<'a> {
        Candidate {
            name,
            offers: true,
            capabilities,
            workspace,
            has_folder: true,
            running,
        }
    }

    fn wants<'a>(needs: &'a [MachineCapability], hints: &'a [MachineCapability]) -> Wants<'a> {
        Wants {
            agent: "Codex",
            needs,
            hints,
            folder: None,
        }
    }

    fn choice(machines: &[Candidate<'_>], wants: &Wants<'_>) -> Result<Choice, AutoError> {
        let pool = pool(machines, wants)?;
        Ok(choose(machines, &pool, wants))
    }

    #[test]
    fn prompts_hint_at_what_they_name() {
        assert_eq!(called_for("Fix the iOS widget in Xcode"), [XCODE]);
        assert_eq!(called_for("Fine-tune the model with CUDA"), [CUDA, GPU]);
        assert_eq!(
            called_for("/srv/project/android-app"),
            [MachineCapability::Android {}]
        );
        assert_eq!(
            called_for("Open it in Blender"),
            [MachineCapability::Blender {}]
        );
        // "blend" is an ordinary word, not Blender.
        assert!(called_for("Blend the two colour themes").is_empty());
        // Part of a word is not the word.
        assert!(called_for("Fix the bios check and the gpuless path").is_empty());
        assert!(called_for("Fix the failing login test").is_empty());
    }

    #[test]
    fn auto_prefers_the_environment_then_the_least_busy() {
        let machines = [
            candidate("gpu-b", &[GPU], false, 2),
            candidate("gpu-a", &[GPU], false, 2),
            candidate("gpu-c", &[GPU], false, 1),
        ];
        let need = wants(&[GPU], &[]);
        let pool = pool(&machines, &need).unwrap();
        assert!(needs_load(&machines, &pool));
        assert_eq!(
            choose(&machines, &pool, &need),
            Choice {
                at: 2,
                why: "It has a GPU and runs the fewest sessions.".into()
            }
        );
        let tied = [
            candidate("gpu-b", &[GPU], false, 1),
            candidate("gpu-a", &[GPU], false, 1),
        ];
        assert_eq!(choice(&tied, &need).map(|c| c.at), Ok(1));
        let with_home = [
            candidate("mac", &[], false, 0),
            candidate("home", &[], true, 5),
        ];
        assert_eq!(
            choice(&with_home, &wants(&[], &[])),
            Ok(Choice {
                at: 1,
                why: "It is your environment.".into()
            })
        );
    }

    #[test]
    fn folder_tests_quote_the_path_and_keep_home() {
        assert_eq!(
            folder_test("/srv/my project").as_deref(),
            Some("test -d '/srv/my project'")
        );
        assert_eq!(
            folder_test("~/code/app").as_deref(),
            Some("test -d \"$HOME\"/code/app")
        );
        assert_eq!(folder_test("~").as_deref(), Some("test -d \"$HOME\""));
    }

    #[test]
    fn windows_folder_tests_run_in_powershell() {
        assert!(windows_home(r"C:\Users\me") && !windows_home("/Users/me"));
        assert_eq!(
            windows_folder_test(r"C:\it's"),
            r"if (-not (Test-Path -LiteralPath 'C:\it''s' -PathType Container)) { $global:LASTEXITCODE = 1 }"
        );
        assert!(windows_folder_test("~/code").contains("(Join-Path $HOME 'code')"));
    }

    #[test]
    fn the_folder_rules_out_machines_without_it() {
        let mut machines = [
            candidate("home", &[], true, 0),
            candidate("mac", &[], false, 0),
        ];
        machines[0].has_folder = false;
        let mut want = wants(&[], &[]);
        want.folder = Some("/Users/sam/app");
        assert_eq!(choice(&machines, &want).map(|c| c.at), Ok(1));
        machines[1].has_folder = false;
        assert_eq!(
            choice(&machines, &want),
            Err(AutoError::NoFolder {
                folder: "/Users/sam/app".into(),
                checked: "home and mac".into()
            })
        );
    }
}
