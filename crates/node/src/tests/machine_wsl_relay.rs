use super::*;
use serde_json::json;

#[test]
fn a_session_reaches_wsl_with_its_folder_and_tools_there() {
    let mut message = json!({
        "method": "session/new",
        "params": {
            "cwd": r"C:\Users\sam\code",
            "additionalDirectories": [r"D:\shared", "/home/sam/notes"],
            "mcpServers": [{
                "name": "beep",
                "command": r"C:\Program Files\Beep\beep-node.exe",
                "args": ["--dir", r"C:\Users\sam\AppData\Roaming\beep-node", "mcp"],
                "env": [{ "name": "BEEP_MCP_TOKEN", "value": "x" }]
            }],
            "prompt": [{ "type": "text", "text": r"look at C:\Users\sam\code\README.md" }]
        }
    });
    translate(&mut message, Toward::Linux);

    let params = &message["params"];
    assert_eq!(params["cwd"], "/mnt/c/Users/sam/code");
    assert_eq!(
        params["additionalDirectories"],
        json!(["/mnt/d/shared", "/home/sam/notes"])
    );
    let server = &params["mcpServers"][0];
    assert_eq!(server["command"], "/mnt/c/Program Files/Beep/beep-node.exe");
    // The Windows program reads its own arguments, as Windows paths.
    assert_eq!(server["args"][1], r"C:\Users\sam\AppData\Roaming\beep-node");
    assert_eq!(
        server["env"][1],
        json!({ "name": "WSLENV", "value": "BEEP_MCP_TOKEN" })
    );
    // Text is never rewritten.
    assert_eq!(
        params["prompt"][0]["text"],
        r"look at C:\Users\sam\code\README.md"
    );
}

#[test]
fn what_the_agent_says_names_windows_paths() {
    let mut message = json!({
        "method": "session/update",
        "params": { "update": {
            "sessionUpdate": "tool_call",
            "locations": [{ "path": "/mnt/c/code/src/main.rs", "line": 4 }],
            "rawInput": { "file_path": "/mnt/c/code/src/main.rs" },
            "content": [{ "type": "diff", "path": "/mnt/c/code/src/main.rs" }, { "type": "text", "text": "/mnt/c/code" }],
            "kind": "read"
        }}
    });
    translate(&mut message, Toward::Windows);

    let update = &message["params"]["update"];
    assert_eq!(update["locations"][0]["path"], r"C:\code\src\main.rs");
    assert_eq!(update["rawInput"]["file_path"], r"C:\code\src\main.rs");
    assert_eq!(update["content"][0]["path"], r"C:\code\src\main.rs");
    assert_eq!(update["content"][1]["text"], "/mnt/c/code");
}

#[test]
fn a_file_the_agent_asks_for_and_paths_only_linux_has() {
    let mut read = json!({ "method": "fs/read_text_file", "params": { "path": "/mnt/d/a.txt" } });
    translate(&mut read, Toward::Windows);
    assert_eq!(read["params"]["path"], r"D:\a.txt");

    let mut home = json!({ "path": "/home/sam/.bashrc", "cwd": "/mnt/cx" });
    translate(&mut home, Toward::Windows);
    assert_eq!(
        home,
        json!({ "path": "/home/sam/.bashrc", "cwd": "/mnt/cx" })
    );
}

#[test]
fn file_uris_cross_both_ways() {
    let mut link = json!({ "type": "resource_link", "uri": "file:///C:/code/notes.md" });
    translate(&mut link, Toward::Linux);
    assert_eq!(link["uri"], "file:///mnt/c/code/notes.md");
    translate(&mut link, Toward::Windows);
    assert_eq!(link["uri"], "file:///C:/code/notes.md");

    let mut web = json!({ "uri": "https://example.com/C:/x" });
    translate(&mut web, Toward::Linux);
    assert_eq!(web["uri"], "https://example.com/C:/x");
}
