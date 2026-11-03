//! Windows runs commands in PowerShell: `pwsh` when installed, else the
//! Windows PowerShell every Windows has. A command reaches it as base64 or
//! through the environment, so no quoting can break the line around it.

use base64::{Engine, engine::general_purpose::STANDARD};

/// The environment variable a one-shot script travels in.
pub(super) const SCRIPT_VAR: &str = "BEEP_SCRIPT";

/// Arguments every PowerShell the node starts gets.
const COMMON: [&str; 5] = [
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
];

/// Output as UTF-8 without progress bars, which would land in the output
/// as text.
const SETUP: &str = "$ProgressPreference = 'SilentlyContinue'; \
                     [Console]::OutputEncoding = [Text.Encoding]::UTF8; \
                     $OutputEncoding = [Text.Encoding]::UTF8";

pub(super) fn program() -> &'static str {
    if which::which("pwsh").is_ok() {
        "pwsh"
    } else {
        "powershell"
    }
}

/// The arguments of a session that reads its commands on stdin, one per
/// line.
pub(super) fn session_args() -> Vec<&'static str> {
    COMMON.into_iter().chain(["-Command", "-"]).collect()
}

/// The first line a session is sent.
pub(super) fn session_setup() -> String {
    format!("{SETUP}\n")
}

/// Runs the script `source` evaluates to in the current scope, every
/// stream merged into stdout, and sets `$__beep_code`: the last native
/// program's exit code, else 1 if anything wrote an error, else 0. Errors
/// print as their message alone, so a native program's stderr reads as it
/// wrote it. A syntax error prints and fails like any other error.
fn run(source: &str) -> String {
    format!(
        "$global:LASTEXITCODE = 0; $__beep_err = $false; \
         try {{ . ([ScriptBlock]::Create({source})) *>&1 | ForEach-Object {{ \
         if ($_ -is [Management.Automation.ErrorRecord]) {{ $__beep_err = $true; $_.ToString() }} \
         else {{ $_ }} }} }} \
         catch {{ $__beep_err = $true; $__beep_e = $_.Exception; \
         if ($__beep_e.InnerException) {{ $__beep_e = $__beep_e.InnerException }}; $__beep_e.Message }}; \
         $__beep_code = if ($LASTEXITCODE) {{ $LASTEXITCODE }} elseif ($__beep_err) {{ 1 }} else {{ 0 }}"
    )
}

/// The arguments that run the script in `SCRIPT_VAR` once, exiting with
/// its code.
pub(super) fn one_shot_args() -> Vec<String> {
    let script = format!(
        "{SETUP}; $__beep_script = $env:{SCRIPT_VAR}; Remove-Item Env:{SCRIPT_VAR}; {}; \
         exit $__beep_code",
        run("$__beep_script")
    );
    COMMON
        .into_iter()
        .map(str::to_owned)
        .chain(["-Command".to_owned(), script])
        .collect()
}

/// One command for a session, on one line, then a line with `mark`, the
/// exit code and the folder. It runs in the session's own scope, so `cd`
/// and variables carry over.
pub(super) fn script(command: &str, mark: &str) -> String {
    let encoded = STANDARD.encode(command);
    let source =
        format!("[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))");
    format!(
        "{}; [Console]::Out.Write(\"`n{mark} $__beep_code $((Get-Location).ProviderPath)`n\")\n",
        run(&source)
    )
}
