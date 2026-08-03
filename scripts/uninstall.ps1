param(
    [string]$InstallDir = $(if ($env:AGENT_VESPER_INSTALL_DIR) { $env:AGENT_VESPER_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\AgentVesper" })
)

# Agent Vesper uninstaller (Windows) — removes the exact launcher, bundled ACP
# binary, and user PATH entry created by install.ps1. Provider credentials are
# not touched.
$ErrorActionPreference = "Stop"

$fullInstallDir = [System.IO.Path]::GetFullPath($InstallDir)
$installRoot = [System.IO.Path]::GetPathRoot($fullInstallDir)
if ([string]::IsNullOrWhiteSpace($fullInstallDir) -or [string]::Equals($fullInstallDir.TrimEnd("\"), $installRoot.TrimEnd("\"), [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "agent-vesper uninstaller: refusing to remove an empty or root install directory"
}
$fullInstallDir = $fullInstallDir.TrimEnd([System.IO.Path]::DirectorySeparatorChar)

$bundle = Join-Path $fullInstallDir "agent-vesper-acp.bundle"
$launcher = Join-Path $fullInstallDir "agent-vesper-acp.cmd"
$tuiLauncher = Join-Path $fullInstallDir "agent-vesper-tui.cmd"
$removed = @()

if (Test-Path -LiteralPath $launcher) {
    Remove-Item -LiteralPath $launcher -Force
    $removed += $launcher
}
if (Test-Path -LiteralPath $tuiLauncher) {
    Remove-Item -LiteralPath $tuiLauncher -Force
    $removed += $tuiLauncher
}
if (Test-Path -LiteralPath $bundle -PathType Container) {
    Remove-Item -LiteralPath $bundle -Recurse -Force
    $removed += $bundle
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($null -ne $userPath) {
    $pathEntries = @($userPath -split ";" | Where-Object { $_ })
    $filteredPath = @($pathEntries | Where-Object {
        [string]::Compare($_.TrimEnd("\"), $fullInstallDir, $true) -ne 0
    })
    if (($filteredPath -join ";") -ne ($pathEntries -join ";")) {
        [Environment]::SetEnvironmentVariable("Path", ($filteredPath -join ";"), "User")
    }
}

if (Test-Path -LiteralPath $fullInstallDir -PathType Container) {
    $remaining = @(Get-ChildItem -LiteralPath $fullInstallDir -Force)
    if ($remaining.Count -eq 0) {
        Remove-Item -LiteralPath $fullInstallDir -Force
    }
}

Write-Host "Agent Vesper uninstall complete."
foreach ($path in $removed) {
    Write-Host "  Removed $path"
}
Write-Host "  Provider credentials were preserved."
