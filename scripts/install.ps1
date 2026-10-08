param(
    [string]$Version = $(if ($env:AGENT_VESPER_VERSION) { $env:AGENT_VESPER_VERSION } else { "latest" }),
    [string]$InstallDir = $(if ($env:AGENT_VESPER_INSTALL_DIR) { $env:AGENT_VESPER_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\AgentVesper" })
)

# Agent Vesper installer (Windows) — downloads the compiled ACP and native TUI
# binaries, verifies the archive SHA-256, installs them under
# `%LOCALAPPDATA%\Programs\AgentVesper`, and adds that directory to the user
# PATH. Mirrors the original Python `native-glm-acp` Windows installer UX.
$ErrorActionPreference = "Stop"
$repository = "99percentgrip/agent-vesper"
$releaseBase = if ($env:AGENT_VESPER_RELEASE_BASE_URL) { $env:AGENT_VESPER_RELEASE_BASE_URL.TrimEnd("/") } else { "https://github.com/$repository/releases" }

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw "agent-vesper installer: Windows is required"
}
if (-not [Environment]::Is64BitOperatingSystem) {
    throw "agent-vesper installer: 64-bit Windows is required"
}
# A 32-bit PowerShell process reports x86 in PROCESSOR_ARCHITECTURE; Windows
# supplies the native architecture in PROCESSOR_ARCHITEW6432 in that case.
$nativeArchitecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if (-not $nativeArchitecture) {
    throw "agent-vesper installer: unable to determine Windows architecture"
}
if ($nativeArchitecture -ne "AMD64") {
    throw "agent-vesper installer: unsupported Windows architecture: $nativeArchitecture (only x86_64 release packages are published)"
}

$asset = "agent-vesper-acp-windows-x86_64.zip"
if ($Version -eq "latest") {
    $downloadRoot = "$releaseBase/latest/download"
} else {
    $tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    $downloadRoot = "$releaseBase/download/$tag"
}

$temporary = Join-Path ([System.IO.Path]::GetTempPath()) ("agent-vesper-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $temporary | Out-Null

try {
    # Windows PowerShell 5.1 defaults to legacy TLS on some Windows images and
    # Invoke-WebRequest otherwise prompts for IE first-run configuration.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $archive = Join-Path $temporary $asset
    $checksum = "$archive.sha256"
    Write-Host "Downloading $asset..."
    Invoke-WebRequest -UseBasicParsing -Uri "$downloadRoot/$asset" -OutFile $archive
    Invoke-WebRequest -UseBasicParsing -Uri "$downloadRoot/$asset.sha256" -OutFile $checksum

    $expected = ((Get-Content -LiteralPath $checksum -Raw).Trim() -split "\s+")[0]
    if ($expected -notmatch '^[a-fA-F0-9]{64}$') {
        throw "agent-vesper installer: invalid SHA-256 checksum file"
    }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToUpperInvariant()
    if ($actual -ne $expected) {
        throw "agent-vesper installer: SHA-256 verification failed"
    }

    Expand-Archive -Path $archive -DestinationPath $temporary -Force
    $source = Join-Path $temporary "agent-vesper-acp"
    if (-not (Test-Path -LiteralPath $source -PathType Container)) {
        throw "agent-vesper installer: archive did not contain agent-vesper-acp bundle"
    }
    foreach ($executable in @("agent-vesper-acp.exe", "agent-vesper-tui.exe")) {
        if (-not (Test-Path -LiteralPath (Join-Path $source $executable) -PathType Leaf)) {
            throw "agent-vesper installer: archive did not contain $executable"
        }
    }

    # Validate the downloaded executables before replacing a working installation
    # or seeding user data. A hosted machine's VC runtime must not hide a broken
    # release package on a clean Windows computer.
    $preflightErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        foreach ($executable in @("agent-vesper-acp.exe", "agent-vesper-tui.exe")) {
            $preflightVersion = & (Join-Path $source $executable) --version 2>&1
            $preflightExitCode = $LASTEXITCODE
            if ($preflightExitCode -eq -1073741515) {
                throw "agent-vesper installer: downloaded $executable cannot start (0xC0000135: required DLL missing). Existing installation was preserved. This release package requires repair."
            }
            if ($preflightExitCode -ne 0 -or -not $preflightVersion) {
                throw "agent-vesper installer: downloaded $executable version preflight failed (exit $preflightExitCode). Existing installation was preserved."
            }
        }
    } finally {
        $ErrorActionPreference = $preflightErrorPreference
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $bundle = Join-Path $InstallDir "agent-vesper-acp.bundle"
    # Preserve user state sharing the bundle root; replace only owned payloads.
    New-Item -ItemType Directory -Path $bundle -Force | Out-Null
    foreach ($payload in @("agent-vesper-acp.exe", "agent-vesper-tui.exe", "vesper-web-fetch.exe", "sandbox_init.exe", "skills", "web-driver")) {
        $incoming = Join-Path $source $payload
        if (Test-Path -LiteralPath $incoming) {
            $destination = Join-Path $bundle $payload
            if (Test-Path -LiteralPath $destination) {
                Remove-Item -LiteralPath $destination -Recurse -Force
            }
            Move-Item -LiteralPath $incoming -Destination $destination
        }
    }
    $launcher = Join-Path $InstallDir "agent-vesper-acp.cmd"
    $launcherContent = "@echo off`r`n`"%~dp0agent-vesper-acp.bundle\agent-vesper-acp.exe`" %*"
    Set-Content -LiteralPath $launcher -Value $launcherContent -NoNewline
    $tuiLauncher = Join-Path $InstallDir "agent-vesper-tui.cmd"
    $tuiLauncherContent = "@echo off`r`n`"%~dp0agent-vesper-acp.bundle\agent-vesper-tui.exe`" %*"
    Set-Content -LiteralPath $tuiLauncher -Value $tuiLauncherContent -NoNewline

    # Seed the curated skill library into the cross-project memory root.
    # Never destructive: existing files win (user edits preserved), slugs in
    # the seed manifest are never resurrected (deletions preserved), and new
    # seed skills from later releases are seeded on upgrade.
    $memoryRoot = if ($env:AGENT_VESPER_MEMORY_ROOT) { $env:AGENT_VESPER_MEMORY_ROOT } else { Join-Path $HOME ".agent-vesper\memory" }
    $seedRoot = Join-Path $bundle "skills"
    $script:seededCount = 0
    if (Test-Path -LiteralPath (Join-Path $seedRoot "skills") -PathType Container) {
        New-Item -ItemType Directory -Path (Join-Path $memoryRoot "skills"), (Join-Path $memoryRoot "bundles") -Force | Out-Null
        $manifest = Join-Path $memoryRoot ".seed-manifest"
        if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
            Set-Content -LiteralPath $manifest -Value ""
        }
        $manifestSlugs = @(Get-Content -LiteralPath $manifest -ErrorAction SilentlyContinue)
        Get-ChildItem -LiteralPath (Join-Path $seedRoot "skills") -Filter "*.md" -File | ForEach-Object {
            $slug = $_.BaseName
            if ($manifestSlugs -contains $slug) { return }
            $dest = Join-Path $memoryRoot "skills\$($_.Name)"
            if (Test-Path -LiteralPath $dest -PathType Leaf) { return }
            Copy-Item -LiteralPath $_.FullName -Destination $dest
            $resDir = Join-Path $seedRoot "skills\$slug"
            $destDir = Join-Path $memoryRoot "skills\$slug"
            if ((Test-Path -LiteralPath $resDir -PathType Container) -and -not (Test-Path -LiteralPath $destDir -PathType Container)) {
                Copy-Item -LiteralPath $resDir -Destination $destDir -Recurse
            }
            Add-Content -LiteralPath $manifest -Value $slug
            $script:seededCount++
        }
        Get-ChildItem -LiteralPath (Join-Path $seedRoot "bundles") -Filter "*.json" -File -ErrorAction SilentlyContinue | ForEach-Object {
            $dest = Join-Path $memoryRoot "bundles\$($_.Name)"
            if (-not (Test-Path -LiteralPath $dest -PathType Leaf)) {
                Copy-Item -LiteralPath $_.FullName -Destination $dest
            }
        }
    }
    Write-Host "Seeded $seededCount skill(s) into $memoryRoot"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ";" | Where-Object { $_ })
    if ($pathEntries -notcontains $InstallDir) {
        $updatedPath = (@($pathEntries) + $InstallDir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $updatedPath, "User")
    }
    if (($env:Path -split ";") -notcontains $InstallDir) {
        $env:Path = "$InstallDir;$env:Path"
    }

    # Windows PowerShell 5.1 promotes native stderr to a PowerShell error
    # under Stop; use the process exit codes for the actual success decision.
    $versionErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $installedVersion = & $launcher --version 2>&1
        $acpExitCode = $LASTEXITCODE
        $tuiVersion = & $tuiLauncher --version 2>&1
        $tuiExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $versionErrorPreference
    }
    if ($acpExitCode -ne 0) { throw "agent-vesper installer: installed ACP version check failed (exit $acpExitCode)" }
    if ($tuiExitCode -ne 0) { throw "agent-vesper installer: installed TUI version check failed (exit $tuiExitCode)" }
    if (-not $installedVersion -or -not $tuiVersion) { throw "agent-vesper installer: installed executable returned no version" }
    Write-Host "Installed Agent Vesper (${installedVersion}; ${tuiVersion}):"
    Write-Host "  $launcher"
    Write-Host "  $tuiLauncher"
    if (Test-Path -LiteralPath (Join-Path $bundle "web-driver\image.tar.gz") -PathType Leaf) {
        Write-Host "Setting up bundled web driver..."
        # The setup CLI writes safe diagnostics to stderr, including success.
        # Windows PowerShell 5.1 must not turn that stream into a fatal error.
        $driverErrorPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            & (Join-Path $bundle "agent-vesper-acp.exe") --setup-web-driver 2>&1 | ForEach-Object { Write-Host "$_" }
        } finally {
            $ErrorActionPreference = $driverErrorPreference
        }
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Web driver is included. Open Settings > Web tools > Set up features / repair for guided runtime installation and browser checks."
        }
    } else {
        Write-Warning "This older release has no bundled web driver; upgrade to a complete driver-bundled release."
    }
    Write-Host ""
    Write-Host "Next:"
    Write-Host "  agent-vesper-tui                  (launch; Settings > Providers for sign-in)"
    Write-Host "  agent-vesper-acp --setup          (optional non-interactive setup)"
    Write-Host "  set ZAI_API_KEY=<your Z.ai key>   (optional environment override)"
    Write-Host ""
    Write-Host "Then register the installed binary as a custom ACP agent in Zed (see README 'Install in Zed')."
    Write-Host "Use 'type': 'custom' until the upstream ACP Registry PR is merged."
} finally {
    Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
}
