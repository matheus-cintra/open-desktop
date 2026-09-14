#!/usr/bin/env python3
"""Transactional per-user installer. Keeps recovery copies until explicit cleanup."""
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import sys
import tempfile


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


v = module('macos-update-validation')
r = module('macos-update-runtime')


class Installer:
    def __init__(self, home):
        self.home = home
        self.app = home / 'Applications/Open Desktop.app'
        self.root = home / 'Library/Application Support/Open Desktop'
        self.link = home / '.local/bin/opendesk'
        self.journal = self.root / 'update-transaction.json'
        self.data = [home / 'Library/Application Support/opendesk',
                     home / 'Library/Preferences/dev.mcintra.opendesk.plist']

    def snapshot(self, stage, gui, running):
        backups = self.root / 'backups'
        backups.mkdir(mode=0o700, exist_ok=True)
        backup = Path(tempfile.mkdtemp(prefix='update.', dir=backups))
        print('Backup: ' + str(backup), flush=True)
        state = {'backup': str(backup), 'stage': str(stage), 'gui': gui,
                 'running': running, 'phase': 'prepared', 'app': self.app.exists(),
                 'data': [p.exists() for p in self.data],
                 'link': self.link.exists() or self.link.is_symlink()}
        if state['app']:
            r.copy(self.app, backup / 'Open Desktop.app')
        for index, path in enumerate(self.data):
            if state['data'][index]:
                r.copy(path, backup / f'data-{index}')
        r.backup_preferences(backup / 'data-1')
        state['data'][1] = (backup / 'data-1').exists()
        if state['link']:
            if self.link.is_dir() and not self.link.is_symlink():
                raise RuntimeError('CLI path is a directory')
            if self.link.is_symlink():
                (backup / 'cli').symlink_to(os.readlink(self.link))
            else:
                r.copy(self.link, backup / 'cli')
        r.save_json(backup / 'manifest.json', state)
        r.save_json(self.journal, state)
        return state

    def restore(self, state):
        backup = Path(state['backup'])
        print('Restoring from ' + str(backup), flush=True)
        r.stop(self.app)
        # Reconstruct into fresh paths; never move/delete the backup itself.
        recovery = Path(tempfile.mkdtemp(prefix='.opendesk-restore.', dir=self.app.parent))
        if state['app']:
            r.copy(backup / 'Open Desktop.app', recovery / 'Open Desktop.app')
            v.signature(recovery / 'Open Desktop.app', recovery, 'restore-cert')
        if self.app.exists():
            os.rename(self.app, recovery / 'failed.app')
        if state['app']:
            os.rename(recovery / 'Open Desktop.app', self.app)
        for index, path in enumerate(self.data):
            if path.exists():
                r.copy(path, recovery / f'failed-data-{index}')
                if path.is_dir():
                    shutil.rmtree(path)
                else:
                    path.unlink()
            if state['data'][index]:
                r.copy(backup / f'data-{index}', path)
        r.restore_preferences(backup / 'data-1')
        if self.link.exists() or self.link.is_symlink():
            self.link.unlink()
        if state['link']:
            old = backup / 'cli'
            if old.is_symlink():
                self.link.symlink_to(os.readlink(old))
            else:
                r.copy(old, self.link)
        if state['app']:
            r.run(r.LSREGISTER, '-f', self.app)
            if state['running'] or state['gui']:
                r.launch(self.app, self.home, state['gui'])
        state['phase'] = 'restored'
        r.save_json(self.journal, state)
        self.journal.unlink()
        print('Previous installation restored. Backup retained: ' + str(backup), flush=True)

    def install(self, source, version, stage):
        candidate = stage / 'Open Desktop.app'
        r.copy(source, candidate)
        identity = v.validate(candidate, stage, version, self.app)
        active = r.processes(self.app)
        gui = any(is_gui for _, is_gui in active)
        if gui:
            print('Closing organization window: unapplied edits are not part of the saved map.', flush=True)
        # Persist reopen intent before stopping, including interruptions during backup.
        intent = {'phase': 'stopping', 'gui': gui, 'running': bool(active)}
        r.save_json(self.journal, intent)
        r.stop(self.app)
        state = self.snapshot(stage, gui, bool(active))
        state['phase'] = 'replacing'
        r.save_json(self.journal, state)
        if self.app.exists():
            os.rename(self.app, stage / 'previous.app')
        os.rename(candidate, self.app)
        self.link.parent.mkdir(parents=True, exist_ok=True)
        link = stage / 'cli-new'
        link.symlink_to(self.app / 'Contents/MacOS/opendesk')
        os.replace(link, self.link)
        if v.signature(self.app, stage, 'final-cert') != identity:
            raise RuntimeError('Installed signature changed')
        r.launch(self.app, self.home, gui)
        state['phase'] = 'committed'
        r.save_json(self.journal, state)
        self.journal.unlink()
        shutil.rmtree(stage)
        print('Installed ' + str(self.app), flush=True)

    def recover(self):
        if not self.journal.exists():
            return False
        state = json.loads(self.journal.read_text())
        if state['phase'] == 'stopping':
            if state['running']:
                r.run('/usr/bin/open', self.app)
                r.healthy(self.app, self.home)
            if state['gui'] and not any(gui for _, gui in r.processes(self.app)):
                r.launch(self.app, self.home, True)
            self.journal.unlink()
        elif state['phase'] not in ('committed', 'restored'):
            self.restore(state)
        else:
            self.journal.unlink()
        return True


def main():
    if platform.system() != 'Darwin' or platform.machine() != 'arm64' or os.getuid() == 0:
        raise RuntimeError('Run as your user on Apple Silicon macOS, without sudo')
    os.umask(0o077)
    installer = Installer(Path.home())
    installer.root.mkdir(parents=True, exist_ok=True)
    installer.app.parent.mkdir(parents=True, exist_ok=True)
    with (installer.root / 'update.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError('Another installation/update is running') from None
        if installer.recover():
            raise RuntimeError('Interrupted transaction recovered; rerun the requested update')
        stage = Path(tempfile.mkdtemp(prefix='.opendesk-install.', dir=installer.app.parent))
        def interrupted(signum, _):
            raise RuntimeError('Update interrupted by signal ' + str(signum))
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            signal.signal(sig, interrupted)
        try:
            args = sys.argv[1:]
            if args and args[0] == 'update':
                source, version = v.public(stage, args[1] if len(args) > 1 else 'latest')
            elif not args or args == ['']:
                source, version = Path(__file__).resolve().parent.parent / 'dist/Open Desktop.app', None
            else:
                raise ValueError('Usage: install-macos.sh [update [latest|vX.Y.Z]]')
            installer.install(source, version, stage)
        except BaseException:
            for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
                signal.signal(sig, signal.SIG_IGN)
            try:
                installer.recover()
            except BaseException as error:
                print(f'RECOVERY FAILED: {error}. Keep {installer.journal} and all backups/staging.', file=sys.stderr)
            raise
        finally:
            if not installer.journal.exists() and stage.exists():
                shutil.rmtree(stage)


if __name__ == '__main__':
    try:
        main()
    except BaseException as error:
        print('Open Desktop update failed: ' + str(error), file=sys.stderr)
        sys.exit(1)
