#!/usr/bin/env python3
"""Exercise the real Skills draft, cancellation, save, restart and preservation."""
import json
from pathlib import Path
import sys
import tempfile
from settings_pty import Host

def run(binary):
    with tempfile.TemporaryDirectory(prefix='vesper-routing-pty-') as temp:
        root = Path(temp)
        library = root/'global-memory/skills'
        library.mkdir(parents=True)
        source = library/'ledger-audit.md'
        source.write_text('---\ndescription: Audit ledger transactions\n---\nUSER EDITS\n')
        before = source.read_bytes()
        path = root/'.agent-vesper/skill-routing.json'
        host = Host(binary, root)
        try:
            host.wait('Start coding')
            host.key('s')
            host.click('Skills')
            host.click('Model-assisted selection: OFF')
            host.wait('Routing: Enhanced')
            host.click('[x] ledger-audit')
            host.wait('[ ] ledger-audit')
            assert not path.exists()
            host.key('\x1b')
            host.key('\x1b')
            host.click('Discard changes')
            host.wait('Start coding')
            assert not path.exists()
            host.key('s')
            host.click('Skills')
            host.wait('Routing: Standard')
            host.wait('Model-assisted selection: OFF')
            host.wait('[x] ledger-audit')
            host.click('Model-assisted selection: OFF')
            host.click('[x] ledger-audit')
            host.key('\x1b')
            host.key('\x1b')
            host.click('Keep editing')
            assert not path.exists()
            host.key('\x1b')
            host.click('Save changes')
            host.wait('Start coding')
            assert json.loads(path.read_text()) == {'mode':'enhanced','model_assistance':True,'disabled':['ledger-audit']}
            assert source.read_bytes() == before
        finally:
            host.close()
        host = Host(binary, root)
        try:
            host.wait('Start coding')
            host.key('s')
            host.click('Skills')
            host.wait('Routing: Enhanced')
            host.wait('[ ] ledger-audit')
            host.wait('Model-assisted selection: ON')
            assert source.read_bytes() == before
        finally:
            host.close()
        print('Skills native PTY: draft/discard/keep/save/restart and library preservation PASS')

if __name__ == '__main__':
    run(str(Path(sys.argv[1]).resolve()))
