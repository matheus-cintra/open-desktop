#!/usr/bin/env python3
"""Exercise the real transaction on isolated files; mock only OS/network boundaries."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import time
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/macos-update.py'
spec = importlib.util.spec_from_file_location('updater', SCRIPT)
u = importlib.util.module_from_spec(spec)
spec.loader.exec_module(u)


def copy(src, dst):
    if Path(src).is_dir():
        shutil.copytree(src, dst, dirs_exist_ok=True)
    else:
        Path(dst).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)


class Transaction(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.i = u.Installer(self.home)
        self.i.root.mkdir(parents=True)
        self.i.app.mkdir(parents=True)
        (self.i.app / 'version').write_text('old')
        self.i.data[0].mkdir(parents=True)
        (self.i.data[0] / 'identity').write_text('private fixture')
        self.i.link.parent.mkdir(parents=True)
        self.i.link.symlink_to('/previous/cli')
        self.source = self.home / 'source.app'
        self.source.mkdir()
        (self.source / 'version').write_text('new')
        self.stage = self.home / 'Applications/stage'
        self.stage.mkdir()
        self.calls = []
        for obj, name, effect in [
            (u.r, 'copy', copy), (u.r, 'processes', lambda _: [(123, True), (124, False)]),
            (u.r, 'stop', lambda _: self.calls.append('stop')),
            (u.r, 'run', lambda *args: ''),
            (u.r, 'launch', lambda *args: self.calls.append('launch')),
            (u.r, 'healthy', lambda *args: None),
            (u.r, 'backup_preferences', lambda *args: None),
            (u.r, 'restore_preferences', lambda *args: None),
            (u.v, 'validate', lambda *args: ('requirement', 'cert')),
            (u.v, 'signature', lambda *args: ('requirement', 'cert')),
        ]:
            mock = patch.object(obj, name, side_effect=effect)
            mock.start()
            self.addCleanup(mock.stop)

    def install(self):
        self.i.install(self.source, None, self.stage)

    def assert_old(self):
        self.assertEqual((self.i.app / 'version').read_text(), 'old')
        self.assertEqual((self.i.data[0] / 'identity').read_text(), 'private fixture')
        self.assertEqual(os.readlink(self.i.link), '/previous/cli')

    def test_success_preserves_state_and_backup(self):
        self.install()
        self.assertEqual((self.i.app / 'version').read_text(), 'new')
        self.assertFalse(self.i.journal.exists())
        backup = next((self.i.root / 'backups').iterdir())
        self.assertEqual(backup.stat().st_mode & 0o777, 0o700)
        self.assertEqual((backup / 'Open Desktop.app/version').read_text(), 'old')
        self.assertEqual(self.calls, ['stop', 'launch'])
        self.assertTrue(json.loads((backup / 'manifest.json').read_text())['gui'])

    def test_invalid_signature_before_stop(self):
        u.v.validate.side_effect = ValueError('signature')
        with self.assertRaises(ValueError):
            self.install()
        self.assertEqual(self.calls, [])
        self.assert_old()

    def test_process_refuses_quit(self):
        u.r.stop.side_effect = RuntimeError('busy')
        with self.assertRaises(RuntimeError):
            self.install()
        self.assert_old()

    def test_startup_failure_rolls_back_data_and_cli(self):
        def fail(*args):
            (self.i.data[0] / 'identity').write_text('candidate changed data')
            raise RuntimeError('IPC timeout')
        u.r.launch.side_effect = fail
        with self.assertRaises(RuntimeError):
            self.install()
        u.r.launch.side_effect = lambda *args: None
        self.assertTrue(self.i.recover())
        self.assert_old()

    def test_replacement_failure_recovers(self):
        real = os.rename
        def rename(src, dst):
            if Path(src) == self.stage / 'Open Desktop.app':
                raise OSError('replacement failure')
            return real(src, dst)
        with patch.object(u.os, 'rename', side_effect=rename):
            with self.assertRaises(OSError):
                self.install()
        self.i.recover()
        self.assert_old()

    def test_abrupt_interruption_recovered_next_invocation(self):
        state = self.i.snapshot(self.stage, True, True)
        state['phase'] = 'replacing'
        u.r.save_json(self.i.journal, state)
        os.rename(self.i.app, self.stage / 'previous.app')
        fresh = u.Installer(self.home)
        self.assertTrue(fresh.recover())
        self.assert_old()

    def test_failed_recovery_keeps_journal_and_only_backup(self):
        state = self.i.snapshot(self.stage, True, True)
        shutil.rmtree(self.i.app)
        u.r.copy.side_effect = OSError('disk failure')
        with self.assertRaises(OSError):
            self.i.recover()
        self.assertTrue(self.i.journal.exists())
        self.assertTrue((Path(state['backup']) / 'Open Desktop.app/version').exists())

    def test_backup_failure_does_not_replace(self):
        u.r.copy.side_effect = [copy(self.source, self.stage / 'Open Desktop.app'), OSError('full')]
        with self.assertRaises(OSError):
            self.install()
        self.assert_old()

    def test_concurrent_lock(self):
        with (self.i.root / 'update.lock').open('a') as lock:
            u.fcntl.flock(lock, u.fcntl.LOCK_EX | u.fcntl.LOCK_NB)
            with patch.object(u.platform, 'system', return_value='Darwin'), \
                 patch.object(u.platform, 'machine', return_value='arm64'), \
                 patch.object(u.os, 'getuid', return_value=501), \
                 patch.object(u.Path, 'home', return_value=self.home):
                with self.assertRaisesRegex(RuntimeError, 'Another installation'):
                    u.main()

    def signal_case(self, sig):
        driver = self.home / 'driver.py'
        driver.write_text("""
import importlib.util, os, pathlib, shutil, sys, time
spec = importlib.util.spec_from_file_location('updater', sys.argv[1])
u = importlib.util.module_from_spec(spec)
spec.loader.exec_module(u)
home = pathlib.Path(sys.argv[2])
u.platform.system = lambda: 'Darwin'
u.platform.machine = lambda: 'arm64'
u.os.getuid = lambda: 501
u.Path.home = lambda: home
u.r.copy = lambda src, dst: shutil.copytree(src, dst) if pathlib.Path(src).is_dir() else shutil.copy2(src, dst)
u.r.processes = lambda app: []
u.r.stop = lambda app: None
u.r.run = lambda *args: ''
u.r.backup_preferences = lambda *args: None
u.r.restore_preferences = lambda *args: None
u.v.validate = lambda *args: ('requirement', 'cert')
u.v.signature = lambda *args: ('requirement', 'cert')
u.v.public = lambda *args: (home / 'source.app', None)
def launch(*args):
    (home / 'started').touch()
    time.sleep(60)
u.r.launch = launch
sys.argv = ['updater', 'update']
try:
    u.main()
except BaseException:
    sys.exit(1)
""")
        with open(os.devnull, 'w') as quiet:
            child = subprocess.Popen([sys.executable, str(driver), str(SCRIPT), str(self.home)],
                                     stdout=quiet, stderr=quiet)
            try:
                deadline = time.monotonic() + 10
                while not (self.home / 'started').exists():
                    if child.poll() is not None or time.monotonic() >= deadline:
                        self.fail('transaction did not reach startup')
                    time.sleep(.02)
                child.send_signal(sig)
                self.assertNotEqual(child.wait(timeout=10), 0)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()
        if sig == signal.SIGKILL:
            self.assertTrue(self.i.journal.exists())
            self.i.recover()
        self.assert_old()
        self.assertFalse(self.i.journal.exists())

    def test_sigterm_restores(self):
        self.signal_case(signal.SIGTERM)

    def test_sigkill_recovers_next_run(self):
        self.signal_case(signal.SIGKILL)


class ProcessStop(unittest.TestCase):
    def test_stubborn_app_aborts_after_deadline(self):
        with patch.object(u.r, 'processes', return_value=[(123, False)]), \
             patch.object(u.r, 'run') as command, \
             patch.object(u.r.time, 'monotonic', side_effect=[0, 11]):
            with self.assertRaisesRegex(RuntimeError, 'did not stop'):
                u.r.stop(Path('/installed.app'))
        self.assertEqual(command.call_args.args[0], '/usr/bin/osascript')

    def test_only_identified_gui_receives_signal(self):
        with patch.object(u.r, 'processes', side_effect=[[(123, True)], []]), \
             patch.object(u.r.os, 'kill') as kill:
            u.r.stop(Path('/installed.app'))
        kill.assert_called_once_with(123, signal.SIGTERM)


class Health(unittest.TestCase):
    def test_ipc_owned_by_installed_process(self):
        sock = MagicMock()
        sock.__enter__.return_value = sock
        sock.getsockopt.return_value = 123
        sock.recv.return_value = b'{"Status":{}}\n'
        with patch.object(u.r, 'processes', return_value=[(123, False)]), \
             patch.object(u.r.socket, 'socket', return_value=sock):
            u.r.healthy(Path('/app'), Path('/home'), timeout=.01)

    def test_ipc_owned_by_other_process_is_rejected(self):
        sock = MagicMock()
        sock.__enter__.return_value = sock
        sock.getsockopt.return_value = 999
        with patch.object(u.r, 'processes', return_value=[(123, False)]), \
             patch.object(u.r.socket, 'socket', return_value=sock):
            with self.assertRaisesRegex(RuntimeError, 'IPC'):
                u.r.healthy(Path('/app'), Path('/home'), timeout=.01)
        sock.sendall.assert_not_called()

    def test_open_without_ipc_is_not_success(self):
        sock = MagicMock()
        sock.__enter__.return_value = sock
        sock.connect.side_effect = OSError('no socket')
        with patch.object(u.r, 'processes', return_value=[(123, False)]), \
             patch.object(u.r.socket, 'socket', return_value=sock):
            with self.assertRaisesRegex(RuntimeError, 'IPC'):
                u.r.healthy(Path('/app'), Path('/home'), timeout=.01)


class Payload(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.stage = Path(self.temp.name)
        self.archive(['Open Desktop.app/Contents/Info.plist'])

    def archive(self, names):
        with zipfile.ZipFile(self.stage / u.v.ASSET, 'w') as archive:
            for name in names:
                archive.writestr(name, 'fixture')
        self.checksum()

    def checksum(self):
        digest = hashlib.sha256((self.stage / u.v.ASSET).read_bytes()).hexdigest()
        self.manifest = self.stage / 'SHA256SUMS-macos'
        self.manifest.write_text(digest + '  ' + u.v.ASSET + '\n')

    def test_valid_zip(self):
        u.v.validate_zip(self.stage)

    def test_missing_duplicate_bad_checksums(self):
        valid = self.manifest.read_text()
        for content in ('', valid * 2, 'g' * 64 + '  ' + u.v.ASSET, '0' * 64 + '  ' + u.v.ASSET):
            with self.subTest(content=content):
                self.manifest.write_text(content)
                with self.assertRaises(ValueError):
                    u.v.validate_zip(self.stage)

    def test_unsafe_zip_paths(self):
        for name in ('../escape', '/absolute', 'Open Desktop.app/../escape',
                     'Open Desktop.app/Contents/../../escape', 'other.app/payload'):
            with self.subTest(name=name):
                self.archive([name])
                with self.assertRaises(ValueError):
                    u.v.validate_zip(self.stage)

    def test_zip_symlink(self):
        with zipfile.ZipFile(self.stage / u.v.ASSET, 'w') as archive:
            item = zipfile.ZipInfo('Open Desktop.app/link')
            item.create_system = 3
            item.external_attr = 0o120777 << 16
            archive.writestr(item, '/tmp')
        self.checksum()
        with self.assertRaises(ValueError):
            u.v.validate_zip(self.stage)

    def test_invalid_zip_with_valid_checksum(self):
        (self.stage / u.v.ASSET).write_bytes(b'invalid')
        self.checksum()
        with self.assertRaises(zipfile.BadZipFile):
            u.v.validate_zip(self.stage)

    def test_download_unavailable(self):
        with patch.object(u.v, 'download', side_effect=RuntimeError('offline')):
            with self.assertRaises(RuntimeError):
                u.v.public(self.stage, 'v0.3.0')

    def test_latest_pins_both_downloads(self):
        urls = []
        def download(url, path):
            urls.append(url)
            if str(path).endswith('release.json'):
                path.write_text('{"tag_name":"v0.3.0"}')
        with patch.object(u.v, 'download', side_effect=download), \
             patch.object(u.v, 'validate_zip'), patch.object(u.v, 'run'):
            _, version = u.v.public(self.stage, 'latest')
        self.assertEqual(version, '0.3.0')
        self.assertTrue(all('/download/v0.3.0/' in url for url in urls[1:]))


if __name__ == '__main__':
    unittest.main()
