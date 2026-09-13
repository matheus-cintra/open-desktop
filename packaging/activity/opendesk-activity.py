#!/usr/bin/python3
"""Physical activity notifier. No key codes or coordinates leave this process."""
import fcntl
import glob
import os
import selectors
import socket
import struct
import subprocess
import time

EVENT = struct.Struct('@llHHi')
CREDENTIALS = struct.Struct('3i')


class Gesture:
    def __init__(self, scales=None):
        self.last = 0.0
        self.distance = 0
        self.sent = False
        self.absolute = {}
        self.scales = scales or {}

    def feed(self, kind, code, value, now):
        if kind == 3 and code in (47, 57):  # slot or tracking ID: a new contact
            self.absolute.clear()
            return False
        if kind == 1:
            return (code < 256 or 272 <= code <= 279) and value == 1  # no repeats or isolated releases
        if kind not in (2, 3) or code not in (0, 1, 53, 54):
            return False
        if kind == 3:
            previous = self.absolute.get(code)
            self.absolute[code] = value
            if previous is None:
                return False
            value = (value - previous) * self.scales.get(code, 1.0)
        if not value:
            return False
        if now - self.last >= .250:
            self.distance = 0
            self.sent = False
        self.last = now
        self.distance += abs(value)
        if self.distance >= 12 and not self.sent:
            self.sent = True
            return True
        return False


def active_sessions():
    """Fail closed if logind cannot prove an active, local, unlocked session."""
    result = {}
    try:
        rows = subprocess.check_output(['loginctl', 'list-sessions', '--no-legend', '--no-pager'], timeout=.5, text=True)
        for row in rows.splitlines():
            session = row.split()[0]
            raw = subprocess.check_output(['loginctl', 'show-session', session, '--no-pager', '-p', 'User', '-p', 'Active', '-p', 'Remote', '-p', 'LockedHint', '-p', 'Type', '-p', 'Class', '-p', 'Seat'], timeout=.5, text=True)
            props = dict(line.split('=', 1) for line in raw.splitlines() if '=' in line)
            if (props.get('Active') == 'yes' and props.get('Remote') == 'no'
                    and props.get('LockedHint') == 'no' and props.get('Class') == 'user'
                    and props.get('Type') in ('wayland', 'x11')):
                seat = props.get('Seat')
                if seat:
                    result.setdefault(seat, set()).add(int(props['User']))
    except (OSError, ValueError, subprocess.SubprocessError, IndexError):
        return {}
    return result


def active_uids():
    return set().union(*active_sessions().values())


def device_seat(path):
    dev = os.stat(path).st_rdev
    try:
        with open(f'/run/udev/data/c{os.major(dev)}:{os.minor(dev)}') as stream:
            for line in stream:
                if line.startswith('E:ID_SEAT='):
                    return line.strip().split('=', 1)[1]
    except FileNotFoundError:
        pass
    return 'seat0'


def absolute_scales(fd):
    result = {}
    for code in (0, 1, 53, 54):
        try:
            raw = fcntl.ioctl(fd, 0x80184540 + code, bytes(24))  # EVIOCGABS
            _, minimum, maximum, _, _, _ = struct.unpack('6i', raw)
            if maximum > minimum:
                result[code] = 1000.0 / (maximum - minimum)
        except OSError:
            pass
    return result


def authorized(client, allowed):
    try:
        pid, uid, _ = CREDENTIALS.unpack(client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, CREDENTIALS.size))
        return pid > 0 and uid >= 1000 and uid in allowed
    except (OSError, ValueError):
        return False


def physical(path):
    sys_path = '/sys/class/input/' + os.path.basename(path) + '/device'
    real = os.path.realpath(sys_path)
    return os.path.exists(sys_path) and '/virtual/' not in real and real.startswith('/sys/devices/')


def run():
    selector = selectors.DefaultSelector()
    path = '/run/opendesk-activity/activity.sock'
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    if os.path.exists(path):
        os.unlink(path)
    server.bind(path)
    os.chmod(path, 0o666)  # every connection is checked with SO_PEERCRED
    server.listen(16)
    server.setblocking(False)
    selector.register(server, selectors.EVENT_READ, None)
    clients = set()
    devices = {}
    allowed = {}
    refresh = 0.0
    while True:
        now = time.monotonic()
        if now >= refresh:
            allowed = active_sessions()
            refresh = time.monotonic() + .100
            for client in list(clients):
                if not authorized(client, set().union(*allowed.values())):
                    clients.remove(client)
                    client.close()
            for device in glob.glob('/dev/input/event*'):
                if device in devices or not physical(device):
                    continue
                try:
                    seat = device_seat(device)
                    fd = os.open(device, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
                    devices[device] = fd
                    selector.register(fd, selectors.EVENT_READ, (device, seat, Gesture(absolute_scales(fd))))
                except OSError:
                    continue
        requested = set()
        for key, _ in selector.select(.025):
            if key.fileobj is server:
                client, _ = server.accept()
                client.setblocking(False)
                if len(clients) < 32 and authorized(client, set().union(*allowed.values())):
                    clients.add(client)
                else:
                    client.close()
                continue
            device, seat, gesture = key.data
            try:
                data = os.read(key.fd, EVENT.size * 64)
                if not data:
                    raise OSError('device disconnected')
                # Discard input while inactive; a new gesture must start on unlock.
                if not allowed.get(seat):
                    gesture.__init__(gesture.scales)
                    continue
                for _, _, kind, code, value in EVENT.iter_unpack(data):
                    if gesture.feed(kind, code, value, time.monotonic()):
                        requested.add(seat)
            except BlockingIOError:
                continue
            except (OSError, struct.error):
                selector.unregister(key.fd)
                os.close(key.fd)
                devices.pop(device, None)
        if requested:
            # Refresh at emission, not just subscription; never publish cached
            # input when a user has locked or switched sessions.
            allowed = active_sessions()
            for client in list(clients):
                try:
                    if not authorized(client, set().union(*(allowed.get(seat, set()) for seat in requested))):
                        raise OSError('inactive')
                    client.sendall(b'A')
                except OSError:
                    clients.remove(client)
                    client.close()


if __name__ == '__main__':
    run()
