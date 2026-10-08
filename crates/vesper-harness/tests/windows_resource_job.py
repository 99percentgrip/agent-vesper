#!/usr/bin/env python3
"""Real Windows constrained-job observation using the production governor.

Private bounded child tree; no provider, release authority, installation or user state.
"""
import ctypes
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import subprocess
import tempfile


def run():
    assert os.name == 'nt', 'this acceptance requires native Windows'
    result = subprocess.run(['cargo', 'test', '--locked', '-p', 'vesper-harness',
                             '--all-features', '--lib', '--no-run', '--message-format=json'],
                            check=True, text=True, capture_output=True, timeout=300)
    executable = next(row['executable'] for line in result.stdout.splitlines()
                      if (row := json.loads(line)).get('reason') == 'compiler-artifact'
                      and row.get('executable') and row['target']['name'] == 'vesper_harness')
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    size = ctypes.c_size_t

    class Basic(ctypes.Structure):
        _fields_ = [('ProcessTime', ctypes.c_int64), ('JobTime', ctypes.c_int64),
                    ('Flags', w.DWORD), ('MinWorking', size), ('MaxWorking', size),
                    ('Active', w.DWORD), ('Affinity', size), ('Priority', w.DWORD), ('Scheduling', w.DWORD)]

    class Io(ctypes.Structure):
        _fields_ = [(name, ctypes.c_uint64) for name in ('ReadOps', 'WriteOps', 'OtherOps', 'ReadBytes', 'WriteBytes', 'OtherBytes')]

    class Extended(ctypes.Structure):
        _fields_ = [('Basic', Basic), ('Io', Io), ('ProcessLimit', size), ('JobLimit', size), ('PeakProcess', size), ('PeakJob', size)]

    class Startup(ctypes.Structure):
        _fields_ = [('cb', w.DWORD), ('reserved', w.LPWSTR), ('desktop', w.LPWSTR), ('title', w.LPWSTR),
                    ('x', w.DWORD), ('y', w.DWORD), ('width', w.DWORD), ('height', w.DWORD),
                    ('chars_x', w.DWORD), ('chars_y', w.DWORD), ('fill', w.DWORD), ('flags', w.DWORD),
                    ('show', w.WORD), ('reserved_size', w.WORD), ('reserved_bytes', ctypes.POINTER(w.BYTE)),
                    ('stdin', w.HANDLE), ('stdout', w.HANDLE), ('stderr', w.HANDLE)]

    class Process(ctypes.Structure):
        _fields_ = [('process', w.HANDLE), ('thread', w.HANDLE), ('pid', w.DWORD), ('tid', w.DWORD)]

    signatures = {
        'CreateJobObjectW': (w.HANDLE, [ctypes.c_void_p, w.LPCWSTR]),
        'SetInformationJobObject': (w.BOOL, [w.HANDLE, ctypes.c_int, ctypes.c_void_p, w.DWORD]),
        'AssignProcessToJobObject': (w.BOOL, [w.HANDLE, w.HANDLE]),
        'CreateProcessW': (w.BOOL, [w.LPCWSTR, w.LPWSTR, ctypes.c_void_p, ctypes.c_void_p, w.BOOL, w.DWORD,
                                  ctypes.c_void_p, w.LPCWSTR, ctypes.POINTER(Startup), ctypes.POINTER(Process)]),
        'ResumeThread': (w.DWORD, [w.HANDLE]),
        'WaitForSingleObject': (w.DWORD, [w.HANDLE, w.DWORD]),
        'GetExitCodeProcess': (w.BOOL, [w.HANDLE, ctypes.POINTER(w.DWORD)]),
        'TerminateJobObject': (w.BOOL, [w.HANDLE, w.UINT]),
        'TerminateProcess': (w.BOOL, [w.HANDLE, w.UINT]),
        'CloseHandle': (w.BOOL, [w.HANDLE]),
    }
    for name, (restype, argtypes) in signatures.items():
        function = getattr(kernel, name)
        function.restype, function.argtypes = restype, argtypes

    def check(value):
        if not value:
            raise ctypes.WinError(ctypes.get_last_error())
        return value

    job = check(kernel.CreateJobObjectW(None, None))
    process = Process()
    started = False
    with tempfile.TemporaryDirectory(prefix='vesper-windows-job-') as temporary:
        root = Path(temporary)
        output = root / 'output.log'
        import msvcrt
        with output.open('wb') as stream, open(os.devnull, 'rb') as null:
            out_handle = msvcrt.get_osfhandle(stream.fileno())
            in_handle = msvcrt.get_osfhandle(null.fileno())
            os.set_handle_inheritable(out_handle, True)
            os.set_handle_inheritable(in_handle, True)
            info = Extended()
            info.Basic.Flags = 0x200 | 0x2000  # Memory cap, kill this owned job on close.
            info.JobLimit = 2 * 1024**3
            startup = Startup()
            startup.cb, startup.flags = ctypes.sizeof(Startup), 0x100
            startup.stdin, startup.stdout, startup.stderr = in_handle, out_handle, out_handle
            command = ctypes.create_unicode_buffer(subprocess.list2cmdline([
                executable, '--exact', 'host_resources::native_backend::tests::windows_job_fixture', '--nocapture']))
            env = dict(os.environ, AGENT_VESPER_WINDOWS_JOB_FIXTURE='1', TEMP=str(root), TMP=str(root))
            environment = ctypes.create_unicode_buffer('\0'.join(f'{key}={value}' for key, value in sorted(env.items())) + '\0\0')
            try:
                check(kernel.SetInformationJobObject(job, 9, ctypes.byref(info), ctypes.sizeof(info)))
                check(kernel.CreateProcessW(None, command, None, None, True, 0x4 | 0x400,
                                            environment, str(root), ctypes.byref(startup), ctypes.byref(process)))
                started = True
                check(kernel.AssignProcessToJobObject(job, process.process))
                assert kernel.ResumeThread(process.thread) != 0xffffffff
                assert kernel.WaitForSingleObject(process.process, 60000) == 0, 'native resource fixture exceeded deadline'
                code = w.DWORD()
                check(kernel.GetExitCodeProcess(process.process, ctypes.byref(code)))
                stream.flush()
                text = output.read_text(errors='replace')
                assert code.value == 0, text
                marker = next(line.split('WINDOWS_JOB_OBSERVATION=', 1)[1] for line in text.splitlines()
                              if 'WINDOWS_JOB_OBSERVATION=' in line)
                observed = json.loads(marker)
                assert observed['native_memory_limit_bytes'] == 2 * 1024**3
                assert observed['native_memory_current_bound_bytes'] > 0
                assert observed['pressure'] != 'normal'
                print('PASS: real 2 GiB Windows Job Object is measured by the production governor and blocks unsafe Cargo admission')
            finally:
                if started:
                    kernel.TerminateJobObject(job, 1)
                    kernel.TerminateProcess(process.process, 1)
                    kernel.WaitForSingleObject(process.process, 10000)
                    kernel.CloseHandle(process.thread)
                    kernel.CloseHandle(process.process)
                kernel.CloseHandle(job)


if __name__ == '__main__':
    run()
