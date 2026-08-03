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

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($architecture -ne "X64") {
    throw "agent-vesper installer: unsupported Windows architecture: $architecture"
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
    $archive = Join-Path $temporary $asset
    $checksum = "$archive.sha256"
    Write-Host "Downloading $asset..."
    Invoke-WebRequest -Uri "$downloadRoot/$asset" -OutFile $archive
    Invoke-WebRequest -Uri "$downloadRoot/$asset.sha256" -OutFile $checksum

    $expected = ((Get-Content -Raw $checksum).Trim() -split "\s+")[0].ToUpperInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToUpperInvariant()
    if ($actual -ne $expected) {
        throw "agent-vesper installer: SHA-256 verification failed"
    }

    Expand-Archive -Path $archive -DestinationPath $temporary -Force
    $source = Join-Path $temporary "agent-vesper-acp"
    if (-not (Test-Path -LiteralPath $source -PathType Container)) {
        throw "agent-vesper installer: archive did not contain agent-vesper-acp bundle"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $source "agent-vesper-tui.exe") -PathType Leaf)) {
        throw "agent-vesper installer: archive did not contain agent-vesper-tui.exe"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $bundle = Join-Path $InstallDir "agent-vesper-acp.bundle"
    Remove-Item -LiteralPath $bundle -Recurse -Force -ErrorAction SilentlyContinue
    Move-Item -LiteralPath $source -Destination $bundle
    $launcher = Join-Path $InstallDir "agent-vesper-acp.cmd"
    $launcherContent = "@echo off`r`n`"%~dp0agent-vesper-acp.bundle\agent-vesper-acp.exe`" %*"
    Set-Content -LiteralPath $launcher -Value $launcherContent -NoNewline
    $tuiLauncher = Join-Path $InstallDir "agent-vesper-tui.cmd"
    $tuiLauncherContent = "@echo off`r`n`"%~dp0agent-vesper-acp.bundle\agent-vesper-tui.exe`" %*"
    Set-Content -LiteralPath $tuiLauncher -Value $tuiLauncherContent -NoNewline

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ";" | Where-Object { $_ })
    if ($pathEntries -notcontains $InstallDir) {
        $updatedPath = (@($pathEntries) + $InstallDir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $updatedPath, "User")
    }
    if (($env:Path -split ";") -notcontains $InstallDir) {
        $env:Path = "$InstallDir;$env:Path"
    }

    $installedVersion = & $launcher --version 2>&1
    $tuiVersion = & $tuiLauncher --version 2>&1
    Write-Host "Installed Agent Vesper (${installedVersion}; ${tuiVersion}):"
    Write-Host "  $launcher"
    Write-Host "  $tuiLauncher"
    Write-Host ""
    Write-Host "Next:"
    Write-Host "  agent-vesper-tui                  (launch; Auth Hub opens if needed)"
    Write-Host "  agent-vesper-acp --setup          (optional non-interactive setup)"
    Write-Host "  set ZAI_API_KEY=<your Z.ai key>   (optional environment override)"
    Write-Host ""
    Write-Host "Then register Agent Vesper as an ACP agent in Zed (see README 'Install in Zed')."
} finally {
    Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
}
