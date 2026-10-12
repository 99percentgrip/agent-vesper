"""Regression proof for the shared POSIX terminal writer; no provider/state."""
import errno
import os
import socket
import subprocess
import sys
import threading
import time
import unittest
from unittest.mock import patch
from settings_pty import Host


@unittest.skipIf(os.name == 'nt', 'POSIX writer; ConPTY has its own writer')
class TerminalWriterTests(unittest.TestCase):
    def test_normal_exit_drains_real_terminal_output(self):
        import pty
        import select
        master, slave = pty.openpty()
        child = subprocess.Popen([sys.executable, '-c',
            'import os; os.write(1, b"final output\\n" * 65536)'],
            stdout=slave, stderr=slave)
        os.close(slave)
        host = Host.__new__(Host)
        host.child = child
        host.master = master
        host.text = lambda: 'exit fixture'
        host.raw = ''
        drained = bytearray()
        def pump(duration):
            if select.select([master], [], [], duration)[0]:
                try:
                    drained.extend(os.read(master, 65536))
                except OSError:
                    pass
        host.pump = pump
        try:
            # An undrained terminal cannot finish this child; the normal exit
            # observer must continue reading without changing its deadline.
            with self.assertRaises(subprocess.TimeoutExpired):
                child.wait(timeout=.1)
            self.assertEqual(host.wait_exit(3), 0)
            self.assertGreater(len(drained), 65536)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=2)
            os.close(master)

    def test_exit_observer_keeps_deadline_and_reports_visible_state(self):
        host = Host.__new__(Host)
        host.child = type('Child', (), {'poll': lambda _self: None})()
        host.text = lambda: 'still in content inspection'
        host.raw = 'last native output'
        pumped = []
        host.pump = lambda duration: pumped.append(duration)
        with patch('settings_pty.time.monotonic', side_effect=[0, 0, 11]):
            with self.assertRaisesRegex(AssertionError, 'still in content inspection'):
                host.wait_exit(10)
        self.assertTrue(pumped)

    def test_partial_writes_preserve_exact_unicode_and_newline_bytes(self):
        host = Host.__new__(Host)
        host.master = 99
        host.pump = lambda *_: None
        payload = 'before\r\n💡after'.encode()
        observed = bytearray()
        def partial(_fd, data):
            count = min(2, len(data))
            observed.extend(data[:count])
            return count
        with patch('settings_pty.fcntl.fcntl', return_value=0), patch('settings_pty.os.write', side_effect=partial):
            host.write(payload)
        self.assertEqual(bytes(observed), payload)

    def test_backpressure_drains_output_before_retry(self):
        host = Host.__new__(Host)
        host.master = 99
        drained = []
        host.pump = lambda *_: drained.append(True)
        with patch('settings_pty.fcntl.fcntl', return_value=0), patch('settings_pty.os.write', side_effect=[BlockingIOError(errno.EAGAIN, 'full terminal'), 3]):
            host.write(b'abc')
        self.assertTrue(drained)

    def test_stalled_writer_has_a_deadline_and_restores_flags(self):
        host = Host.__new__(Host)
        host.master = 99
        host.pump = lambda *_: None
        with patch('settings_pty.fcntl.fcntl', return_value=0) as flags, patch('settings_pty.os.write', side_effect=BlockingIOError(errno.EAGAIN, 'full terminal')), patch('settings_pty.time.monotonic', side_effect=[0, 0, 11]):
            with self.assertRaisesRegex(TimeoutError, 'terminal input'):
                host.write(b'abc')
        self.assertEqual(flags.call_args_list[-1].args, (99, __import__('fcntl').F_SETFL, 0))

    def test_real_duplex_backpressure_preserves_complete_payload(self):
        writer, peer = socket.socketpair()
        writer.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1024)
        peer.settimeout(2)
        host = Host.__new__(Host)
        host.master = writer.fileno()
        drained = threading.Event()
        received = bytearray()
        payload = ('paste 💡\r\n' * 5000).encode()
        peer.sendall(b'frame')
        def pump(_duration):
            try:
                writer.recv(1024)
            except BlockingIOError:
                pass
            drained.set()
        host.pump = pump
        def consume():
            if not drained.wait(2):
                return
            while len(received) < len(payload):
                try:
                    block = peer.recv(4096)
                except (OSError, TimeoutError):
                    return
                if not block:
                    return
                received.extend(block)
        thread = threading.Thread(target=consume, daemon=True)
        thread.start()
        watchdog = threading.Timer(3, lambda: writer.shutdown(socket.SHUT_RDWR))
        watchdog.start()
        try:
            host.write(payload)
            thread.join(3)
            self.assertFalse(thread.is_alive())
            self.assertEqual(bytes(received), payload)
        finally:
            watchdog.cancel()
            writer.close()
            peer.close()
            thread.join(3)


if __name__ == '__main__':
    unittest.main()
