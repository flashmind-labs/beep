//! Where a project's commands run.

use anyhow::Result;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{borrow::Cow, time::Duration};

/// What a script printed and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOutput {
    /// `None` when the script did not exit with a code.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// What it printed, clipped by [`Clip`]: the start and the end.
    pub output: String,
}

impl ShellOutput {
    /// A script that exited with `code` and printed `output`.
    pub fn exited(code: i32, output: &str) -> ShellOutput {
        ShellOutput {
            exit_code: Some(code),
            timed_out: false,
            output: output.to_owned(),
        }
    }
}

/// One machine's shell. The server runs scripts over a node's link and the desktop through a
/// peer. A script may carry a token, so implementations never log it.
#[async_trait]
pub trait Shell: Send + Sync {
    async fn run(&self, script: &str, limit: Duration) -> Result<ShellOutput>;
}

/// The most of the start of a script's output kept.
const HEAD: usize = 256 * 1024;
/// The most of the end kept: a clone says how it finished on its last line.
const TAIL: usize = 8 * 1024;

/// Collects a script's output for [`ShellOutput`], keeping its start and its end.
#[derive(Default)]
pub struct Clip {
    head: String,
    tail: String,
}

impl Clip {
    pub fn push(&mut self, text: &str) {
        let room = HEAD.saturating_sub(self.head.len());
        let mut cut = text.len().min(room);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        self.head.push_str(&text[..cut]);
        self.tail.push_str(&text[cut..]);
        let mut start = self.tail.len().saturating_sub(TAIL);
        while !self.tail.is_char_boundary(start) {
            start += 1;
        }
        self.tail.drain(..start);
    }

    pub fn finish(mut self) -> String {
        self.head.push_str(&self.tail);
        self.head
    }
}

/// `text` as one shell word.
pub fn quote(text: &str) -> String {
    // Only a NUL byte cannot be quoted, and no command can carry one.
    shlex::try_quote(&text.replace('\0', ""))
        .map(|quoted| quoted.into_owned())
        .unwrap_or_default()
}

/// The code a script ends with on Windows when Git for Windows, and so its `sh.exe`, is missing.
pub const NO_GIT_SH: i32 = 127;

/// What a script prints when it ends with [`NO_GIT_SH`].
pub const NO_GIT_SH_TEXT: &str = "Git for Windows is not installed on this machine.";

/// Whether a machine that reports `os` runs PowerShell rather than a POSIX shell.
pub fn is_windows(os: &str) -> bool {
    os == "windows"
}

/// `script`, a POSIX sh script, as the machine's shell takes it: as it is, or on Windows, whose
/// node runs PowerShell, through [`posix_on_windows`].
pub fn for_machine(script: &str, windows: bool) -> Cow<'_, str> {
    if windows {
        Cow::Owned(posix_on_windows(script))
    } else {
        Cow::Borrowed(script)
    }
}

/// A PowerShell command that runs the POSIX `script` in the `sh.exe` of Git for Windows, found
/// from `git`. The script travels as base64 and reaches sh in an environment variable, so no
/// quoting can break it: sh expands that variable with no splitting or globbing and evaluates
/// it, stderr merged into stdout. Without git or its sh it prints [`NO_GIT_SH_TEXT`] and fails
/// with [`NO_GIT_SH`]. It never calls `exit`, which would end a node's long-lived shell.
pub fn posix_on_windows(script: &str) -> String {
    // Undoes what the sh command line sets to read the variable whole. `/usr/bin` holds the
    // tools scripts use (`head`, `base64`), which a bare `usr\bin\sh.exe` leaves off PATH.
    let script = format!("set +f; unset IFS; PATH=\"/usr/bin:$PATH\"; {script}");
    let encoded = STANDARD.encode(script);
    format!(
        "$__beep_sh = $null; \
         $__beep_git = Get-Command git -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1; \
         if ($__beep_git) {{ \
         $__beep_roots = @(Split-Path (Split-Path $__beep_git.Source)); \
         $__beep_exec = & $__beep_git.Source --exec-path 2>$null; \
         if ($__beep_exec) {{ $__beep_roots += Split-Path (Split-Path (Split-Path $__beep_exec)) }}; \
         if ($env:ProgramFiles) {{ $__beep_roots += Join-Path $env:ProgramFiles 'Git' }}; \
         foreach ($__beep_root in $__beep_roots) {{ foreach ($__beep_rel in 'bin\\sh.exe', 'usr\\bin\\sh.exe') {{ \
         $__beep_try = Join-Path $__beep_root $__beep_rel; \
         if (-not $__beep_sh -and (Test-Path -LiteralPath $__beep_try -PathType Leaf)) {{ $__beep_sh = $__beep_try }} }} }} }}; \
         if (-not $__beep_sh) {{ '{NO_GIT_SH_TEXT}'; $global:LASTEXITCODE = {NO_GIT_SH} }} \
         else {{ \
         $env:BEEP_SH = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}')); \
         try {{ & $__beep_sh -c '{SH_LINE}' }} \
         finally {{ Remove-Item Env:BEEP_SH -ErrorAction SilentlyContinue }} }}"
    )
}

/// What sh runs on Windows: the script in `BEEP_SH`, read as one word. No double quotes, which
/// Windows PowerShell would pass on mangled.
const SH_LINE: &str = "exec 2>&1; set -f; IFS=; eval $BEEP_SH";

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Runs `script` as Git's sh runs it on Windows, with the variable PowerShell sets.
    fn as_git_sh_runs(script: &str) -> (Option<i32>, String) {
        let wrapped = posix_on_windows(script);
        let encoded = wrapped
            .split("FromBase64String('")
            .nth(1)
            .and_then(|rest| rest.split('\'').next())
            .expect("the script travels as base64");
        let decoded = String::from_utf8(STANDARD.decode(encoded).expect("base64")).expect("utf-8");
        let out = Command::new("sh")
            .arg("-c")
            .arg(SH_LINE)
            .env("BEEP_SH", decoded)
            .output()
            .expect("sh runs");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    #[test]
    fn a_windows_machine_runs_the_script_whole_in_git_sh() {
        let script = "x='a  b'; for w in $x; do echo \"[$w]\"; done; echo \"$x\"; \
                      case $- in *f*) echo noglob;; esac; echo 'it''s'; echo oops >&2; exit 7";
        let (code, out) = as_git_sh_runs(script);
        assert_eq!(code, Some(7));
        // Splitting and globbing are back for the script itself, and stderr is merged.
        assert_eq!(out, "[a]\n[b]\na  b\nits\noops\n");
        let wrapped = posix_on_windows(script);
        assert!(
            !wrapped.contains("$x"),
            "the script never shows in the line"
        );
        assert!(wrapped.contains(&format!("-c '{SH_LINE}'")));
        assert!(!SH_LINE.contains('"') && !SH_LINE.contains('\''));
    }

    #[test]
    fn only_windows_gets_the_wrapper() {
        assert_eq!(for_machine("echo hi", false), "echo hi");
        let windows = for_machine("echo hi", true);
        assert!(windows.contains("Get-Command git") && windows.contains("bin\\sh.exe"));
        assert!(windows.contains(NO_GIT_SH_TEXT));
        assert!(is_windows("windows") && !is_windows("macos"));
    }

    #[test]
    fn output_keeps_its_start_and_end_on_character_boundaries() {
        let mut clip = Clip::default();
        clip.push(&"a".repeat(HEAD - 1));
        clip.push("éé");
        clip.push(&"b".repeat(TAIL * 2));
        clip.push("end");
        let kept = clip.finish();
        assert!(kept.starts_with("aaa"));
        assert!(kept.ends_with("bbbend"));
        assert!(kept.len() <= HEAD + 1 + TAIL + 1);
    }
}
