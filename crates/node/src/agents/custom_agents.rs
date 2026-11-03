//! The custom agents coding agent harnesses define, for a project page's
//! Custom agents list (`SessionsRequest::CustomAgents`): Markdown with
//! YAML frontmatter (Claude Code, OpenCode, pi, flsh), Codex role files,
//! and the agent tables of `opencode.json` and Codex's `config.toml`. They
//! are read in the project folder, then in each harness's folder in the
//! home. The project folder is checked against the node's work folders
//! before it gets here (`reach::Access::Work`). A harness folder in the
//! home, a definitions folder or a file whose real path leads outside the
//! folder it was found in is not read.

mod parse;

use anyhow::{Context, Result, bail};
use beep_proto::{
    CustomAgent, CustomAgentHarness, CustomAgentScope, MAX_CUSTOM_AGENT_PROMPT, MAX_CUSTOM_AGENTS,
};
use parse::Found;
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// The largest definition file read.
const MAX_FILE: u64 = 256 * 1024;

/// The most files read from one folder.
const MAX_FOLDER_FILES: usize = 100;

#[derive(Debug, Clone, Copy)]
enum Format {
    /// A folder of `*.md` files with frontmatter.
    Markdown,
    /// A folder of Codex role files, `*.toml`.
    CodexRoles,
    /// An `opencode.json` or `opencode.jsonc`.
    OpenCodeConfig,
    /// A Codex `config.toml`.
    CodexConfig,
}

/// One place a harness keeps definitions: `rel` is relative to the folder
/// it is read in, a folder or a file as `format` says.
struct Place {
    harness: CustomAgentHarness,
    rel: &'static str,
    format: Format,
}

const fn place(harness: CustomAgentHarness, rel: &'static str, format: Format) -> Place {
    Place {
        harness,
        rel,
        format,
    }
}

const CLAUDE: CustomAgentHarness = CustomAgentHarness::Claude {};
const OPENCODE: CustomAgentHarness = CustomAgentHarness::OpenCode {};
const CODEX: CustomAgentHarness = CustomAgentHarness::Codex {};
const PI: CustomAgentHarness = CustomAgentHarness::Pi {};
const FLSH: CustomAgentHarness = CustomAgentHarness::Flsh {};

/// Where each harness looks in a project folder.
const PROJECT: &[Place] = &[
    place(CLAUDE, ".claude/agents", Format::Markdown),
    place(OPENCODE, ".opencode/agent", Format::Markdown),
    place(OPENCODE, ".opencode/agents", Format::Markdown),
    place(OPENCODE, "opencode.json", Format::OpenCodeConfig),
    place(OPENCODE, "opencode.jsonc", Format::OpenCodeConfig),
    place(CODEX, ".codex/agents", Format::CodexRoles),
    place(CODEX, ".codex/config.toml", Format::CodexConfig),
    place(PI, ".pi/agents", Format::Markdown),
    place(FLSH, ".agents", Format::Markdown),
];

/// A harness's own folder in the home, and where it looks inside it.
struct UserPlace {
    /// Relative to the home.
    folder: &'static str,
    place: Place,
}

const fn user(folder: &'static str, place: Place) -> UserPlace {
    UserPlace { folder, place }
}

/// Where each harness looks in the home.
const USER: &[UserPlace] = &[
    user(".claude", place(CLAUDE, "agents", Format::Markdown)),
    user(
        ".config/opencode",
        place(OPENCODE, "agent", Format::Markdown),
    ),
    user(
        ".config/opencode",
        place(OPENCODE, "agents", Format::Markdown),
    ),
    user(
        ".config/opencode",
        place(OPENCODE, "opencode.json", Format::OpenCodeConfig),
    ),
    user(
        ".config/opencode",
        place(OPENCODE, "opencode.jsonc", Format::OpenCodeConfig),
    ),
    user(".codex", place(CODEX, "agents", Format::CodexRoles)),
    user(".codex", place(CODEX, "config.toml", Format::CodexConfig)),
    user(".pi/agent", place(PI, "agents", Format::Markdown)),
    user(".agents", place(FLSH, "", Format::Markdown)),
];

/// A folder definitions are read in, its symlinks resolved; nothing
/// outside it is read. `shown` goes before a path inside it in a row.
struct Base {
    root: PathBuf,
    scope: CustomAgentScope,
    shown: String,
}

impl Base {
    /// What `full`, inside the root, shows as.
    fn show(&self, full: &Path) -> String {
        let inside = full
            .strip_prefix(&self.root)
            .unwrap_or(full)
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        format!("{}{inside}", self.shown)
    }
}

/// The custom agents for the project folder `folder`, already resolved
/// and checked as a work folder, then the user's own in `home`. Blocks on
/// the file system.
pub(super) fn list(folder: &Path, home: Option<&Path>) -> Result<Vec<CustomAgent>> {
    let root = fs::canonicalize(folder)
        .with_context(|| format!("the folder {} is not there", folder.display()))?;
    if !root.is_dir() {
        bail!("{} is not a folder", folder.display());
    }
    let mut reader = Reader::default();
    let project = Base {
        root,
        scope: CustomAgentScope::Project {},
        shown: String::new(),
    };
    for place in PROJECT {
        reader.read(&project, place);
    }
    let Some(home) = home.and_then(|home| fs::canonicalize(home).ok()) else {
        return Ok(reader.agents);
    };
    for at in USER {
        let Ok(root) = fs::canonicalize(home.join(at.folder)) else {
            continue;
        };
        // A harness folder linked out of the home is not read.
        if !root.starts_with(&home) {
            log::debug!("custom agents: ~/{} leads outside the home", at.folder);
            continue;
        }
        let base = Base {
            root,
            scope: CustomAgentScope::User {},
            shown: format!("~/{}/", at.folder),
        };
        reader.read(&base, &at.place);
    }
    Ok(reader.agents)
}

/// The rows read so far, and the files they came from, so a file is read
/// once when the project folder is the home.
#[derive(Default)]
struct Reader {
    agents: Vec<CustomAgent>,
    seen: HashSet<PathBuf>,
}

/// Where a row came from.
struct Source<'a> {
    base: &'a Base,
    harness: CustomAgentHarness,
    path: String,
    /// The file's stem, the name when the file gives none.
    stem: String,
}

impl Source<'_> {
    fn row(&self, found: Found) -> CustomAgent {
        let (prompt, prompt_cut) = cap(found.prompt);
        CustomAgent {
            name: found.name.unwrap_or_else(|| self.stem.clone()),
            harness: self.harness,
            scope: self.base.scope,
            description: found.description,
            model: found.model,
            tools: found.tools,
            color: found.color,
            path: self.path.clone(),
            prompt,
            prompt_cut,
            error: found.error,
        }
    }

    fn error(&self, message: String) -> CustomAgent {
        self.row(Found {
            error: Some(message),
            ..Found::default()
        })
    }
}

impl Reader {
    fn full(&self) -> bool {
        self.agents.len() >= MAX_CUSTOM_AGENTS
    }

    fn push(&mut self, agent: CustomAgent) {
        if !self.full() {
            self.agents.push(agent);
        }
    }

    fn read(&mut self, base: &Base, place: &Place) {
        let at = base.root.join(place.rel);
        match place.format {
            Format::Markdown => self.read_folder(base, place, &at, "md"),
            Format::CodexRoles => self.read_folder(base, place, &at, "toml"),
            Format::OpenCodeConfig | Format::CodexConfig => {
                if fs::symlink_metadata(&at).is_ok() {
                    self.read_file(base, place, &at);
                }
            }
        }
    }

    fn read_folder(&mut self, base: &Base, place: &Place, folder: &Path, ext: &str) {
        match fs::canonicalize(folder) {
            Ok(real) if !real.starts_with(&base.root) => {
                let source = Source {
                    base,
                    harness: place.harness,
                    path: base.show(folder),
                    stem: place.rel.to_owned(),
                };
                return self.push(source.error(
                    "the folder leads outside its harness folder, so it is not read".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return,
            Err(e) => {
                log::debug!("custom agents: cannot open {}: {e}", folder.display());
                return;
            }
        }
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return,
            Err(e) => {
                log::debug!("custom agents: cannot list {}: {e}", folder.display());
                return;
            }
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|e| e == ext))
            .collect();
        files.sort();
        files.truncate(MAX_FOLDER_FILES);
        for file in files {
            self.read_file(base, place, &file);
        }
    }

    fn read_file(&mut self, base: &Base, place: &Place, file: &Path) {
        if self.full() {
            return;
        }
        let source = Source {
            base,
            harness: place.harness,
            path: base.show(file),
            stem: file
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        let text = match read_inside(&base.root, file) {
            Ok(Some(Inside { real, text })) => {
                if !self.seen.insert(real) {
                    return;
                }
                text
            }
            // A folder named like a definition.
            Ok(None) => return,
            Err(e) => return self.push(source.error(format!("{e:#}"))),
        };
        match place.format {
            Format::Markdown => self.push(source.row(parse::markdown(&text))),
            Format::CodexRoles => match parse::codex_role(&text) {
                Ok(found) => self.push(source.row(found)),
                Err(e) => self.push(source.error(format!("{e:#}"))),
            },
            Format::OpenCodeConfig => match parse::opencode_config(&text) {
                Ok(all) => {
                    for found in all {
                        self.push(source.row(found));
                    }
                }
                Err(e) => self.push(source.error(format!("{e:#}"))),
            },
            Format::CodexConfig => match parse::codex_config(&text) {
                Ok(all) => {
                    for declared in all {
                        let found = codex_declared(&base.root, file, declared);
                        self.push(source.row(found));
                    }
                }
                Err(e) => self.push(source.error(format!("{e:#}"))),
            },
        }
    }
}

/// A role `config.toml` declares, with what its role file adds: the
/// prompt and model.
fn codex_declared(root: &Path, config: &Path, declared: parse::CodexDeclared) -> Found {
    let mut found = Found {
        name: Some(declared.name),
        description: declared.description,
        ..Found::default()
    };
    let Some(rel) = declared.config_file else {
        return found;
    };
    let file = config.parent().unwrap_or(root).join(&rel);
    let role = read_inside(root, &file).and_then(|inside| match inside {
        Some(inside) => parse::codex_role(&inside.text),
        None => bail!("{rel} is not a file"),
    });
    match role {
        Ok(role) => {
            found.model = role.model;
            found.prompt = role.prompt;
            found.error = role.error;
            if found.description.is_empty() {
                found.description = role.description;
            }
        }
        Err(e) => found.error = Some(format!("its config_file {rel}: {e:#}")),
    }
    found
}

/// A definition file's text and real path.
struct Inside {
    real: PathBuf,
    text: String,
}

/// The text of `file`, once its symlinks resolve inside `root`; `None`
/// when it is not a file. An error when it leads outside, is too large or
/// is not UTF-8.
fn read_inside(root: &Path, file: &Path) -> Result<Option<Inside>> {
    let real = fs::canonicalize(file).context("it cannot be opened")?;
    if !real.starts_with(root) {
        bail!("it leads outside its folder, so it is not read");
    }
    let meta = fs::metadata(&real).context("it cannot be opened")?;
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_FILE {
        bail!("it is larger than {} KB", MAX_FILE / 1024);
    }
    let bytes = fs::read(&real).context("it cannot be read")?;
    let text = String::from_utf8(bytes).context("it is not UTF-8 text")?;
    Ok(Some(Inside { real, text }))
}

/// `prompt` cut to [`MAX_CUSTOM_AGENT_PROMPT`] bytes on a character
/// boundary, and whether it was.
fn cap(mut prompt: String) -> (String, bool) {
    if prompt.len() <= MAX_CUSTOM_AGENT_PROMPT {
        return (prompt, false);
    }
    let mut end = MAX_CUSTOM_AGENT_PROMPT;
    while !prompt.is_char_boundary(end) {
        end -= 1;
    }
    prompt.truncate(end);
    (prompt, true)
}

#[cfg(test)]
mod tests {
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
            .map(|a| (a.harness.as_str(), a.name.as_str()))
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

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_folder_is_not_read() {
        let s = setup();
        let p = Path::new(&s.project);
        write(&s.home, "secret.md", "---\nname: secret\n---\nPrivate.");
        fs::create_dir_all(p.join(".claude/agents")).unwrap();
        std::os::unix::fs::symlink(s.home.join("secret.md"), p.join(".claude/agents/out.md"))
            .unwrap();
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
}
