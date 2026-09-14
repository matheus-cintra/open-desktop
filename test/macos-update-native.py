#!/usr/bin/env python3
"""Real codesign/ditto validation in disposable staging; never stops the live app."""
import importlib.util
from pathlib import Path
import platform
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/macos-update.py'
spec = importlib.util.spec_from_file_location('updater', SCRIPT)
u = importlib.util.module_from_spec(spec)
spec.loader.exec_module(u)


@unittest.skipUnless(platform.system() == 'Darwin', 'requires macOS codesign')
class NativeValidation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.stage = Path(self.temp.name)
        self.installed = Path.home() / 'Applications/Open Desktop.app'
        self.app = self.stage / 'Open Desktop.app'
        u.r.copy(self.installed, self.app)

    def test_same_identity(self):
        self.assertTrue(u.v.validate(self.app, self.stage, installed=self.installed))

    def test_different_signature(self):
        u.r.run('/usr/bin/codesign', '--force', '--sign', '-', self.app)
        with self.assertRaises((ValueError, OSError, subprocess.CalledProcessError)):
            u.v.validate(self.app, self.stage, installed=self.installed)

    def test_wrong_release_version(self):
        with self.assertRaisesRegex(ValueError, 'version mismatch'):
            u.v.validate(self.app, self.stage, '99.99.99', self.installed)

    def test_invalid_identifier(self):
        path = self.app / 'Contents/Info.plist'
        info = plistlib.loads(path.read_bytes())
        info['CFBundleIdentifier'] = 'other.app'
        path.write_bytes(plistlib.dumps(info))
        with self.assertRaisesRegex(ValueError, 'CFBundleIdentifier'):
            u.v.validate(self.app, self.stage, installed=self.installed)

    def test_signature_tampering(self):
        with (self.app / 'Contents/Resources/BUILD.txt').open('a') as stream:
            stream.write('tampered=true\n')
        with self.assertRaises(subprocess.CalledProcessError):
            u.v.validate(self.app, self.stage, installed=self.installed)

    def test_rollback_with_real_signed_bundle_and_ditto(self):
        home = self.stage / 'isolated-home'
        installer = u.Installer(home)
        installer.root.mkdir(parents=True)
        installer.app.parent.mkdir(parents=True)
        u.r.copy(self.installed, installer.app)
        installer.data[0].mkdir(parents=True)
        (installer.data[0] / 'identity').write_text('isolated identity fixture')
        installer.link.parent.mkdir(parents=True)
        installer.link.symlink_to('/previous/cli')
        stage = home / 'Applications/stage'
        stage.mkdir()
        def fail_launch(*args):
            (installer.data[0] / 'identity').write_text('candidate mutation')
            raise RuntimeError('controlled initialization failure')
        # Keep all process and preference operations away from the real user.
        # Bundle validation, certificate extraction, ditto and replacement are real.
        with patch.object(u.r, 'processes', return_value=[]), \
             patch.object(u.r, 'stop'), patch.object(u.r, 'launch', side_effect=fail_launch), \
             patch.object(u.r, 'backup_preferences'), patch.object(u.r, 'restore_preferences'):
            with self.assertRaisesRegex(RuntimeError, 'controlled'):
                installer.install(self.app, None, stage)
            self.assertTrue(installer.recover())
        self.assertEqual((installer.data[0] / 'identity').read_text(), 'isolated identity fixture')
        self.assertEqual(installer.link.readlink(), Path('/previous/cli'))
        self.assertFalse(installer.journal.exists())
        u.v.validate(installer.app, self.stage, installed=self.installed)


if __name__ == '__main__':
    unittest.main()
