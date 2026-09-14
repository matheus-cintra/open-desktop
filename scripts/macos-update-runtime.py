"""macOS process and persisted-state operations, shared by install and recovery."""
import ctypes
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

LSREGISTER = '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister'


def run(*args, timeout=30):
    return subprocess.check_output([str(a) for a in args], stderr=subprocess.STDOUT,
                                   timeout=timeout, text=True).strip()


def copy(source, target):
    run('/usr/bin/ditto', source, target, timeout=180)


def processes(app):
    binary = str(app / 'Contents/MacOS/opendesk')
    lib = ctypes.CDLL('/usr/lib/libproc.dylib')
    found = []
    for line in run('/bin/ps', '-axo', 'pid=,command=').splitlines():
        pid, command = line.strip().split(None, 1)
        buf = ctypes.create_string_buffer(4096)
        if lib.proc_pidpath(int(pid), buf, len(buf)) <= 0 or os.fsdecode(buf.value) != binary:
            continue
        if command in (binary, binary + ' daemon', binary + ' gui') or command.startswith(binary + ' --opendesk-permission-'):
            found.append((int(pid), command == binary + ' gui'))
    return found


def stop(app):
    deadline = time.monotonic() + 10
    active = processes(app)
    # GUI is an eframe child, not the AppKit app delegate. Target only verified PIDs.
    for pid, gui in active:
        if gui:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
    if any(not gui for _, gui in active):
        escaped = str(app).replace('\\', '\\\\').replace('"', '\\"')
        run('/usr/bin/osascript', '-e', 'with timeout of 5 seconds',
            '-e', f'tell application "{escaped}" to quit', '-e', 'end timeout', timeout=6)
    while processes(app):
        if time.monotonic() >= deadline:
            raise RuntimeError('Installed processes did not stop; refusing replacement')
        time.sleep(.2)


def healthy(app, home, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        owners = {pid for pid, gui in processes(app) if not gui}
        if owners:
            try:
                with socket.socket(socket.AF_UNIX) as sock:
                    sock.settimeout(min(1, max(.01, deadline - time.monotonic())))
                    sock.connect(str(home / 'Library/Application Support/Open Desktop/ipc.sock'))
                    # Darwin sys/un.h: SOL_LOCAL=0, LOCAL_PEERPID=2.
                    if sock.getsockopt(0, 2) not in owners:
                        raise OSError('IPC belongs to a different process')
                    sock.sendall(b'"Status"\n')
                    reply = b''
                    while b'\n' not in reply and len(reply) < 1024 * 1024:
                        chunk = sock.recv(65536)
                        if not chunk:
                            break
                        reply += chunk
                    if isinstance(json.loads(reply).get('Status'), dict):
                        return
            except (OSError, ValueError):
                pass
        time.sleep(.2)
    raise RuntimeError('Installed app did not answer IPC within 30 seconds')


def launch(app, home, gui):
    run(LSREGISTER, '-f', app)
    run('/usr/bin/codesign', '--verify', '--deep', '--strict', app)
    run('/usr/bin/open', app)
    healthy(app, home)
    if gui:
        log = home / 'Library/Logs/Open Desktop'
        log.mkdir(parents=True, exist_ok=True)
        with (log / 'updater-gui.log').open('ab') as stream:
            subprocess.Popen([str(app / 'Contents/MacOS/opendesk'), 'gui'],
                             stdout=stream, stderr=stream, start_new_session=True)
        deadline = time.monotonic() + 5
        while not any(is_gui for _, is_gui in processes(app)):
            if time.monotonic() >= deadline:
                raise RuntimeError('Organization window process did not reopen')
            time.sleep(.2)


def save_json(path, value):
    temporary = path.with_suffix('.tmp')
    with temporary.open('w') as stream:
        json.dump(value, stream)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def backup_preferences(path):
    try:
        run('/usr/bin/defaults', 'export', 'dev.mcintra.opendesk', path)
    except subprocess.CalledProcessError as error:
        if 'does not exist' not in error.output:
            raise


def restore_preferences(path):
    if path.exists():
        run('/usr/bin/defaults', 'import', 'dev.mcintra.opendesk', path)
    else:
        try:
            run('/usr/bin/defaults', 'delete', 'dev.mcintra.opendesk')
        except subprocess.CalledProcessError as error:
            if 'not found' not in error.output and 'does not exist' not in error.output:
                raise
