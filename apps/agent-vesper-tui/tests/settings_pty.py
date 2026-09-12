#!/usr/bin/env python3
"""Linux/macOS native Settings smoke test; stdlib only, isolated state, no prompts."""
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
                   XDG_DATA_HOME=str(root/'data'), XDG_CONFIG_HOME=str(root/'config'),
                   TERM='xterm-256color', LANG='C.UTF-8', ZAI_API_KEY='fixture-not-a-real-key',
                   AGENT_VESPER_PROVIDER='zai', AGENT_VESPER_SANDBOX='off',
                   HTTP_PROXY='http://127.0.0.1:9', HTTPS_PROXY='http://127.0.0.1:9',
                   ALL_PROXY='http://127.0.0.1:9', NO_PROXY='',
                   AGENT_VESPER_LMSTUDIO_ROOT=str(root/'lmstudio'),
                   AGENT_VESPER_GLOBAL_MEMORY_ROOT=str(root/'global-memory'),
                   AGENT_VESPER_GLOBAL_COGNITION_ROOT=str(root/'global-cognition'))
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
                    self.y = max(0, min(39, n-1))
                    self.x = max(0, min(119, (values[1] if len(values)>1 else 1)-1))
                elif final == 'G': self.x = max(0,min(119,n-1))
                elif final == 'A': self.y = max(0,self.y-n)
                elif final == 'B': self.y = min(39,self.y+n)
                elif final == 'C': self.x = min(119,self.x+n)
                elif final == 'D': self.x = max(0,self.x-n)
                elif final == 'J' and params in ('2','3'): self.screen = [[' ']*120 for _ in range(40)]
                elif final == 'K':
                    for x in range(self.x,120): self.screen[self.y][x]=' '
                elif final == 'n' and params == '6': os.write(self.master, b'\x1b[1;1R')
                continue
            c, self.pending = self.pending[0], self.pending[1:]
            if c == '\x1b':
                if not self.pending:
                    self.pending = c
                    return
                self.pending = self.pending[1:]
            elif c == '\r': self.x = 0
            elif c == '\n': self.y = min(39,self.y+1)
            elif c >= ' ':
                self.screen[self.y][self.x] = c
                self.x = min(119,self.x+1)
    def text(self): return '\n'.join(''.join(row) for row in self.screen)
    def pump(self, duration=.25):
        deadline = time.monotonic()+duration
        while time.monotonic()<deadline:
            ready,_,_ = select.select([self.master],[],[],max(0,deadline-time.monotonic()))
            if ready:
                try: data=os.read(self.master,65536)
                except OSError: break
                if not data: break
                text=data.decode('utf-8',errors='replace')
                self.raw+=text
                self.feed(text)
    def wait(self, label, timeout=20):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            self.pump(.1)
            if label in self.text(): return
            if self.child.poll() is not None: break
        raise AssertionError(f'Missing {label!r}; exit={self.child.poll()}\n{self.text()}\nraw tail: {self.raw[-1200:]!r}')
    def key(self, value):
        os.write(self.master,value.encode())
        self.pump()
    def click(self, label):
        self.wait(label)
        for y,row in enumerate(self.screen):
            line=''.join(row)
            if label in line and line.strip().startswith("│"):
                x=line.index(label)+1
                self.key(f'\x1b[<0;{x+1};{y+1}M')
                return
        raise AssertionError(label)
    def close(self):
        if self.child.poll() is None:
            os.killpg(self.child.pid,signal.SIGTERM)
            self.child.wait(timeout=10)
        os.close(self.master)


def run(binary):
    with tempfile.TemporaryDirectory(prefix='vesper-settings-pty-') as temp:
        root=Path(temp)
        saved=root/'home/.agent-vesper/ui/settings.json'
        host=Host(binary,root)
        try:
            host.wait('Start coding')
            host.key('s')
            host.click('Permissions')
            host.click('bypass')
            assert not saved.exists()
            host.key('\x1b')
            host.click('Discard changes')
            host.wait('Start coding')
            assert not saved.exists()
            host.key('s')
            host.wait('current Ask')
            host.click('Providers')
            host.click('openai')
            host.click('Cancel')
            assert not (root/'.agent-vesper/provider').exists()
            for category,value in [('Primary model','glm-5.2'),('Reasoning depth','high'),
                                   ('Auxiliary model','glm-4.7'),('Generation style','precise'),
                                   ('Mixture of Agents','enabled'),('Permissions','bypass'),
                                   ('Session mode','ask'),('Visual theme','dracula')]:
                host.click(category)
                host.click(value)
            host.click('Implementation acceptance')
            host.click('Enforced completion: OFF')
            host.wait('Enforced completion: ON')
            assert 'PRD path:' not in host.text()
            host.key('\x1b')
            host.click('Swarm')
            host.click('Swarm: OFF')
            host.wait('Swarm: ON')
            host.key('\x1b')
            host.click('Web tools')
            host.click('Web tools: OFF')
            host.wait('Web tools: ON')
            host.key('\x1b')
            host.key('\x1b')
            host.click('Keep editing')
            assert not saved.exists()
            host.key('\x1b')
            host.click('Save changes')
            host.wait('Start coding')
            data=json.loads(saved.read_text())
            assert data['common']['/permission']=='/permission bypass'
            assert data['common']['/mode']=='/mode ask'
            assert data['providers']['zai']['/model']=='/model glm-5.2'
            assert data['providers']['zai']['/thinking']=='/thinking high'
            assert data['providers']['zai']['/auxiliary']=='/auxiliary glm-4.7'
            assert data['providers']['zai']['/generation']=='/generation precise'
            assert data['providers']['zai']['/mixture']=='/mixture enabled'
            assert json.loads((root/'.agent-vesper/acceptance-settings.json').read_text())==dict(enabled=True,prd='')
            assert json.loads((root/'.agent-vesper/swarm-settings.json').read_text())['enabled']
            assert json.loads((root/'.agent-vesper/web-settings.json').read_text())['enabled']
            host.click('Start coding')
            host.key('/permission\r')
            host.wait('Bypass')
        finally: host.close()
        host=Host(binary,root)
        try:
            host.wait('Start coding')
            host.wait('glm-5.2')
            host.key('s')
            host.wait('current Bypass')
            host.wait('current Plan')
            host.click('Implementation acceptance')
            host.wait('Enforced completion: ON')
        finally: host.close()
        print('PASS: native Settings mouse navigation, discard, keep editing, grouped save, automatic PRD toggle, active permission and restart persistence; isolated HOME/workspace, no provider prompt.')

if __name__=='__main__':
    run(str(Path(sys.argv[1]).resolve()))
