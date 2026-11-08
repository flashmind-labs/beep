use super::*;

impl Sink for Vec<String> {
    async fn send_text(&mut self, text: String) -> Result<()> {
        self.push(text);
        Ok(())
    }
}

async fn run(shell: &mut Shell, command: &str) -> (String, End) {
    let mut sent = Vec::new();
    let mut out = Output::new(&mut sent);
    let end = shell.run(command, &mut out).await.unwrap();
    out.finish().await.unwrap();
    (sent.concat(), end)
}

#[cfg(unix)]
#[tokio::test]
async fn the_shell_keeps_its_folder_and_variables() {
    let mut shell = Shell::start().await.unwrap();
    let (out, end) = run(&mut shell, "cd / && export GREETING=hi && echo 'it''s set'").await;
    assert_eq!(out, "its set\n");
    assert!(matches!(end, End::Finished { exit_code: 0, ref cwd } if cwd == "/"));
    let (out, _) = run(&mut shell, "printf %s \"$GREETING\"; ls -d nowhere").await;
    assert!(out.starts_with("hi"), "{out}");
    assert!(out.contains("nowhere"), "stderr is merged: {out}");
    let (_, end) = run(&mut shell, "cat; false").await;
    assert!(
        matches!(end, End::Finished { exit_code: 1, .. }),
        "stdin is empty"
    );
    let (out, end) = run(&mut shell, "echo 'unclosed").await;
    assert!(
        matches!(end, End::Finished { exit_code, .. } if exit_code != 0),
        "a syntax error fails the command, not the shell: {out}"
    );
    let (_, end) = run(&mut shell, "exit 3").await;
    assert!(
        matches!(end, End::ShellEnded { exit_code: Some(3) }),
        "exit keeps its code"
    );
}

#[test]
fn quotes_in_a_command_survive_eval() {
    let script = sh_script("echo 'a b'", "__m__");
    assert!(script.starts_with("{ eval 'echo '\\''a b'\\'''\n} < /dev/null\n"));
}
