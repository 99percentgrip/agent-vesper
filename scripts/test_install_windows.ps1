# Offline, real-process installer regression. Run with powershell.exe (Desktop)
# and pwsh.exe on a disposable Windows CI runner; no release/network calls.
$ErrorActionPreference = 'Stop'
$installer = Join-Path $PSScriptRoot 'install.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) ('vesper-install-test-' + [guid]::NewGuid().ToString('N'))
$oldUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$oldArch = $env:PROCESSOR_ARCHITECTURE
$oldWow = $env:PROCESSOR_ARCHITEW6432
$oldInstall = $env:AGENT_VESPER_INSTALL_DIR
$oldMemory = $env:AGENT_VESPER_MEMORY_ROOT
$oldPath = $env:Path
$oldTls = [Net.ServicePointManager]::SecurityProtocol

function Invoke-WebRequest {
    param([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing)
    if (-not $UseBasicParsing) { throw 'missing UseBasicParsing' }
    $file = $Uri.Substring($Uri.LastIndexOf('/') + 1)
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $OutFile
}
function Expect-Failure {
    param([scriptblock]$Run, [string]$Message)
    try { & $Run | Out-Null } catch {
        if ($_.Exception.Message -notlike "*$Message*") { throw "unexpected failure: $_" }
        return
    }
    throw "expected failure: $Message"
}

try {
    New-Item -ItemType Directory -Path $root, (Join-Path $root 'package/agent-vesper-acp') -Force | Out-Null
    $package = Join-Path $root 'package/agent-vesper-acp'
    # The release's two executable names, launcher behavior and --version are
    # exercised, without making network calls or installing a user release.
    $exe = Join-Path $root 'fixture.exe'
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'
    if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw 'Windows .NET Framework C# compiler unavailable on test runner' }
    $source = Join-Path $root 'fixture.cs'
    Set-Content -LiteralPath $source -Value 'public class Fixture { public static int Main(string[] args) { if (args.Length == 1 && args[0] == "--version") { System.Console.WriteLine("fixture 0.0.0"); return 0; } return 1; } }'
    & $compiler /nologo "/out:$exe" $source
    if ($LASTEXITCODE -ne 0) { throw 'fixture compilation failed' }
    Copy-Item -LiteralPath $exe -Destination (Join-Path $package 'agent-vesper-acp.exe')
    Copy-Item -LiteralPath $exe -Destination (Join-Path $package 'agent-vesper-tui.exe')
    $asset = 'agent-vesper-acp-windows-x86_64.zip'
    $archive = Join-Path $root $asset
    Compress-Archive -Path $package -DestinationPath $archive
    $sha = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash
    Set-Content -LiteralPath "$archive.sha256" -Value "$sha  $asset"
    $env:AGENT_VESPER_INSTALL_DIR = Join-Path $root 'installed'
    $env:AGENT_VESPER_MEMORY_ROOT = Join-Path $root 'memory'
    $env:PROCESSOR_ARCHITECTURE = 'AMD64'
    Remove-Item Env:PROCESSOR_ARCHITEW6432 -ErrorAction SilentlyContinue

    # Explicit unsupported/unknown platform before a download or disk install.
    $env:PROCESSOR_ARCHITECTURE = 'ARM64'
    Expect-Failure { & $installer } 'unsupported Windows architecture: ARM64'
    $env:PROCESSOR_ARCHITECTURE = 'x86'
    Expect-Failure { & $installer } 'unsupported Windows architecture: x86'
    Remove-Item Env:PROCESSOR_ARCHITECTURE
    Expect-Failure { & $installer } 'unable to determine Windows architecture'
    $env:PROCESSOR_ARCHITECTURE = 'AMD64'

    # Tampered and malformed checksum must fail before any installation.
    Set-Content -LiteralPath "$archive.sha256" -Value "$('0' * 64)  $asset"
    Expect-Failure { & $installer } 'SHA-256 verification failed'
    if (Test-Path $env:AGENT_VESPER_INSTALL_DIR) { throw 'checksum failure installed payload' }
    Set-Content -LiteralPath "$archive.sha256" -Value 'garbage'
    Expect-Failure { & $installer } 'invalid SHA-256 checksum file'
    Set-Content -LiteralPath "$archive.sha256" -Value "$sha  $asset"

    # Direct execution and the equivalent local text | iex path (remote main
    # cannot exercise an unmerged change). Both must install usable launchers.
    & $installer | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'direct installer did not verify executables' }
    $acp = Join-Path $env:AGENT_VESPER_INSTALL_DIR 'agent-vesper-acp.cmd'
    $tui = Join-Path $env:AGENT_VESPER_INSTALL_DIR 'agent-vesper-tui.cmd'
    if ((& $acp --version) -ne 'fixture 0.0.0' -or (& $tui --version) -ne 'fixture 0.0.0') { throw 'launchers failed' }

    # A missing-DLL loader exit must be diagnosed before replacing installed
    # payloads or touching user memory. A fixture returns the actual NTSTATUS.
    $installedExe = Join-Path $env:AGENT_VESPER_INSTALL_DIR 'agent-vesper-acp.bundle/agent-vesper-acp.exe'
    $installedHash = (Get-FileHash -Algorithm SHA256 $installedExe).Hash
    $brokenSource = Join-Path $root 'broken.cs'
    $brokenExe = Join-Path $root 'broken.exe'
    Set-Content -LiteralPath $brokenSource -Value 'public class Broken { public static int Main() { return unchecked((int)0xC0000135); } }'
    & $compiler /nologo "/out:$brokenExe" $brokenSource
    if ($LASTEXITCODE -ne 0) { throw 'broken fixture compilation failed' }
    Copy-Item -LiteralPath $brokenExe -Destination (Join-Path $package 'agent-vesper-acp.exe') -Force
    Compress-Archive -Path $package -DestinationPath $archive -Force
    $brokenSha = (Get-FileHash -Algorithm SHA256 $archive).Hash
    Set-Content -LiteralPath "$archive.sha256" -Value "$brokenSha  $asset"
    Expect-Failure { & $installer } '0xC0000135: required DLL missing'
    if ((Get-FileHash -Algorithm SHA256 $installedExe).Hash -ne $installedHash) { throw 'failed preflight replaced working executable' }
    if (Test-Path $env:AGENT_VESPER_MEMORY_ROOT) { throw 'failed preflight wrote user memory' }
    Copy-Item -LiteralPath $exe -Destination (Join-Path $package 'agent-vesper-acp.exe') -Force
    Compress-Archive -Path $package -DestinationPath $archive -Force
    $sha = (Get-FileHash -Algorithm SHA256 $archive).Hash
    Set-Content -LiteralPath "$archive.sha256" -Value "$sha  $asset"

    Get-Content -LiteralPath $installer -Raw | Invoke-Expression | Out-Null
    if ((& $acp --version) -ne 'fixture 0.0.0' -or (& $tui --version) -ne 'fixture 0.0.0') { throw 'piped install failed' }

    # WOW64 emulation: native architecture takes precedence over x86 process.
    $env:PROCESSOR_ARCHITECTURE = 'x86'
    $env:PROCESSOR_ARCHITEW6432 = 'AMD64'
    & $installer | Out-Null
    if ((& $tui --version) -ne 'fixture 0.0.0') { throw 'WOW64 install failed' }
    Write-Host 'PASS: PowerShell installer offline direct/piped/WOW64, launchers and checksum guards'
} finally {
    [Environment]::SetEnvironmentVariable('Path', $oldUserPath, 'User')
    $env:Path = $oldPath
    $env:PROCESSOR_ARCHITECTURE = $oldArch
    $env:PROCESSOR_ARCHITEW6432 = $oldWow
    $env:AGENT_VESPER_INSTALL_DIR = $oldInstall
    $env:AGENT_VESPER_MEMORY_ROOT = $oldMemory
    [Net.ServicePointManager]::SecurityProtocol = $oldTls
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
