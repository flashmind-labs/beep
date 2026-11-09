use super::*;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

const CONSOLE_CHECK: &str = r#"Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class ConsoleProbe { [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow(); }'; [ConsoleProbe]::GetConsoleWindow().ToInt64()"#;

#[test]
fn synchronous_background_commands_have_no_console() {
    let output = background_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", CONSOLE_CHECK])
        .output()
        .expect("PowerShell starts");
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[tokio::test]
async fn asynchronous_background_commands_have_no_console() {
    let output = command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", CONSOLE_CHECK])
        .output()
        .await
        .expect("PowerShell starts");
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[test]
fn interactive_pty_commands_still_produce_output() {
    use std::{
        io::{Read, Write},
        sync::mpsc,
        thread,
    };

    let pty = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize::default())
        .expect("a terminal opens");
    let mut command = portable_pty::CommandBuilder::new("cmd.exe");
    command.args(["/d", "/c", "echo terminal-ready"]);
    let mut child = pty
        .slave
        .spawn_command(command)
        .expect("cmd starts in the terminal");
    drop(pty.slave);
    let mut output = pty.master.try_clone_reader().expect("the terminal reads");
    let mut input = pty
        .master
        .take_writer()
        .expect("the terminal accepts input");
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut text = Vec::new();
        let mut bytes = [0; 256];
        let mut cursor_answered = false;
        while let Ok(read) = output.read(&mut bytes) {
            if read == 0 {
                break;
            }
            text.extend_from_slice(&bytes[..read]);
            // ConPTY inherits the cursor by asking the terminal emulator.
            if !cursor_answered && text.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                input
                    .write_all(b"\x1b[1;1R")
                    .expect("cursor position reply");
                cursor_answered = true;
            }
            if String::from_utf8_lossy(&text).contains("terminal-ready") {
                let _ = send.send(());
                break;
            }
        }
    });
    let read = receive.recv_timeout(Duration::from_secs(15));
    if read.is_err() {
        let _ = child.kill();
    }
    read.expect("the terminal prints its output");
    assert!(child.wait().expect("cmd exits").success());
}

#[tokio::test]
async fn stopping_a_leader_ends_what_it_started() {
    // ping inherits stdout, so it reads to its end only once ping is
    // gone as well as cmd.
    let mut command = Command::new("cmd");
    command
        .args(["/d", "/c", "echo ready& ping -n 300 127.0.0.1"])
        .stdout(Stdio::piped());
    let mut child = own_group(&mut command).spawn().expect("cmd starts");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut leader = GroupLeader::new(child);
    assert!(leader.group().is_some(), "the child is in a job");
    let mut line = String::new();
    stdout.read_line(&mut line).await.expect("cmd writes");
    assert_eq!(line.trim_end(), "ready");
    leader.stop(Duration::from_secs(600)).await;
    assert!(leader.reaped());
    let mut rest = Vec::new();
    timeout(Duration::from_secs(20), stdout.read_to_end(&mut rest))
        .await
        .expect("the whole job stopped")
        .expect("stdout reads");
}
