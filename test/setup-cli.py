#!/usr/bin/env python3
"""Exercise real CLI prompts in a PTY against an isolated IPC peer."""
import json
import os
from pathlib import Path
import pty
import select
import socket
import subprocess
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/opendesk'
PEER = dict(name='notebook', side=None, connected=True, address='192.0.2.2:47820')

class Setup(unittest.TestCase):
    def scenario(self, mode, choices):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            stub = stage / 'systemctl'
            stub.write_text('#!/bin/sh\nexit 0\n')
            stub.chmod(0o755)
            server = socket.socket(socket.AF_UNIX)
            address = stage / 'ipc.sock'
            server.bind(str(address))
            server.listen()
            server.settimeout(.1)
            stop = threading.Event()
            requests = []
            status_calls = 0
            paired = mode == 'existing'

            def serve():
                nonlocal status_calls, paired
                while not stop.is_set():
                    try:
                        connection, _ = server.accept()
                    except socket.timeout:
                        continue
                    with connection, connection.makefile('r') as stream:
                        request = json.loads(stream.readline())
                        requests.append(request)
                        if request == 'Status':
                            status_calls += 1
                            if mode == 'receive' and status_calls >= 6:
                                paired = True
                            response = {'Status': dict(name='desktop', peer_id='00', state='Idle', enabled=True,
                                peers=[PEER] if paired else [],
                                pending_pin='123456' if mode == 'receive' and 3 <= status_calls < 6 else None)}
                        elif request == 'Discover':
                            response = {'Discovered': [dict(name='notebook', peer_id='11', address='192.0.2.2:47820', version='0.1.0', paired=False)]}
                        elif isinstance(request, dict) and 'Pair' in request:
                            response = 'PinRequired'
                        else:
                            if isinstance(request, dict) and 'SubmitPin' in request:
                                paired = True
                            response = 'Ok'
                        connection.sendall(json.dumps(response).encode() + b'\n')

            worker = threading.Thread(target=serve)
            worker.start()
            master, slave = pty.openpty()
            env = dict(os.environ, PATH=f'{stage}:{os.environ["PATH"]}', OPENDESK_IPC_SOCKET=str(address))
            process = subprocess.Popen([BINARY, 'setup'], stdin=slave, stdout=slave, stderr=slave, env=env)
            os.close(slave)
            output = b''
            # Feed choices as lines; PTY preserves terminal behavior and buffering.
            os.write(master, ('\n'.join(choices) + '\n').encode())
            try:
                deadline = time.monotonic() + 12
                while process.poll() is None and time.monotonic() < deadline:
                    if select.select([master], [], [], .1)[0]:
                        try:
                            output += os.read(master, 65536)
                        except OSError:
                            break
                process.wait(timeout=2)
                while select.select([master], [], [], 0)[0]:
                    try:
                        output += os.read(master, 65536)
                    except OSError:
                        break
                return process.returncode, output.decode(), requests
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                os.close(master)
                stop.set()
                worker.join(timeout=2)
                server.close()

    def test_initiator_submits_pin_and_places_peer(self):
        code, output, requests = self.scenario('initiate', ['1', '1', '123456'])
        self.assertEqual(code, 0, output)
        self.assertIn({'SubmitPin': {'pin': '123456'}}, requests)
        self.assertIn({'PeerSet': {'name': 'notebook', 'side': 'all'}}, requests)

    def test_receiver_displays_pin_and_places_peer(self):
        code, output, requests = self.scenario('receive', ['2'])
        self.assertEqual(code, 0, output)
        self.assertIn('PIN para digitar no outro computador: 123456', output)
        self.assertIn({'PeerSet': {'name': 'notebook', 'side': 'all'}}, requests)

    def test_existing_peer_never_repairs(self):
        code, output, requests = self.scenario('existing', ['3', 'notebook'])
        self.assertEqual(code, 0, output)
        self.assertNotIn({'Pair': {'name': 'notebook'}}, requests)

    def test_invalid_selection_does_not_pair(self):
        code, output, requests = self.scenario('initiate', ['1', '9'])
        self.assertNotEqual(code, 0, output)
        self.assertNotIn({'Pair': {'name': 'notebook'}}, requests)

    def test_setup_rejects_pipe_before_starting_service(self):
        result = subprocess.run([BINARY, 'setup'], input='', capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('em um terminal', result.stderr)

if __name__ == '__main__':
    unittest.main()
