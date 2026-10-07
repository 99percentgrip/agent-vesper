param([Parameter(Mandatory = $true)][string]$BinaryDir)

# Run real, exact-candidate release executables through both installer forms in
# private state. No provider, microphone, credentials or user's installation.
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('vesper-real-package-' + [guid]::NewGuid().ToString('N'))
$oldUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$oldPath = $env:Path
$oldInstall = $env:AGENT_VESPER_INSTALL_DIR
$oldMemory = $env:AGENT_VESPER_MEMORY_ROOT
$oldTls = [Net.ServicePointManager]::SecurityProtocol

function Invoke-WebRequest {
    param([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing)
    if (-not $UseBasicParsing) { throw 'missing UseBasicParsing' }
    Copy-Item -LiteralPath (Join-Path $root $Uri.Substring($Uri.LastIndexOf('/') + 1)) -Destination $OutFile
}

try {
    $package = Join-Path $root 'package/agent-vesper-acp'
    New-Item -ItemType Directory -Path $package -Force | Out-Null
    foreach ($executable in @('agent-vesper-acp.exe', 'agent-vesper-tui.exe', 'vesper-web-fetch.exe', 'sandbox_init.exe')) {
        Copy-Item -LiteralPath (Join-Path $BinaryDir $executable) -Destination $package
    }
    $archive = Join-Path $root 'agent-vesper-acp-windows-x86_64.zip'
    Compress-Archive -Path $package -DestinationPath $archive
    $hash = (Get-FileHash -Algorithm SHA256 $archive).Hash
    Set-Content -LiteralPath "$archive.sha256" -Value "$hash  agent-vesper-acp-windows-x86_64.zip"
    $env:AGENT_VESPER_INSTALL_DIR = Join-Path $root 'installed'
    $env:AGENT_VESPER_MEMORY_ROOT = Join-Path $root 'memory'
    $installer = Join-Path $PSScriptRoot 'install.ps1'
    & $installer | Out-Null
    Get-Content -LiteralPath $installer -Raw | Invoke-Expression | Out-Null
    foreach ($hostName in @('agent-vesper-acp', 'agent-vesper-tui')) {
        $launcher = Join-Path $env:AGENT_VESPER_INSTALL_DIR "$hostName.cmd"
        # ACP reserves stdout for protocol traffic and prints CLI metadata on
        # stderr. Desktop 5.1 wraps redirected stderr as NativeCommandError;
        # startup success is determined by the real exit code plus output.
        $checkPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $observedVersion = @(& $launcher --version 2>&1 | ForEach-Object { "$_" })
            $versionExit = $LASTEXITCODE
            & $launcher --help 2>&1 | Out-Null
            $helpExit = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $checkPreference
        }
        if ($versionExit -ne 0 -or -not $observedVersion) { throw "$hostName --version failed (exit $versionExit)" }
        if ($helpExit -ne 0) { throw "$hostName --help failed (exit $helpExit)" }
        Write-Host "PASS real exact-candidate Windows release: $observedVersion"
    }
} finally {
    [Environment]::SetEnvironmentVariable('Path', $oldUserPath, 'User')
    $env:Path = $oldPath
    $env:AGENT_VESPER_INSTALL_DIR = $oldInstall
    $env:AGENT_VESPER_MEMORY_ROOT = $oldMemory
    [Net.ServicePointManager]::SecurityProtocol = $oldTls
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
