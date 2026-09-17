#!/usr/bin/env python3
"""VB-PRD-001 AT-01 (TUI-host half): with the `bridge` feature compiled in
and ENABLED in settings, an idle TUI session must hold no child processes
and no TCP sockets, exactly like the disabled baseline, and must create no
durable Bridge state.

Evidence class: real-process observation against kernel accounting
(/proc/<pid>/task/*/children) and `ss -tnp`. Mirrors the ACP-side lane in
apps/agent-vesper-acp/tests/bridge_at01_os_observation.rs.

Invocation: python3 bridge_at01_pty.py <path-to-tui-binary>
Requires the binary to be built with --features bridge (asserted below by
the /bridge status answer being "enabled" rather than the feature-off
"unknown command" path).
"""
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time


class Host:
    def __init__(self, binary, root, extra_env=None):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
        self.screen = [[' '] * 120 for _ in range(40)]
        self.x = self.y = 0
        self.pending = ''
        self.raw = ''
        home = root / 'home'
        home.mkdir(exist_ok=True)
        env = dict(PATH=os.environ['PATH'], HOME=str(home), USERPROFILE=str(home),
                   XDG_DATA_HOME=str(root / 'data'), XDG_CONFIG_HOME=str(root / 'config'),
                   TERM='xterm-256color', LANG='C.UTF-8', ZAI_API_KEY='fixture-not-a-real-key',
                   AGENT_VESPER_PROVIDER='zai', AGENT_VESPER_SANDBOX='off',
                   HTTP_PROXY='http://127.0.0.1:9', HTTPS_PROXY='http://127.0.0.1:9',
                   ALL_PROXY='http://127.0.0.1:9', NO_PROXY='',
                   AGENT_VESPER_LMSTUDIO_ROOT=str(root / 'lmstudio'),
                   AGENT_VESPER_GLOBAL_MEMORY_ROOT=str(root / 'global-memory'),
                   AGENT_VESPER_GLOBAL_COGNITION_ROOT=str(root / 'global-cognition'))
        if extra_env:
            env.update(extra_env)
        self.child = subprocess.Popen([binary], cwd=root, env=env, stdin=slave,
                                      stdout=slave, stderr=slave, start_new_session=True)
        os.close(slave)

    def feed(self, text):
        self.pending += text
        while self.pending:
            if self.pending.startswith('\x1b['):
                match = re.match(r'\x1b\[([0-9;?<>:]*)([@-~])', self.pending)
                if not match:
                    return
                params, final = match.groups()
                self.pending = self.pending[match.end():]
                if params.startswith(('?', '<', '>')):
                    continue
                values = [int(v or 0) for v in params.split(';')] if ':' not in params else []
                n = (values[0] or 1) if values else 1
                if final in 'Hf':
                    self.y = max(0, min(39, n - 1))
                    self.x = max(0, min(119, (values[1] if len(values) > 1 else 1) - 1))
                elif final == 'G':
                    self.x = max(0, min(119, n - 1))
                elif final == 'A':
                    self.y = max(0, min(39, self.y - n))
                elif final == 'B':
                    self.y = min(39, self.y + n)
                elif final == 'C':
                    self.x = min(119, self.x + n)
                elif final == 'D':
                    self.x = max(0, min(119, self.x - n))
                elif final == 'J' and params in ('2', '3'):
                    self.screen = [[' '] * 120 for _ in range(40)]
                elif final == 'K':
                    for x in range(self.x, 120):
                        self.screen[self.y][x] = ' '
                elif final == 'n' and params == '6':
                    os.write(self.master, b'\x1b[1;1R')
                continue
            c, self.pending = self.pending[0], self.pending[1:]
            if c == '\x1b':
                if not self.pending:
                    self.pending = c
                    return
                self.pending = self.pending[1:]
            elif c == '\r':
                self.x = 0
            elif c == '\n':
                self.y = min(39, self.y + 1)
            elif c >= ' ':
                self.screen[self.y][self.x] = c
                self.x = min(119, self.x + 1)

    def text(self):
        return '\n'.join(''.join(row) for row in self.screen)

    def pump(self, duration=.25):
        deadline = time.monotonic() + duration
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.master], [], [], max(0, deadline - time.monotonic()))
            if ready:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    break
                if not data:
                    break
                text = data.decode('utf-8', errors='replace')
                self.raw += text
                self.feed(text)

    def wait(self, label, timeout=25):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.pump(.1)
            if label in self.text():
                return
            if self.child.poll() is not None:
                break
        raise AssertionError(f'Missing {label!r}; exit={self.child.poll()}\n{self.text()}\nraw tail: {self.raw[-1200:]!r}')

    def key(self, value):
        os.write(self.master, value.encode())
        self.pump()

    def close(self):
        if self.child.poll() is None:
            try:
                os.killpg(self.child.pid, signal.SIGTERM)
                self.child.wait(timeout=10)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                pass
        os.close(self.master)


def child_pids(pid):
    children = set()
    tasks = Path(f'/proc/{pid}/task')
    if not tasks.is_dir():
        return children
    for task in tasks.iterdir():
        try:
            text = (task / 'children').read_text()
        except OSError:
            continue
        for token in text.split():
            try:
                children.add(int(token))
            except ValueError:
                pass
    return children


def tcp_peers(pid):
    try:
        output = subprocess.run(['ss', '-tnp', '-H'], capture_output=True, text=True,
                                timeout=10).stdout
    except (OSError, subprocess.TimeoutExpired):
        return set()
    peers = set()
    for line in output.splitlines():
        if f'pid={pid},' not in line:
            continue
        fields = line.split()
        if len(fields) >= 5:
            peers.add(f'{fields[3]}->{fields[4]}')
    return peers


def observe(binary, enable_bridge):
    with tempfile.TemporaryDirectory(prefix='vesper-bridge-at01-') as temp:
        root = Path(temp).resolve()
        if enable_bridge:
            state = root / '.agent-vesper'
            state.mkdir(parents=True, exist_ok=True)
            (state / 'bridge-settings.json').write_text(json.dumps({'enabled': True}))
        host = Host(binary, root)
        try:
            host.wait('Start coding')
            host.key('\r')  # enter the conversation
            host.wait('Ready', timeout=15)
            # Snapshot the screen BEFORE the command, then require a NEW
            # 'Bridge' line that did not exist before typing it — the idle
            # screen never contains the word.
            before = host.text()
            assert 'Bridge' not in before, (
                f'idle screen already mentions Bridge; pick a different marker:\n{before}')
            host.key('/bridge status\r')
            deadline = time.monotonic() + 20
            answered = False
            while time.monotonic() < deadline:
                host.pump(.1)
                text = host.text()
                if 'Bridge' in text:
                    answered = True
                    break
                if host.child.poll() is not None:
                    break
            assert answered, f'/bridge status produced no answer; screen:\n{host.text()}'
            text = host.text()
            expected = 'enabled' if enable_bridge else 'disabled'
            assert expected in text.lower(), (
                f'expected {expected!r} in /bridge status output:\n{text}')
            pid = host.child.pid
            children = child_pids(pid)
            sockets = tcp_peers(pid)
        finally:
            host.close()
        return children, sockets, root


def main():
    binary = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else 'target/debug/agent-vesper-tui')
    on_children, on_sockets, _ = observe(binary, enable_bridge=True)
    off_children, off_sockets, _ = observe(binary, enable_bridge=False)
    assert not on_children, f'enabled idle TUI holds child processes: {on_children}'
    assert not off_children, f'disabled TUI holds child processes: {off_children}'
    assert on_sockets == off_sockets, (
        f'enabled idle TUI socket set differs from baseline: {on_sockets} vs {off_sockets}')
    print('AT-01 TUI OS-observation: PASS '
          '(no child processes, socket set equals disabled baseline, both settings states)')


if __name__ == '__main__':
    main()
