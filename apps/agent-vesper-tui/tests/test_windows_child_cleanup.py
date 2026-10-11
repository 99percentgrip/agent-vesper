"""Offline ConPTY cleanup races; no Windows device or process is opened."""
import subprocess
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch
from settings_pty import WindowsChild


class WindowsCleanupTests(unittest.TestCase):
    def child(self, polls):
        child = WindowsChild.__new__(WindowsChild)
        child.pid = 123
        child.poll = Mock(side_effect=polls)
        child.wait = Mock()
        child.terminal = SimpleNamespace(pty=Mock(), fileobj=Mock(), _server=Mock(),
                                         _thread=Mock(), closed=False)
        child.terminal._thread.is_alive.return_value = False
        return child

    def assert_closed(self, child):
        self.assertTrue(child.terminal.closed)
        child.terminal._thread.join.assert_called_once_with(timeout=5)
        child.terminal.fileobj.close.assert_called_once()

    def test_already_exited_never_kills(self):
        child = self.child([0])
        with patch('settings_pty.subprocess.run') as run:
            child.close()
        run.assert_not_called()
        self.assert_closed(child)

    def test_successful_kill_observes_exit(self):
        child = self.child([None])
        with patch('settings_pty.subprocess.run') as run:
            child.close()
        run.assert_called_once_with(['taskkill', '/PID', '123', '/T', '/F'],
                                    check=True, capture_output=True, timeout=10)
        child.wait.assert_called_once_with(10)
        self.assert_closed(child)

    def test_exit_during_taskkill_is_verified(self):
        child = self.child([None])
        failure = subprocess.CalledProcessError(255, ['taskkill'])
        with patch('settings_pty.subprocess.run', side_effect=failure):
            child.close()
        child.wait.assert_called_once_with(10)
        self.assert_closed(child)

    def test_taskkill_failure_with_live_child_remains_failure(self):
        child = self.child([None])
        child.wait.side_effect = subprocess.TimeoutExpired('ConPTY child', 10)
        failure = subprocess.CalledProcessError(255, ['taskkill'])
        with patch('settings_pty.subprocess.run', side_effect=failure):
            with self.assertRaisesRegex(AssertionError, 'ConPTY child did not exit'):
                child.close()
        self.assertFalse(child.terminal.closed)
        child.terminal.fileobj.close.assert_not_called()

    def test_taskkill_timeout_remains_failure(self):
        child = self.child([None])
        with patch('settings_pty.subprocess.run', side_effect=subprocess.TimeoutExpired('taskkill', 10)):
            with self.assertRaises(subprocess.TimeoutExpired):
                child.close()
        self.assertFalse(child.terminal.closed)

    def test_reader_that_does_not_settle_remains_failure(self):
        child = self.child([0])
        child.terminal._thread.is_alive.return_value = True
        with self.assertRaisesRegex(AssertionError, 'ConPTY reader did not settle'):
            child.close()
        self.assertFalse(child.terminal.closed)


if __name__ == '__main__':
    unittest.main()
