#!/usr/bin/env python3
"""Real native setup consent/decline/failure with installation structurally blocked."""
import sys
import tempfile
from pathlib import Path
from settings_pty import Host

def run(binary):
    with tempfile.TemporaryDirectory(prefix='vesper-setup-pty-') as temp:
        root = Path(temp)
        host = Host(str(Path(binary).resolve()), root, {'VESPER_DOCKER_BIN': str(root/'missing-engine')})
        try:
            host.wait('Start coding')
            host.click('Settings')
            host.click('Web tools')
            host.click('Set up features / repair')
            host.wait('Set up web and isolated tools')
            host.click('Continue coding without setup')
            host.wait('Setup declined')
            assert not (root/'home/.agent-vesper/runtime.json').exists()
            assert not (root/'home/.agent-vesper/runtime-setup.lock').exists()
            for _ in range(2):
                host.click('Set up features / repair')
                host.click('Set up web and isolated tools')
                host.wait('driver is missing')
                assert not (root/'home/.agent-vesper/runtime.json').exists()
                assert not (root/'.agent-vesper/web-settings.json').exists()
            host.key('\x1b')
            host.key('\x1b')
            host.wait('Start coding')
            print('PASS: native setup confirmation, decline, truthful failure, retry and unchanged web preferences')
        finally:
            host.close()

if __name__ == '__main__':
    run(sys.argv[1])
