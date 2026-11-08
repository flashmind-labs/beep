param([string[]]$TauriArgs = @())

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location (Join-Path $repo 'apps/desktop')
$previousWrapper = $env:RUSTC_WRAPPER
try {
    if (-not $env:RUSTC_WRAPPER -and (Get-Command sccache -ErrorAction SilentlyContinue)) {
        $env:RUSTC_WRAPPER = 'sccache'
    }
    & npm.cmd ci
    if ($LASTEXITCODE) { throw 'npm ci failed' }
    & npm.cmd exec --yes --package '@tauri-apps/cli@2.12.4' -- tauri build --target x86_64-pc-windows-msvc --ci @TauriArgs
    if ($LASTEXITCODE) { throw 'Windows desktop build failed' }
} finally {
    $env:RUSTC_WRAPPER = $previousWrapper
    Pop-Location
}
