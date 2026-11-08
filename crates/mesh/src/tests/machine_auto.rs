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
