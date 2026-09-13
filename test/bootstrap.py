#!/usr/bin/env python3
"""Offline tests of the actual POSIX bootstrap, including pipe-style stdin."""
import hashlib
import io
import os
import pty
import select
import signal
import time
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

class Bootstrap(unittest.TestCase):
    def run_bootstrap(self, failure="", version="latest", interactive=False, skip_setup=False):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            binaries = stage / "bin"
            binaries.mkdir()
            asset = stage / "asset.tar.gz"
            with tarfile.open(asset, "w:gz") as archive:
                data = b'''#!/bin/sh
[ "$1" != install ] || touch "$RECORD"
if [ "$1" = setup ]; then
  [ -t 0 ] || exit 91
  echo SETUP_PROMPT
  read -r answer
  printf '%s' "$answer" > "$RECORD.setup"
fi
echo "opendesk 0.1.1"
'''
                entry = tarfile.TarInfo("opendesk")
                entry.size = len(data)
                entry.mode = 0o755
                archive.addfile(entry, io.BytesIO(data))
            digest = hashlib.sha256(asset.read_bytes()).hexdigest()
            (stage / "sum").write_text(f'{digest if failure != "checksum" else "0" * 64}  open-desktop-linux-x86_64.tar.gz\n')
            stubs = {
                "id": 'echo 1000',
                "uname": 'case "$1" in -s) echo Linux;; -m) echo x86_64;; esac',
                "ldd": '[ "$FAILURE" != dependencies ] || { echo "libfoo => not found"; exit 0; }; echo libc.so',
                "curl": '''[ "$FAILURE" != download ] || exit 22
case "$*" in *--proto*https*--tlsv1.2*) ;; *) exit 90;; esac
while [ "$#" -gt 0 ]; do
 case "$1" in https:*) url=$1;; -o) shift; dest=$1;; esac
 shift
done
case "$url" in */SHA256SUMS) cp "$FIXTURE/sum" "$dest";; *) cp "$FIXTURE/asset.tar.gz" "$dest";; esac''',
            }
            for name, content in stubs.items():
                path = binaries / name
                path.write_text('#!/bin/sh\nset -eu\n' + content + '\n')
                path.chmod(0o755)
            # Replace only OS identification with a fixture; all download and
            # checksum handling comes unmodified from the production bootstrap.
            script = (ROOT / "install.sh").read_text().replace('. /etc/os-release', 'ID=arch; ID_LIKE=arch')
            env = dict(os.environ, PATH=f'{binaries}:{os.environ["PATH"]}', FIXTURE=str(stage),
                       RECORD=str(stage / "installed"), FAILURE=failure, OPENDESK_NO_SETUP="1" if skip_setup else "0")
            if interactive:
                (stage / 'bootstrap.sh').write_text(script)
                result = self.run_in_terminal(env)
            else:
                result = subprocess.run(['sh', '-s', '--', version], input=script, text=True,
                                        capture_output=True, env=env, timeout=15, start_new_session=True)
            result.setup_answer = (stage / 'installed.setup').read_text() if (stage / 'installed.setup').exists() else None
            return result, (stage / "installed").exists()

    def run_in_terminal(self, env):
        pid, terminal = pty.fork()
        if pid == 0:
            os.execvpe('sh', ['sh', '-c', 'cat "$FIXTURE/bootstrap.sh" | sh'], env)
        output = b''
        answered = False
        status = None
        try:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if select.select([terminal], [], [], .1)[0]:
                    try:
                        output += os.read(terminal, 65536)
                    except OSError:
                        break
                if b'SETUP_PROMPT' in output and not answered:
                    os.write(terminal, b'pair-now\n')
                    answered = True
                done, status = os.waitpid(pid, os.WNOHANG)
                if done:
                    pid = None
                    break
            else:
                self.fail('Interactive installer timed out')
            if pid is not None:
                _, status = os.waitpid(pid, 0)
                pid = None
            return subprocess.CompletedProcess('curl-style-pipe', os.waitstatus_to_exitcode(status), output.decode(), output.decode())
        finally:
            if pid is not None:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
            os.close(terminal)

    def test_pipe_opens_setup_and_reads_terminal(self):
        result, installed = self.run_bootstrap(interactive=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(installed)
        self.assertEqual(result.setup_answer, 'pair-now')

    def test_update_can_skip_setup_even_with_terminal(self):
        result, installed = self.run_bootstrap(interactive=True, skip_setup=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(installed)
        self.assertIsNone(result.setup_answer)


    def test_installs_verified_binary_from_pipe(self):
        result, installed = self.run_bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(installed)
        self.assertIsNone(result.setup_answer)
        self.assertIn("Execute em um terminal", result.stdout)

    def test_specific_version(self):
        result, installed = self.run_bootstrap(version="v0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(installed)

    def test_failures_never_install(self):
        for failure in ("download", "checksum", "dependencies"):
            with self.subTest(failure=failure):
                result, installed = self.run_bootstrap(failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(installed)

    def test_invalid_version(self):
        result, installed = self.run_bootstrap(version="v0.1.0/../../bad")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(installed)

if __name__ == '__main__':
    unittest.main()
