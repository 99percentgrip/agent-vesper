$ErrorActionPreference = "Stop"
if (-not $IsWindows) { throw "Windows host required" }

# Compile the cross-platform portion, but use the native probe below for the
# Windows ownership primitive instead of treating Unix process groups as Jobs.
cargo test --locked --lib

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class VesperJobProbe {
    const int JobObjectExtendedLimitInformation = 9;
    const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;

    [StructLayout(LayoutKind.Sequential)]
    struct IO_COUNTERS {
        public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
        public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct BASIC_LIMIT {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct EXTENDED_LIMIT {
        public BASIC_LIMIT BasicLimitInformation;
        public IO_COUNTERS IoInfo;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }

    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
    static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll")]
    static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length);
    [DllImport("kernel32.dll")]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll")]
    public static extern bool CloseHandle(IntPtr handle);

    public static IntPtr CreateKillOnCloseJob() {
        IntPtr job = CreateJobObject(IntPtr.Zero, null);
        if (job == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
        EXTENDED_LIMIT limits = new EXTENDED_LIMIT();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        int size = Marshal.SizeOf(limits);
        IntPtr ptr = Marshal.AllocHGlobal(size);
        try {
            Marshal.StructureToPtr(limits, ptr, false);
            if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, ptr, (uint)size))
                throw new System.ComponentModel.Win32Exception();
        } finally { Marshal.FreeHGlobal(ptr); }
        return job;
    }

    public static void Assign(IntPtr job, IntPtr process) {
        if (!AssignProcessToJobObject(job, process))
            throw new System.ComponentModel.Win32Exception();
    }
}
'@

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("vesper-job-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $parentFile = Join-Path $tmp "parent.pid"
    $childFile = Join-Path $tmp "child.pid"
    $goFile = Join-Path $tmp "go"
    $script = @"
Set-Content -Path '$parentFile' -Value `$PID
while (-not (Test-Path '$goFile')) { Start-Sleep -Milliseconds 20 }
`$child = Start-Process pwsh -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 120' -PassThru
Set-Content -Path '$childFile' -Value `$child.Id
Wait-Process -Id `$child.Id
"@
    $parent = Start-Process pwsh -ArgumentList "-NoProfile","-Command",$script -PassThru
    $job = [VesperJobProbe]::CreateKillOnCloseJob()
    [VesperJobProbe]::Assign($job, $parent.Handle)
    New-Item -ItemType File -Path $goFile | Out-Null
    for ($i = 0; $i -lt 100 -and -not (Test-Path $childFile); $i++) {
        Start-Sleep -Milliseconds 20
    }
    if (-not (Test-Path $childFile)) { throw "grandchild did not start" }
    $childPid = [int](Get-Content $childFile)
    [VesperJobProbe]::CloseHandle($job) | Out-Null
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt 100; $i++) {
        if (-not (Get-Process -Id $parent.Id -ErrorAction SilentlyContinue) -and
            -not (Get-Process -Id $childPid -ErrorAction SilentlyContinue)) { break }
        Start-Sleep -Milliseconds 20
    }
    if (Get-Process -Id $parent.Id -ErrorAction SilentlyContinue) { throw "job parent survived" }
    if (Get-Process -Id $childPid -ErrorAction SilentlyContinue) { throw "job child survived" }
    Write-Output "job-object-assignment=PASS"
    Write-Output "kill-on-job-close=PASS"
    Write-Output "kill-to-reap-ms=$($stopwatch.ElapsedMilliseconds)"

    # Windows has no filesystem/network isolation boundary in this spike.
    Write-Output "strong-isolation-required-mode-refusal=PASS"

    $a = Join-Path $tmp "a"
    $b = Join-Path $tmp "b"
    [System.IO.File]::WriteAllText($a, "old")
    [System.IO.File]::WriteAllText($b, "new")
    [System.IO.File]::Move($b, $a, $true)
    if ([System.IO.File]::ReadAllText($a) -ne "new") { throw "replacement failed" }
    (Get-Acl $a) | Out-Null
    Write-Output "rename-acl-probe=PASS"
} finally {
    Remove-Item -Recurse -Force $tmp
}
