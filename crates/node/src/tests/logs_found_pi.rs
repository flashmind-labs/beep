use super::*;
use std::fs;

const ID: &str = "01a04575-2085-7b50-ace5-79c3f3ba9ff3";

fn session(root: &Path, lines: &[&str]) -> PathBuf {
    let dir = root.join("--home-user-proj--");
    fs::create_dir_all(&dir).expect("folders");
    let path = dir.join(format!("2026-08-27T23-01-39-077Z_{ID}.jsonl"));
    fs::write(&path, lines.join("\n") + "\n").expect("a session");
    path
}

const HEADER: &str = r#"{"type":"session","version":3,"id":"01a04575-2085-7b50-ace5-79c3f3ba9ff3","timestamp":"2026-08-27T23:01:39.077Z","cwd":"/home/user/proj"}"#;
const MODEL: &str = r#"{"type":"model_change","id":"a1","parentId":null,"timestamp":"2026-08-27T23:01:40.000Z","provider":"p","modelId":"m"}"#;
const USER: &str = r#"{"type":"message","id":"b2","parentId":"a1","timestamp":"2026-08-27T23:01:41.000Z","message":{"role":"user","content":[{"type":"text","text":"Tidy the README"}],"timestamp":1}}"#;

#[test]
fn reads_the_header_and_the_first_prompt() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = session(root.path(), &[HEADER, MODEL, USER]);
    fs::write(root.path().join("--home-user-proj--/notes.md"), "x").expect("a file");
    let found = sessions(root.path(), &Memos::default());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].agent, Agent::Pi);
    assert_eq!(found[0].id, ID);
    assert_eq!(found[0].cwd, Path::new("/home/user/proj"));
    assert!(found[0].started_at > 0);
    assert_eq!(title(&path).as_deref(), Some("Tidy the README"));
}
