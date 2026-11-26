use super::{
    Language,
    client::{Client, encode_frame, file_uri, parse_locations, read_frame, server_request_reply},
    find_server, language_for,
};
use serde_json::json;
use std::{fs, io::Cursor};
use tokio::io::BufReader;

#[test]
fn language_mapping_covers_common_extensions() {
    assert_eq!(language_for("src/main.rs"), Some((Language::Rust, "rust")));
    assert_eq!(
        language_for("app/view.tsx"),
        Some((Language::TypeScript, "typescriptreact"))
    );
    assert_eq!(
        language_for("app/mod.ts"),
        Some((Language::TypeScript, "typescript"))
    );
    assert_eq!(
        language_for("script.py"),
        Some((Language::Python, "python"))
    );
    assert_eq!(language_for("cmd/main.go"), Some((Language::Go, "go")));
    assert_eq!(language_for("src/lib.cpp"), Some((Language::CCpp, "cpp")));
    assert_eq!(language_for("README.md"), None);
    assert_eq!(language_for("Makefile"), None);
}

#[tokio::test]
async fn frame_round_trip_and_headers() {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "result": { "label": "héllo — café" }
    });
    let frame = encode_frame(&payload);
    let mut reader = BufReader::new(Cursor::new(frame));
    assert_eq!(read_frame(&mut reader).await.unwrap(), Some(payload));
    assert_eq!(read_frame(&mut reader).await.unwrap(), None);

    let body = br#"{"jsonrpc":"2.0","id":1,"result":null}"#;
    let raw = format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        std::str::from_utf8(body).unwrap()
    );
    let mut reader = BufReader::new(Cursor::new(raw.into_bytes()));
    let decoded = read_frame(&mut reader).await.unwrap().unwrap();
    assert_eq!(decoded["id"], 1);
}

#[test]
fn parse_locations_handles_all_lsp_forms_and_deduplicates() {
    let dir = tempfile::tempdir().unwrap();
    let root = crate::fs::paths::canonicalize(dir.path()).unwrap();
    let inside = root.join("src").join("lib.rs");
    fs::create_dir_all(inside.parent().unwrap()).unwrap();
    fs::write(&inside, "pub fn demo() {}\n").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    let outside = crate::fs::paths::canonicalize(outside_dir.path())
        .unwrap()
        .join("ext.rs");
    fs::write(&outside, "pub fn ext() {}\n").unwrap();

    let inside_uri = file_uri(&inside).unwrap();
    let outside_uri = file_uri(&outside).unwrap();

    let single = json!({
        "uri": inside_uri,
        "range": { "start": { "line": 4, "character": 7 }, "end": { "line": 4, "character": 11 } }
    });
    let parsed = parse_locations(&single, &root);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].path, "src/lib.rs");
    assert!(parsed[0].inside_root);
    assert_eq!((parsed[0].line, parsed[0].column), (4, 7));

    let links = json!([
        {
            "targetUri": inside_uri,
            "targetRange": { "start": { "line": 1, "character": 0 }, "end": { "line": 10, "character": 1 } },
            "targetSelectionRange": { "start": { "line": 2, "character": 3 }, "end": { "line": 2, "character": 8 } }
        },
        {
            "uri": inside_uri,
            "range": { "start": { "line": 2, "character": 3 }, "end": { "line": 2, "character": 8 } }
        },
        {
            "uri": outside_uri,
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }
        }
    ]);
    let parsed = parse_locations(&links, &root);
    assert_eq!(parsed.len(), 2);
    assert!(parsed[0].inside_root);
    assert_eq!(parsed[0].path, "src/lib.rs");
    assert_eq!((parsed[0].line, parsed[0].column), (2, 3));
    assert!(!parsed[1].inside_root);
    assert_eq!(parsed[1].path, outside.to_string_lossy());

    assert!(parse_locations(&json!(null), &root).is_empty());
}

#[test]
fn server_request_reply_answers_common_requests() {
    let config = server_request_reply(
        "workspace/configuration",
        Some(&json!({ "items": [{}, {}] })),
        "file:///repo",
        "repo",
    );
    assert_eq!(config, json!([null, null]));

    let folders = server_request_reply("workspace/workspaceFolders", None, "file:///repo", "repo");
    assert_eq!(folders, json!([{ "uri": "file:///repo", "name": "repo" }]));

    let progress = server_request_reply(
        "window/workDoneProgress/create",
        Some(&json!({ "token": "1" })),
        "file:///repo",
        "repo",
    );
    assert_eq!(progress, json!(null));
}

#[test]
fn find_server_prefers_workspace_local_binary() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("node_modules/.bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let binary = bin_dir.join(if cfg!(windows) { "vtsls.cmd" } else { "vtsls" });
    fs::write(&binary, "#!/bin/sh\n").unwrap();

    let found = find_server(Language::TypeScript, dir.path(), None, None);
    assert_eq!(found, Some((binary, &["--stdio"][..])));
}

#[cfg(unix)]
#[tokio::test]
async fn client_handles_handshake_server_request_did_change_definition_and_graceful_shutdown() {
    use std::os::unix::fs::PermissionsExt;

    if which::which("python3").is_err() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = crate::fs::paths::canonicalize(dir.path()).unwrap();
    let file = root.join("demo.rs");
    let marker = root.join("clean-exit");
    fs::write(&file, "fn alpha() {}\n").unwrap();

    let server_script = root.join("fake-lsp.py");
    fs::write(
        &server_script,
        r#"#!/usr/bin/env python3
import json, pathlib, sys

def read_msg():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
    body = sys.stdin.buffer.read(length)
    return json.loads(body.decode("utf-8"))

def send_msg(obj):
    data = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(data)}\r\n\r\n".encode("ascii") + data)
    sys.stdout.buffer.flush()

last_text = ""
saw_shutdown = False
while True:
    msg = read_msg()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": 99, "method": "workspace/configuration", "params": {"items": [{}]}})
        cfg_reply = read_msg()
        assert cfg_reply["id"] == 99 and cfg_reply["result"] == [None]
        send_msg({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {"definitionProvider": True}}})
    elif method == "textDocument/didOpen":
        last_text = msg["params"]["textDocument"]["text"]
    elif method == "textDocument/didChange":
        last_text = msg["params"]["contentChanges"][0]["text"]
    elif method == "textDocument/definition":
        uri = msg["params"]["textDocument"]["uri"]
        line = 2 if "beta" in last_text else 0
        send_msg({"jsonrpc": "2.0", "id": msg["id"], "result": {"uri": uri, "range": {"start": {"line": line, "character": 3}, "end": {"line": line, "character": 8}}}})
    elif method == "shutdown":
        saw_shutdown = True
        send_msg({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif method == "exit":
        if saw_shutdown:
            pathlib.Path("clean-exit").write_text("ok")
        break
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&server_script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&server_script, perms).unwrap();

    let client = Client::spawn(&root, &server_script, &[]).await.unwrap();
    client
        .sync_document(&file, "rust", "fn alpha() {}\n")
        .await
        .unwrap();
    let first = client.definition(&file, 0, 3).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].line, first[0].column), (0, 3));

    client
        .sync_document(&file, "rust", "fn alpha() {}\n\nfn beta() {}\n")
        .await
        .unwrap();
    let second = client.definition(&file, 2, 3).await.unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!((second[0].line, second[0].column), (2, 3));

    client.shutdown().await;
    assert!(!client.is_alive());
    assert!(marker.is_file(), "expected graceful LSP shutdown + exit");
}
