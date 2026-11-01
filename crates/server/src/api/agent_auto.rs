//! Auto: the machine a session started from the app runs on when the app
//! names none, from what the prompt calls for and what each machine can do.

use crate::protocol::MachineCapability;

/// Words that call for a capability, matched against whole words of the
/// prompt and folder, lowercase. Two-word phrases are matched joined by a
/// space.
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
    (MachineCapability::Blender {}, &["blender", "blend"]),
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

/// `needs` with `more` added, each once.
pub(super) fn with(
    mut needs: Vec<MachineCapability>,
    more: Vec<MachineCapability>,
) -> Vec<MachineCapability> {
    for need in more {
        if !needs.contains(&need) {
            needs.push(need);
        }
    }
    needs
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
    /// Sessions running on it now; only read to break a tie.
    pub(super) running: u32,
}

impl Candidate<'_> {
    fn fits(&self, needs: &[MachineCapability]) -> bool {
        self.offers && needs.iter().all(|need| self.capabilities.contains(need))
    }
}

/// The machines that offer the agent and have every capability `needs`
/// names, by index.
pub(super) fn fitting(candidates: &[Candidate<'_>], needs: &[MachineCapability]) -> Vec<usize> {
    (0..candidates.len())
        .filter(|&at| candidates[at].fits(needs))
        .collect()
}

/// Whether picking among `fit` needs each machine's running sessions: when
/// several fit and none is the user's environment.
pub(super) fn needs_load(candidates: &[Candidate<'_>], fit: &[usize]) -> bool {
    fit.len() > 1 && !fit.iter().any(|&at| candidates[at].workspace)
}

/// The machine Auto picks and why, said to the user.
#[derive(Debug, PartialEq)]
pub(super) struct Choice {
    pub(super) at: usize,
    pub(super) why: String,
}

/// Picks among `fit`: the user's environment, else the machine running the
/// fewest sessions, then by name. The error says why none fits.
pub(super) fn choose(
    candidates: &[Candidate<'_>],
    fit: &[usize],
    needs: &[MachineCapability],
    agent: &str,
) -> Result<Choice, String> {
    let has = list(needs.iter().map(|need| need.label()));
    let Some(&first) = fit.first() else {
        let offering: Vec<&str> = candidates
            .iter()
            .filter(|candidate| candidate.offers)
            .map(|candidate| candidate.name)
            .collect();
        return Err(if offering.is_empty() {
            format!("None of your machines online offers {agent}.")
        } else {
            format!(
                "No machine online with {agent} has {has}. With {agent}: {}.",
                offering.join(", ")
            )
        });
    };
    if let Some(&at) = fit.iter().find(|&&at| candidates[at].workspace) {
        let why = if needs.is_empty() {
            "It is your environment.".to_owned()
        } else {
            format!("It has {has} and is your environment.")
        };
        return Ok(Choice { at, why });
    }
    let at = fit
        .iter()
        .copied()
        .min_by(|&a, &b| {
            let (a, b) = (&candidates[a], &candidates[b]);
            a.running.cmp(&b.running).then_with(|| a.name.cmp(b.name))
        })
        .unwrap_or(first);
    let why = match (needs.is_empty(), fit.len()) {
        (true, 1) => format!("It is the only machine with {agent}."),
        (true, _) => "It runs the fewest sessions.".to_owned(),
        (false, 1) => format!("It is the only one with {has}."),
        (false, _) => format!("It has {has} and runs the fewest sessions."),
    };
    Ok(Choice { at, why })
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
            running,
        }
    }

    #[test]
    fn prompts_call_for_what_they_name() {
        assert_eq!(called_for("Fix the iOS widget in Xcode"), [XCODE]);
        assert_eq!(called_for("Fine-tune the model with CUDA"), [CUDA, GPU]);
        assert_eq!(
            called_for("/srv/project/android-app"),
            [MachineCapability::Android {}]
        );
        assert_eq!(
            called_for("Render scene.blend"),
            [MachineCapability::Blender {}]
        );
        // Part of a word is not the word.
        assert!(called_for("Fix the bios check and the gpuless path").is_empty());
        assert!(called_for("Fix the failing login test").is_empty());
    }

    #[test]
    fn needs_join_without_repeats() {
        assert_eq!(with(vec![XCODE], vec![GPU, XCODE]), [XCODE, GPU]);
    }

    #[test]
    fn auto_picks_the_only_machine_with_what_is_needed() {
        let machines = [
            candidate("home", &[], true, 0),
            candidate("studio", &[XCODE], false, 3),
        ];
        let fit = fitting(&machines, &[XCODE]);
        assert_eq!(fit, [1]);
        assert!(!needs_load(&machines, &fit));
        assert_eq!(
            choose(&machines, &fit, &[XCODE], "Claude Code"),
            Ok(Choice {
                at: 1,
                why: "It is the only one with Xcode.".into()
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
        let fit = fitting(&machines, &[GPU]);
        assert!(needs_load(&machines, &fit));
        assert_eq!(
            choose(&machines, &fit, &[GPU], "Codex"),
            Ok(Choice {
                at: 2,
                why: "It has a GPU and runs the fewest sessions.".into()
            })
        );
        let tied = [
            candidate("gpu-b", &[GPU], false, 1),
            candidate("gpu-a", &[GPU], false, 1),
        ];
        assert_eq!(choose(&tied, &[0, 1], &[GPU], "Codex").map(|c| c.at), Ok(1));
        let with_home = [
            candidate("mac", &[], false, 0),
            candidate("home", &[], true, 5),
        ];
        let fit = fitting(&with_home, &[]);
        assert!(!needs_load(&with_home, &fit));
        assert_eq!(
            choose(&with_home, &fit, &[], "Codex"),
            Ok(Choice {
                at: 1,
                why: "It is your environment.".into()
            })
        );
    }

    #[test]
    fn auto_says_why_nothing_fits() {
        let mut machines = [candidate("home", &[GPU], true, 0)];
        assert_eq!(
            choose(&machines, &[], &[XCODE, CUDA], "Claude Code"),
            Err(
                "No machine online with Claude Code has Xcode and CUDA. With Claude Code: home."
                    .into()
            )
        );
        machines[0].offers = false;
        assert_eq!(
            choose(&machines, &[], &[], "pi"),
            Err("None of your machines online offers pi.".into())
        );
    }
}
