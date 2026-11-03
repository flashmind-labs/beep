//! Auto picks a session's machine: `needs` and the folder are required, prompt hints only steer.

use crate::protocol::MachineCapability;

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
pub(super) fn called_for(text: &str) -> Vec<MachineCapability> {
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
pub(super) struct Candidate<'a> {
    pub(super) name: &'a str,
    /// It offers the agent asked for, or any agent when none was.
    pub(super) offers: bool,
    pub(super) capabilities: &'a [MachineCapability],
    /// The user's environment.
    pub(super) workspace: bool,
    /// The folder asked for is there, or none was asked for.
    pub(super) has_folder: bool,
    /// Sessions running on it now; only read to break a tie.
    pub(super) running: u32,
}

impl Candidate<'_> {
    pub(super) fn has(&self, needs: &[MachineCapability]) -> bool {
        needs.iter().all(|need| self.capabilities.contains(need))
    }
}

/// Why no machine fits a start.
#[derive(Debug, PartialEq, thiserror::Error)]
pub(super) enum AutoError {
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
pub(super) struct Wants<'a> {
    /// The agent's name for messages, such as "Claude Code".
    pub(super) agent: &'a str,
    /// Asked for by the app: a machine must have all of them.
    pub(super) needs: &'a [MachineCapability],
    /// Mentioned in the prompt or folder: preferred, not required.
    pub(super) hints: &'a [MachineCapability],
    pub(super) folder: Option<&'a str>,
}

/// The machines Auto picks among, by index, and the hints none of them met.
#[derive(Debug, PartialEq)]
pub(super) struct Pool {
    pub(super) at: Vec<usize>,
    pub(super) missed: Vec<MachineCapability>,
}

/// The machines with the agent, every need and the folder, narrowed by hints when some match.
pub(super) fn pool(candidates: &[Candidate<'_>], wants: &Wants<'_>) -> Result<Pool, AutoError> {
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
pub(super) fn needs_load(candidates: &[Candidate<'_>], pool: &Pool) -> bool {
    pool.at.len() > 1 && !pool.at.iter().any(|&at| candidates[at].workspace)
}

/// The machine Auto picks and why, said to the user.
#[derive(Debug, PartialEq)]
pub(super) struct Choice {
    pub(super) at: usize,
    pub(super) why: String,
}

/// Picks the user's environment, else the least busy machine, then by name; `pool` is never empty.
pub(super) fn choose(candidates: &[Candidate<'_>], pool: &Pool, wants: &Wants<'_>) -> Choice {
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

    fn pick(machines: &[Candidate<'_>], wants: &Wants<'_>) -> Result<Choice, AutoError> {
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
    fn a_hint_steers_to_the_machine_that_has_it() {
        let machines = [
            candidate("home", &[], true, 0),
            candidate("studio", &[XCODE], false, 3),
        ];
        let wants = wants(&[], &[XCODE]);
        let pool = pool(&machines, &wants).unwrap();
        assert_eq!(pool.at, [1]);
        assert!(!needs_load(&machines, &pool));
        assert_eq!(
            choose(&machines, &pool, &wants),
            Choice {
                at: 1,
                why: "It is the only one with Xcode.".into()
            }
        );
    }

    #[test]
    fn a_hint_no_machine_meets_falls_back_with_a_note() {
        let machines = [
            candidate("home", &[GPU], true, 0),
            candidate("mac", &[], false, 0),
        ];
        assert_eq!(
            pick(&machines, &wants(&[], &[XCODE, GPU])),
            Ok(Choice {
                at: 0,
                why: "It has a GPU and is your environment. No machine has Xcode, which the task mentions."
                    .into()
            })
        );
        assert_eq!(
            pick(&machines, &wants(&[], &[XCODE])),
            Ok(Choice {
                at: 0,
                why: "It is your environment. No machine has Xcode, which the task mentions."
                    .into()
            })
        );
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
        assert_eq!(pick(&tied, &need).map(|c| c.at), Ok(1));
        let with_home = [
            candidate("mac", &[], false, 0),
            candidate("home", &[], true, 5),
        ];
        assert_eq!(
            pick(&with_home, &wants(&[], &[])),
            Ok(Choice {
                at: 1,
                why: "It is your environment.".into()
            })
        );
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
        assert_eq!(pick(&machines, &want).map(|c| c.at), Ok(1));
        machines[1].has_folder = false;
        assert_eq!(
            pick(&machines, &want),
            Err(AutoError::NoFolder {
                folder: "/Users/sam/app".into(),
                checked: "home and mac".into()
            })
        );
    }

    #[test]
    fn auto_says_why_nothing_fits() {
        let mut machines = [candidate("home", &[GPU], true, 0)];
        let mut need = wants(&[XCODE, CUDA], &[]);
        need.agent = "Claude Code";
        assert_eq!(
            pick(&machines, &need).unwrap_err().to_string(),
            "No machine online with Claude Code has Xcode and CUDA. With Claude Code: home."
        );
        machines[0].offers = false;
        need.agent = "pi";
        assert_eq!(
            pick(&machines, &need),
            Err(AutoError::NoAgent { agent: "pi".into() })
        );
    }
}
