$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class VesperJobObservation {
    [StructLayout(LayoutKind.Sequential)] struct Basic {
        public long ProcessTime, JobTime;
        public uint Flags;
        public UIntPtr MinimumWorkingSet, MaximumWorkingSet;
        public uint ActiveProcesses;
        public UIntPtr Affinity;
        public uint Priority, Scheduling;
    }
    [StructLayout(LayoutKind.Sequential)] struct IoCounters {
        public ulong ReadOperations, WriteOperations, OtherOperations;
        public ulong ReadBytes, WriteBytes, OtherBytes;
    }
    [StructLayout(LayoutKind.Sequential)] struct Extended {
        public Basic Basic;
        public IoCounters Io;
        public UIntPtr ProcessLimit, JobLimit, PeakProcess, PeakJob;
    }
    [StructLayout(LayoutKind.Sequential)] struct MemoryCounters {
        public uint Size, Faults;
        public UIntPtr PeakWorkingSet, WorkingSet, PeakPagedPool, PagedPool;
        public UIntPtr PeakNonPagedPool, NonPagedPool, Pagefile, PeakPagefile, Private;
    }
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool inJob);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("psapi.dll", SetLastError=true)]
    static extern bool GetProcessMemoryInfo(IntPtr process, out MemoryCounters info, uint size);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool QueryInformationJobObject(IntPtr job, int kind, out Extended info, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError=true, EntryPoint="QueryInformationJobObject")]
    static extern bool QueryProcessIds(IntPtr job, int kind, IntPtr info, uint size, IntPtr returned);
    static Extended Limits() {
        Extended info;
        if (!QueryInformationJobObject(IntPtr.Zero, 9, out info, (uint)Marshal.SizeOf(typeof(Extended)), IntPtr.Zero))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return info;
    }
    static List<uint> Processes() {
        for (int size=4096; size<=1048576; size*=2) {
            IntPtr buffer=Marshal.AllocHGlobal(size);
            try {
                if (!QueryProcessIds(IntPtr.Zero, 3, buffer, (uint)size, IntPtr.Zero)) {
                    int error=Marshal.GetLastWin32Error();
                    if (error == 234) continue;
                    throw new Win32Exception(error);
                }
                uint count=unchecked((uint)Marshal.ReadInt32(buffer, 4));
                if (count > (size-8)/IntPtr.Size) throw new InvalidOperationException("Invalid job process inventory");
                var pids=new List<uint>();
                for (int i=0; i<count; i++) {
                    long raw=Marshal.ReadIntPtr(buffer, 8+i*IntPtr.Size).ToInt64();
                    if (raw <= 0 || raw > uint.MaxValue) throw new InvalidOperationException("Invalid job process identity");
                    pids.Add((uint)raw);
                }
                pids.Sort(); return pids;
            } finally { Marshal.FreeHGlobal(buffer); }
        }
        throw new InvalidOperationException("Job process inventory exceeds bounds");
    }
    public static string Read() {
        bool inJob;
        if (!IsProcessInJob(GetCurrentProcess(), IntPtr.Zero, out inJob))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        if (!inJob) return "{\"in_job\":false,\"limit\":0,\"current_bound\":0}";
        Extended info=Limits();
        if ((info.Basic.Flags & 0x300) == 0)
            return "{\"in_job\":true,\"limit\":0,\"current_bound\":0}";
        // Current private commitment, not historical peaks: resource deferral
        // must recover when the actual job's pressure has settled.
        for (int attempt=0; attempt<3; attempt++) {
            try {
                var before=Processes();
                ulong total=0, maximum=0;
                foreach (uint pid in before) {
                    IntPtr process=OpenProcess(0x1000, false, pid);
                    if (process == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
                    try {
                        MemoryCounters memory;
                        if (!GetProcessMemoryInfo(process, out memory, (uint)Marshal.SizeOf(typeof(MemoryCounters))))
                            throw new Win32Exception(Marshal.GetLastWin32Error());
                        ulong committed=memory.Private.ToUInt64();
                        total=checked(total+committed); maximum=Math.Max(maximum, committed);
                    } finally { CloseHandle(process); }
                }
                var after=Processes();
                if (before.Count != after.Count) continue;
                bool same=true;
                for (int i=0; i<before.Count; i++) if (before[i] != after[i]) same=false;
                if (!same) continue;
                info=Limits();
                ulong limit=ulong.MaxValue, remaining=ulong.MaxValue;
                if ((info.Basic.Flags & 0x200) != 0) {
                    ulong cap=info.JobLimit.ToUInt64();
                    limit=Math.Min(limit,cap); remaining=Math.Min(remaining,total>=cap?0:cap-total);
                }
                if ((info.Basic.Flags & 0x100) != 0) {
                    ulong cap=info.ProcessLimit.ToUInt64();
                    limit=Math.Min(limit,cap); remaining=Math.Min(remaining,maximum>=cap?0:cap-maximum);
                }
                if (limit == ulong.MaxValue) return "{\"in_job\":true,\"limit\":0,\"current_bound\":0}";
                return "{\"in_job\":true,\"limit\":" + limit + ",\"current_bound\":" + (limit-Math.Min(limit,remaining)) + "}";
            } catch (Win32Exception) { if (attempt == 2) throw; }
        }
        throw new InvalidOperationException("Job accounting did not settle");
    }
}
'@
while ($null -ne [Console]::ReadLine()) {
    [Console]::WriteLine([VesperJobObservation]::Read())
    [Console]::Out.Flush()
}
