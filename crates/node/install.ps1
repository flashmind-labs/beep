# Installs beep-node into %LOCALAPPDATA%\Programs\beep-node and puts it on
# the user's PATH:
#
#   irm https://releases.flashmind.com/beep/node/install.ps1 | iex
#
# BEEP_NODE_VERSION picks a version instead of the latest one.
# BEEP_NODE_BASE replaces the download location, for testing.
#
# It runs in the caller's own PowerShell, so it never calls exit: a failure
# throws, and the shell stays open.

& {
    $ErrorActionPreference = 'Stop'
    # Invoke-WebRequest is many times slower while it draws progress.
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 may not offer TLS 1.2 on its own.
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $base = if ($env:BEEP_NODE_BASE) { $env:BEEP_NODE_BASE.TrimEnd('/') } else { 'https://releases.flashmind.com/beep/node' }
    $bindir = Join-Path $env:LOCALAPPDATA 'Programs\beep-node'
    $exe = Join-Path $bindir 'beep-node.exe'
    $task = 'Beep node'

    function Fail([string] $message) {
        throw "beep-node install: $message"
    }

    function Fetch([string] $url, [string] $out) {
        try {
            Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $out
        } catch {
            Fail "could not download ${url}: $($_.Exception.Message)"
        }
    }

    # An x64 build: Windows on Arm runs it too.
    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($arch -ne 'AMD64' -and $arch -ne 'ARM64') {
        Fail "no build for Windows on $arch"
    }
    $target = 'x86_64-pc-windows-msvc'

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("beep-node-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $version = $env:BEEP_NODE_VERSION
        if (-not $version) {
            Fetch "$base/latest.txt" (Join-Path $tmp 'latest.txt')
            $version = (Get-Content -Raw (Join-Path $tmp 'latest.txt')).Trim()
        }
        $version = $version.Trim().TrimStart('v')
        if ($version -notmatch '^[0-9][0-9.]*$') {
            Fail "bad version '$version'"
        }

        $archive = "beep-node-$version-$target.zip"
        Write-Host "Downloading beep-node $version for $target"
        Fetch "$base/$version/$archive" (Join-Path $tmp $archive)
        Fetch "$base/$version/SHA256SUMS" (Join-Path $tmp 'SHA256SUMS')

        $expected = $null
        foreach ($line in Get-Content (Join-Path $tmp 'SHA256SUMS')) {
            $fields = $line.Trim() -split '\s+'
            if ($fields.Count -eq 2 -and ($fields[1] -eq $archive -or $fields[1] -eq "*$archive")) {
                $expected = $fields[0].ToLowerInvariant()
            }
        }
        if (-not $expected) {
            Fail "$archive is not listed in SHA256SUMS"
        }
        $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            Fail "checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
        }

        $unpacked = Join-Path $tmp 'unpacked'
        # Not Expand-Archive: its progress bar throws in a console it cannot read, as over ssh.
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [IO.Compression.ZipFile]::ExtractToDirectory((Join-Path $tmp $archive), $unpacked)
        $new = Join-Path $unpacked 'beep-node.exe'
        if (-not (Test-Path $new)) {
            Fail "$archive holds no beep-node.exe"
        }

        New-Item -ItemType Directory -Force -Path $bindir | Out-Null
        # A running beep-node.exe cannot be overwritten, but it can be
        # renamed: the old one moves aside and is removed once it stopped.
        $old = "$exe.old"
        Remove-Item -Force -ErrorAction SilentlyContinue $old
        if (Test-Path $exe) {
            Move-Item -Force $exe $old
        }
        Copy-Item $new $exe
        Remove-Item -Force -ErrorAction SilentlyContinue $old
        Write-Host "Installed $exe"
    } finally {
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @($userPath -split ';' | Where-Object { $_ })
    if ($parts -notcontains $bindir) {
        [Environment]::SetEnvironmentVariable('Path', (($parts + $bindir) -join ';'), 'User')
        Write-Host "Added $bindir to your PATH; new terminals pick it up."
    }
    if (($env:Path -split ';') -notcontains $bindir) {
        $env:Path = "$env:Path;$bindir"
    }

    # An upgrade over a node that `beep-node setup` already runs in the
    # background: the scheduled task keeps the old one running until it
    # restarts.
    $scheduled = Get-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue
    if ($scheduled -and ($scheduled.Actions | Where-Object { $_.Arguments -like "*$exe*" })) {
        Stop-ScheduledTask -TaskName $task
        Start-ScheduledTask -TaskName $task
        Remove-Item -Force -ErrorAction SilentlyContinue "$exe.old"
        Write-Host 'Restarted the beep-node service on the new version.'
        return
    }

    Write-Host ''
    Write-Host 'Next, open Add a machine in Beep and run'
    Write-Host '  beep-node setup <token>'
    Write-Host 'with the token it shows. It links this machine and starts beep-node'
    Write-Host 'in the background.'
}
