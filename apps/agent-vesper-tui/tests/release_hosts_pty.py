#!/usr/bin/env python3
"""Native TUI/ACP observation and cancellation of one isolated RRC ledger.

Only synthetic persisted state; no provider, GitHub mutation or user-state writes.
"""
import hashlib
import json
import os
from pathlib import Path
import select
import socket
import subprocess
import sys
import tempfile
import time
from settings_pty import Host


def seed(root, sha, published=False):
    common = Path(subprocess.check_output(
        ['git', 'rev-parse', '--git-common-dir'], cwd=root, text=True).strip())
    identity = str((common if common.is_absolute() else root / common).resolve())
    now = '2026-10-05T00:00:00Z'
    jobs = [dict(workflow_id=10, run_id=20, attempt=1, job_id=100+i,
                 workflow_name='five-target-foundation', job_name=f'fixture-lane-{i}',
                 platform='fixture', state='failure' if i==0 else 'in_progress',
                 failed_step='exact regression' if i==0 else None,
                 url=f'https://example.invalid/jobs/{100+i}') for i in range(5)]
    record = dict(schema_version=1, repo_identity=identity, epoch_id='host-parity-epoch',
                  objective=dict(bump='patch', branch_ref='main', post_release_main_epoch=published,
                                 required_gate_names=['five-target-foundation']),
                  state='post_release_main_degraded' if published else 'waiting_for_matrix',
                  release_version='v0.24.4' if published else None, release_commit='1'*40 if published else sha,
                  current_main=sha if published else None, required_gates=[dict(name='five-target-foundation',
                  head_sha=sha, run_id=20, run_attempt=1, run_state='in_progress', jobs=jobs, url=None)],
                  failures=[], repair_attempts=[], state_changes=[], retry_budget=dict(full_gate_limit=1,
                  full_gate_used=0, infrastructure_limit=1, infrastructure_used=0, targeted_diagnostic_limit=1,
                  targeted_diagnostic_used=0, repair_attempt_limit_per_family=2), external_block=None,
                  transitions=[], consecutive_stagnant_actions=0, last_progress_at=now, created_at=now, updated_at=now)
    if published:
        record['mutation'] = dict(source_commit='1'*40, version_before='0.24.3', version_after='0.24.4',
                objective_evidence_reports=[], intended_commits=[],
                version_files=[], local_gates=[], candidate_committed=True, candidate_pushed=True,
                candidate_push_ref='origin/main@'+'1'*40, tag_name='v0.24.4', tag_object='2'*40,
                tag_pushed=True, publication_run_id=30, publication_verified=True,
                published_asset_names=['fixture-immutable-asset'])
    ledger = root/'release-state'/f'{hashlib.sha256(identity.encode()).hexdigest()}.json'
    ledger.parent.mkdir(exist_ok=True)
    ledger.write_text(json.dumps(record))
    return ledger


class Acp:
    def __init__(self, binary, root, env):
        self.child = subprocess.Popen([binary], cwd=root, env=env, stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.pending = b''
        self.messages = []

    def rpc(self, identity, method, params):
        self.child.stdin.write((json.dumps(dict(jsonrpc='2.0', id=identity, method=method, params=params))+'\n').encode())
        self.child.stdin.flush()
        deadline = time.monotonic()+30
        while time.monotonic()<deadline:
            while b'\n' in self.pending:
                row, self.pending = self.pending.split(b'\n',1)
                value = json.loads(row);self.messages.append(value)
                if value.get('id') == identity:
                    assert 'error' not in value, value
                    return value
            ready,_,_ = select.select([self.child.stdout],[],[],max(0,deadline-time.monotonic()))
            if ready:
                chunk=os.read(self.child.stdout.fileno(),65536)
                assert chunk, 'ACP ended before response';self.pending+=chunk
        raise AssertionError('ACP response deadline exceeded')

    def prompt(self, identity, session, text):
        start=len(self.messages)
        self.rpc(identity,'session/prompt',dict(sessionId=session,prompt=[dict(type='text',text=text)]))
        return '\n'.join(value['params']['update'].get('content',{}).get('text','')
                         for value in self.messages[start:] if value.get('method')=='session/update')

    def close(self):
        self.child.terminate();self.child.wait(timeout=10)


def run(tui_binary, acp_binary):
    with tempfile.TemporaryDirectory(prefix='vesper-release-hosts-') as temp:
        root=Path(temp).resolve()
        subprocess.run(['git','init','-q','-b','main'],cwd=root,check=True)
        subprocess.run(['git','-c','user.name=fixture','-c','user.email=fixture@example.invalid',
                        'commit','-q','--allow-empty','-m','fixture'],cwd=root,check=True)
        subprocess.run(['git','remote','add','origin','https://github.com/fixture/rrc-hosts'],cwd=root,check=True)
        sha=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
        ledger=seed(root,sha)
        vault=root/'vault.json';vault.write_text(json.dumps(dict(credentials={name:{'native-auth':json.dumps(dict(mode='signed-out'))} for name in ['openai','xai']})));vault.chmod(0o600)
        listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen();listener.settimeout(.05)
        extra=dict(AGENT_VESPER_RELEASE_ROOT=str(ledger.parent),AGENT_VESPER_OPENAI_CREDENTIALS_PATH=str(vault),
                   AGENT_VESPER_XAI_CREDENTIALS_PATH=str(vault),AGENT_VESPER_FULL_HARNESS='1',AGENT_VESPER_VRO_ENABLED='0',
                   AGENT_VESPER_GLM_BASE_URL=f'http://127.0.0.1:{listener.getsockname()[1]}/v4',
                   AGENT_VESPER_ALLOW_INSECURE_LOOPBACK='1')
        host=Host(tui_binary,root,extra)
        env=dict(PATH=os.environ['PATH'],HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),
                 XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'),ZAI_API_KEY='fixture-key',
                 AGENT_VESPER_PROVIDER='zai',AGENT_VESPER_COGNITION_ROOT=str(root/'acp-cognition'),
                 AGENT_VESPER_GLOBAL_COGNITION_ROOT=str(root/'global-cognition'),
                 HTTP_PROXY='http://127.0.0.1:9',HTTPS_PROXY='http://127.0.0.1:9',ALL_PROXY='http://127.0.0.1:9',NO_PROXY='127.0.0.1',**extra)
        acp=Acp(acp_binary,root,env)
        try:
            acp.rpc(1,'initialize',dict(protocolVersion=1))
            session=acp.rpc(2,'session/new',dict(cwd=str(root),mcpServers=[]))['result']['sessionId']
            before=ledger.read_bytes()
            text=acp.prompt(3,session,'/release status')
            assert 'WaitingForMatrix' in text and '1/5 terminal' in text and 'blocked' in text,text
            host.wait('Start coding');host.click('Start coding');host.key('/release status\r')
            host.wait('WaitingForMatrix');host.wait('1/5 terminal')
            assert ledger.read_bytes()==before,'status observers mutated the ledger'
            text=acp.prompt(4,session,'/release retry');assert 'blocked' in text,text
            assert ledger.read_bytes()==before,'blocked retry altered epoch'
            ledger=seed(root,sha,published=True)
            text=acp.prompt(5,session,'/release status')
            assert 'PUBLISHED / VERIFIED' in text and 'DEGRADED' in text,text
            host.key('/release status\r');host.wait('PUBLISHED / VERIFIED');host.wait('DEGRADED')
            text=acp.prompt(6,session,'/release cancel');assert 'cancelled locally' in text,text
            cancelled=json.loads(ledger.read_text());assert cancelled['state']=='cancelled'
            assert cancelled['epoch_id']=='host-parity-epoch' and cancelled['required_gates'][0]['run_id']==20
            assert cancelled['mutation']['tag_name']=='v0.24.4' and cancelled['mutation']['published_asset_names']==['fixture-immutable-asset']
            host.key('/release status\r');host.wait('Cancelled')
            text=acp.prompt(7,session,'/ci');assert 'Cancelled' in text and 'PUBLISHED / VERIFIED' in text,text
            try: listener.accept();raise AssertionError('host controls dispatched a provider request')
            except socket.timeout: pass
        finally:
            acp.close();host.close();listener.close()
    print('PASS: native TUI and ACP observed identical partial-matrix and Published/main-degraded state; blocked retry wrote nothing; ACP cancellation reached TUI with epoch/run/tag/assets retained; no provider calls.')


if __name__=='__main__': run(*(str(Path(arg).resolve()) for arg in sys.argv[1:3]))
