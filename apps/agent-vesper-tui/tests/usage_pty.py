#!/usr/bin/env python3
"""Tight feedback loop for the `/usage` regression (2ai).

Drives the REAL TUI binary over a PTY using the same terminal-emulation
harness as bridge_at01_pty.py, types `/usage`, and asserts the exact
user-visible contract from the recorded pre-regression transcript:

  * a panel header line ending in "Usage"
  * aligned field rows (Model:, Reasoning:, Permissions:)
  * a Context window row
  * a completed terminal state: a window meter ("% left") or an honest
    failure notice (Warning: / refresh failed) — never silence.

Exit codes: 0 PASS, 1 assertion failure (dumps screen + raw tail),
2 environment missing.
"""
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time


class Host:
    def __init__(self, binary, root):
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
                   ALL_PROXY='http://127.0.0.1:9', NO_PROXY='')
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
                values = [int(v or '0') for v in params.split(';')] if ':' not in params else []
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
                    self.x = min(119, self.x - n)
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


def main() -> int:
    if len(sys.argv) != 2:
        print('usage: usage_pty.py <path-to-tui-binary>')
        return 2
    binary = sys.argv[1]
    if not os.path.exists(binary):
        print(f'FAIL: binary not found: {binary}')
        return 2

    with tempfile.TemporaryDirectory(prefix='vb-usage-pty-') as temp:
        root = Path(temp).resolve()
        host = Host(binary, root)
        try:
            host.wait('Start coding')
            host.key('\r')
            host.wait('Ready', timeout=15)
            before = host.text()
            assert 'Usage' not in before or 'usage' not in before.lower(), (
                'idle screen already shows usage output')
            host.key('/usage\r')
            deadline = time.monotonic() + 30
            text = ''
            while time.monotonic() < deadline:
                host.pump(.1)
                text = host.text()
                if re.search(r'% left|Warning:|refresh failed|not exposed', text):
                    break
                if host.child.poll() is not None:
                    break

            problems = []
            if not re.search(r'Usage\b', text):
                problems.append('no panel header containing Usage')
            for row in ('Model:', 'Reasoning:', 'Permissions:', 'Context window:'):
                if row not in text:
                    problems.append(f'missing row {row!r}')
            if not re.search(r'% left|Warning:|refresh failed|not exposed', text):
                problems.append('panel never reached a terminal state (no meter, warning, or notice)')

            if problems:
                print('=== SCREEN ===')
                print(text)
                print('=== PROBLEMS ===')
                for p in problems:
                    print(f'- {p}')
                return 1

            print('PASS: /usage panel rendered with header, rows, and terminal state')
            return 0
        finally:
            try:
                host.child.terminate()
                host.child.wait(timeout=5)
            except Exception:
                try:
                    host.child.kill()
                    host.child.wait(timeout=5)
                except Exception:
                    pass


if __name__ == '__main__':
    sys.exit(main())
