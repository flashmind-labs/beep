use super::*;
use tempfile::TempDir;

fn write(at: &Path, rel: &str, text: &str) {
    let path = at.join(rel);
    fs::create_dir_all(path.parent().expect("a file has a folder")).expect("folder made");
    fs::write(path, text).expect("file written");
}

struct Setup {
    _dir: TempDir,
    project: PathBuf,
    home: PathBuf,
}

fn setup() -> Setup {
    let dir = TempDir::new().expect("a temp folder can be made");
    let project = dir.path().join("work/widget");
    let home = dir.path().join("home");
    fs::create_dir_all(&project).expect("project made");
    fs::create_dir_all(&home).expect("home made");
    Setup {
        project,
        home,
        _dir: dir,
    }
}

fn names(agents: &[CustomAgent]) -> Vec<(&str, &str)> {
    agents
        .iter()
        .map(|a| (a.harness.as_ref(), a.name.as_str()))
        .collect()
}

#[test]
fn reads_every_harness_in_the_project_then_the_users() {
    let s = setup();
    let p = Path::new(&s.project);
    write(
        p,
        ".claude/agents/reviewer.md",
        "---\nname: reviewer\ndescription: Reviews\ncolor: red\n---\nReview.",
    );
    write(
        p,
        ".opencode/agent/docs.md",
        "---\ndescription: Docs\n---\nWrite.",
    );
    write(
        p,
        "opencode.json",
        r#"{"agent": {"planner": {"description": "Plans", "prompt": "Plan."}}}"#,
    );
    write(
        p,
        ".codex/agents/explorer.toml",
        "name = \"explorer\"\ndescription = \"Reads\"\ndeveloper_instructions = \"Read.\"",
    );
    write(
        p,
        ".pi/agents/scout.md",
        "---\nname: scout\ndescription: Scouts\n---\nLook.",
    );
    write(
        p,
        ".agents/helper.md",
        "---\nname: helper\ndescription: Helps\n---\nHelp.",
    );
    write(
        &s.home,
        ".claude/agents/mine.md",
        "---\nname: mine\ndescription: Mine\n---\nMe.",
    );
    write(
        &s.home,
        ".codex/config.toml",
        "[agents]\nmax_threads = 2\n[agents.tester]\ndescription = \"Tests\"\n\
         config_file = \"roles/tester.toml\"\n",
    );
    write(
        &s.home,
        ".codex/roles/tester.toml",
        "name = \"tester\"\nmodel = \"model-b\"\ndeveloper_instructions = \"Test it.\"",
    );

    let agents = list(&s.project, Some(&s.home)).unwrap();
    assert_eq!(
        names(&agents),
        [
            ("claude", "reviewer"),
            ("opencode", "docs"),
            ("opencode", "planner"),
            ("codex", "explorer"),
            ("pi", "scout"),
            ("flsh", "helper"),
            ("claude", "mine"),
            ("codex", "tester"),
        ]
    );
    let reviewer = &agents[0];
    assert_eq!(reviewer.scope, CustomAgentScope::Project {});
    assert_eq!(reviewer.path, ".claude/agents/reviewer.md");
    assert_eq!(reviewer.color.as_deref(), Some("red"));
    assert_eq!(reviewer.prompt, "Review.");
    let mine = &agents[6];
    assert_eq!(mine.scope, CustomAgentScope::User {});
    assert_eq!(mine.path, "~/.claude/agents/mine.md");
    let tester = &agents[7];
    assert_eq!(tester.path, "~/.codex/config.toml");
    assert_eq!(tester.model.as_deref(), Some("model-b"));
    assert_eq!(tester.prompt, "Test it.");
    assert_eq!(tester.error, None);
}

#[test]
fn the_instructions_in_a_shared_agents_folder_are_no_agent() {
    let s = setup();
    write(&s.home, ".agents/AGENTS.md", "# Global instructions");
    write(
        &s.home,
        ".agents/helper.md",
        "---\nname: helper\n---\nHelp.",
    );
    write(
        Path::new(&s.project),
        ".claude/agents/plain.md",
        "No block.",
    );
    let agents = list(&s.project, Some(&s.home)).unwrap();
    let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"helper"), "{names:?}");
    assert!(!names.contains(&"AGENTS"), "{names:?}");
    // A harness's own folder still says a definition is broken.
    assert!(names.contains(&"plain"), "{names:?}");
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_folder_is_not_read() {
    let s = setup();
    let p = Path::new(&s.project);
    write(&s.home, "secret.md", "---\nname: secret\n---\nPrivate.");
    fs::create_dir_all(p.join(".claude/agents")).unwrap();
    std::os::unix::fs::symlink(s.home.join("secret.md"), p.join(".claude/agents/out.md")).unwrap();
    let config = s.home.join(".codex");
    write(
        &config,
        "config.toml",
        "[agents.x]\ndescription = \"X\"\nconfig_file = \"../secret.md\"\n",
    );
    let agents = list(&s.project, Some(&s.home)).unwrap();
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].name, "out");
    assert!(agents[0].error.as_deref().unwrap().contains("outside"));
    assert!(agents[0].prompt.is_empty());
    assert_eq!(agents[1].name, "x");
    assert!(agents[1].error.as_deref().unwrap().contains("outside"));
    assert!(agents[1].prompt.is_empty());
}

#[test]
fn the_home_as_project_lists_each_file_once() {
    let s = setup();
    write(&s.home, ".claude/agents/a.md", "---\nname: a\n---\nA.");
    let agents = list(&s.home, Some(&s.home)).unwrap();
    assert_eq!(names(&agents), [("claude", "a")]);
    assert_eq!(agents[0].scope, CustomAgentScope::Project {});
}

#[cfg(unix)]
#[test]
fn a_symlink_in_a_home_agents_folder_pointing_outside_is_refused() {
    let s = setup();
    let outside = TempDir::new().unwrap();
    write(
        outside.path(),
        "secret.md",
        "---\nname: secret\n---\nPrivate.",
    );
    write(
        &s.home,
        ".claude/agents/mine.md",
        "---\nname: mine\n---\nMe.",
    );
    std::os::unix::fs::symlink(
        outside.path().join("secret.md"),
        s.home.join(".claude/agents/out.md"),
    )
    .unwrap();
    let agents = list(&s.project, Some(&s.home)).unwrap();
    assert_eq!(names(&agents), [("claude", "mine"), ("claude", "out")]);
    assert_eq!(agents[0].prompt, "Me.");
    let out = &agents[1];
    assert_eq!(out.scope, CustomAgentScope::User {});
    assert!(out.error.as_deref().unwrap().contains("outside"));
    assert!(out.prompt.is_empty());
}

#[cfg(unix)]
#[test]
fn a_home_agents_folder_linked_outside_is_not_listed() {
    let s = setup();
    let outside = TempDir::new().unwrap();
    write(
        outside.path(),
        "agents/secret.md",
        "---\nname: secret\n---\nPrivate.",
    );
    // The definitions folder inside the harness folder links out.
    fs::create_dir_all(s.home.join(".claude")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("agents"), s.home.join(".claude/agents"))
        .unwrap();
    // The harness folder itself links out of the home.
    std::os::unix::fs::symlink(outside.path(), s.home.join(".agents")).unwrap();
    let agents = list(&s.project, Some(&s.home)).unwrap();
    assert_eq!(agents.len(), 1, "{agents:?}");
    assert_eq!(agents[0].path, "~/.claude/agents");
    assert!(agents[0].error.as_deref().unwrap().contains("outside"));
    assert!(agents.iter().all(|a| a.prompt.is_empty()));
}

#[test]
fn a_persona_is_found_by_name_the_threads_harness_first() {
    let s = setup();
    write(
        &s.project,
        ".claude/agents/researcher.md",
        "---\nname: researcher\ndescription: Digs\ncolor: blue\n---\nDig deep.",
    );
    write(
        &s.project,
        ".codex/agents/researcher.toml",
        "name = \"researcher\"\ndescription = \"Digs\"\ndeveloper_instructions = \"Dig with Codex.\"",
    );
    write(&s.home, ".claude/agents/mine.md", "---\nname: mine\n---\nMine.");

    let codex = persona(&s.project, Some(&s.home), "Researcher", "codex").unwrap();
    assert_eq!((codex.harness.as_ref(), codex.prompt.as_str()), ("codex", "Dig with Codex."));
    let claude = persona(&s.project, Some(&s.home), "researcher", "claude").unwrap();
    assert_eq!((claude.prompt.as_str(), claude.color.as_deref()), ("Dig deep.", Some("blue")));
    // One for another harness still serves.
    let other = persona(&s.project, Some(&s.home), "researcher", "flsh").unwrap();
    assert_eq!(other.harness.as_ref(), "claude");
    // And the user's own.
    let mine = persona(&s.project, Some(&s.home), "mine", "codex").unwrap();
    assert_eq!(mine.scope, CustomAgentScope::User {});
}

#[test]
fn an_unknown_persona_names_the_ones_there_are() {
    let s = setup();
    write(&s.project, ".claude/agents/researcher.md", "---\nname: researcher\n---\nDig.");
    write(&s.project, ".codex/agents/researcher.toml", "name = \"researcher\"\ndeveloper_instructions = \"Dig.\"");
    write(&s.home, ".claude/agents/mine.md", "---\nname: mine\n---\nMine.");
    // Without instructions it is no persona.
    write(&s.project, ".claude/agents/empty.md", "---\nname: empty\n---\n");

    let error = persona(&s.project, Some(&s.home), "writer", "claude").unwrap_err();
    assert_eq!(error, "There is no custom agent called writer. There are: researcher, mine.");

    let none = setup();
    let error = persona(&none.project, Some(&none.home), "writer", "claude").unwrap_err();
    assert_eq!(error, "There is no custom agent called writer: this folder and the user define none.");
}

#[test]
fn a_personas_brief_names_it_and_holds_its_instructions() {
    let s = setup();
    write(&s.project, ".claude/agents/researcher.md", "---\nname: researcher\n---\nDig deep.");
    let found = persona(&s.project, None, "researcher", "claude").unwrap();
    let brief = persona_brief(&found);
    assert!(brief.contains("researcher"), "{brief}");
    assert!(brief.contains(".claude/agents/researcher.md"), "{brief}");
    assert!(brief.ends_with("Dig deep."), "{brief}");
}
