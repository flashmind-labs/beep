use super::*;

#[test]
fn broken_yaml_still_gives_what_its_lines_say() {
    let text = "---\nname: helper\ndescription: Use it when: tests fail\n  bad: [\n---\nHelp.";
    let found = markdown(text);
    assert_eq!(found.name.as_deref(), Some("helper"));
    assert_eq!(found.description, "Use it when: tests fail");
    assert!(found.error.unwrap().contains("not valid YAML"));
}

#[test]
fn codex_role_file() {
    let text = "name = \"explorer\"\ndescription = \"Reads code\"\nmodel = \"gpt-5\"\n\
                developer_instructions = \"\"\"\nRead, never write.\n\"\"\"\n";
    let found = codex_role(text).unwrap();
    assert_eq!(found.name.as_deref(), Some("explorer"));
    assert_eq!(found.model.as_deref(), Some("gpt-5"));
    assert_eq!(found.prompt, "Read, never write.");
    assert_eq!(found.error, None);
    assert!(codex_role("name = \"x\"").unwrap().error.is_some());
    assert!(codex_role("name = ").is_err());
}

#[test]
fn codex_config_roles_skip_plain_settings() {
    let text = "model = \"gpt-5\"\n[agents]\nmax_threads = 4\n\
                [agents.reviewer]\ndescription = \"Reviews\"\nconfig_file = \"agents/reviewer.toml\"\n";
    assert_eq!(
        codex_config(text).unwrap(),
        [CodexDeclared {
            name: "reviewer".to_owned(),
            description: "Reviews".to_owned(),
            config_file: Some("agents/reviewer.toml".to_owned()),
        }]
    );
}

#[test]
fn opencode_jsonc_with_comments_and_trailing_commas() {
    let text = r#"{
            // the agents
            "$schema": "https://example.com/config.json",
            "agent": {
                "planner": {
                    "description": "Plans // not a comment",
                    "model": "provider/model-a", /* inline */
                    "prompt": "{file:./prompts/plan.txt}",
                    "tools": { "write": false, },
                },
            },
        }"#;
    let found = opencode_config(text).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name.as_deref(), Some("planner"));
    assert_eq!(found[0].description, "Plans // not a comment");
    assert_eq!(found[0].tools, ["write off"]);
    assert_eq!(found[0].prompt, "{file:./prompts/plan.txt}");
    assert!(opencode_config("{\"model\": \"x\"}").unwrap().is_empty());
    assert!(opencode_config("{").is_err());
}
