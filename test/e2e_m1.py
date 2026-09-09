#!/usr/bin/env python3
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNTIME = Path(os.environ["XDG_RUNTIME_DIR"])
STATE = RUNTIME / "opendesk-e2e"
BINARY = ROOT / "target/debug/opendesk"
INJECT = ROOT / "target/debug/examples/inject"
PORTS = {"alpha": 47831, "beta": 47832}
PLACEMENT = {"alpha": ("beta", "right"), "beta": ("alpha", "left")}
KEY_ESC, KEY_LEFTCTRL, KEY_LEFTALT, KEY_F12, KEY_LEFTMETA = 1, 29, 56, 88, 125
MOD_CTRL, MOD_ALT, MOD_LOGO = 4, 8, 64

results = []


def check(name, passed, detail=""):
    results.append((name, passed))
    print(f"{'PASS' if passed else 'FAIL'}: {name}{' (' + detail + ')' if detail else ''}", flush=True)


def poll(seconds, predicate):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    return predicate()


class Machine:
    def __init__(self, name):
        self.name = name
        self.dir = STATE / name
        self.dir.mkdir(parents=True, exist_ok=True)
        self.socket = self.dir / "ipc.sock"
        self.log = open(self.dir / "daemon.log", "w")
        self.daemon = None
        self.env = dict(os.environ)

    def start_compositor(self):
        output = subprocess.run(
            [ROOT / "test/run-nested-hyprland.sh", self.name],
            capture_output=True, text=True, check=True,
        ).stdout
        for line in output.splitlines():
            if line.startswith("export "):
                key, value = line[len("export "):].split("=", 1)
                self.env[key] = value
        self.env["OPENDESK_CONFIG"] = str(self.dir / "config.toml")
        self.env["OPENDESK_IPC_SOCKET"] = str(self.socket)
        self.env["RUST_LOG"] = "debug"
        (self.dir / "config.toml").write_text(
            f'[general]\nname = "{self.name}"\nport = {PORTS[self.name]}\n'
        )

    def start_daemon(self):
        self.daemon = subprocess.Popen(
            [BINARY, "daemon"], env=self.env, stdout=self.log, stderr=subprocess.STDOUT
        )
        return poll(5, lambda: self.socket.exists())

    def ipc(self, request):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.connect(str(self.socket))
            client.sendall((json.dumps(request) + "\n").encode())
            data = b""
            while not data.endswith(b"\n"):
                chunk = client.recv(65536)
                if not chunk:
                    break
                data += chunk
        return json.loads(data)

    def status(self):
        return self.ipc("Status")["Status"]

    def state(self):
        return self.status()["state"]

    def cli(self, *arguments):
        completed = subprocess.run(
            [BINARY, *arguments], env=self.env, capture_output=True, text=True
        )
        return completed.returncode, (completed.stdout + completed.stderr).strip()

    def hyprctl(self, *arguments):
        return subprocess.run(
            ["hyprctl", *arguments], env=self.env, capture_output=True, text=True, check=True
        ).stdout.strip()

    def cursor(self):
        x, y = self.hyprctl("cursorpos").split(",")
        return float(x), float(y)

    def monitor(self):
        return json.loads(self.hyprctl("monitors", "-j"))[0]

    def inject(self, *steps):
        subprocess.run([INJECT, *steps], env=self.env, check=True, capture_output=True)

    def stop_daemon(self, force=False):
        if self.daemon and self.daemon.poll() is None:
            self.daemon.send_signal(signal.SIGKILL if force else signal.SIGTERM)
            self.daemon.wait(timeout=10)
        return self.daemon.returncode if self.daemon else None

    def stop_compositor(self):
        subprocess.run([ROOT / "test/stop-nested-hyprland.sh", self.name], capture_output=True)


def cross(alpha, beta):
    monitor = alpha.monitor()
    alpha.inject(f"abs:{monitor['width'] - 1},{monitor['height'] / 2}")
    crossed = poll(3, lambda: alpha.state() == "controlling" and beta.state() == "controlled")
    time.sleep(0.45)
    return crossed


def leave_strip(alpha):
    alpha.inject("motion:-25,0")
    time.sleep(0.2)


def run():
    shutil.rmtree(STATE, ignore_errors=True)
    alpha, beta = Machine("alpha"), Machine("beta")
    try:
        for machine in (alpha, beta):
            machine.start_compositor()
        time.sleep(1.0)
        for machine in (alpha, beta):
            check(f"{machine.name}: daemon started", machine.start_daemon())
        found = poll(15, lambda: any(p["name"] == "beta" for p in alpha.ipc("Discover")["Discovered"]))
        check("alpha discovers beta over mDNS", bool(found), json.dumps(alpha.ipc("Discover")))

        check("pair request asks for a PIN", alpha.ipc({"Pair": {"name": "beta"}}) == "PinRequired")
        pin = poll(3, lambda: beta.status()["pending_pin"])
        check("beta exposes the pending PIN in status", bool(pin), f"pin={pin}")
        wrong = alpha.ipc({"SubmitPin": {"pin": "000000"}})
        check("wrong PIN is rejected", "Error" in wrong, json.dumps(wrong))
        right = alpha.ipc({"SubmitPin": {"pin": pin}})
        check("right PIN pairs", right == "Ok", json.dumps(right))
        for machine in (alpha, beta):
            peer, side = PLACEMENT[machine.name]
            code, output = machine.cli("peer", "set", peer, side)
            check(f"{machine.name}: peer set {peer} {side}", code == 0, output)
        connected = poll(15, lambda: all(p["connected"] for p in alpha.status()["peers"]) and all(p["connected"] for p in beta.status()["peers"]))
        check("both daemons report the link as connected", bool(connected), alpha.cli("status")[1])

        check("crossing: alpha controlling, beta controlled", cross(alpha, beta), f"alpha={alpha.state()} beta={beta.state()}")
        entry, beta_monitor, alpha_monitor = beta.cursor(), beta.monitor(), alpha.monitor()
        sizes = f"alpha={alpha_monitor['width']}x{alpha_monitor['height']} beta={beta_monitor['width']}x{beta_monitor['height']}"
        check("beta cursor placed 2 px inside its left edge at the proportional height", entry[0] == 2 and abs(entry[1] - beta_monitor["height"] / 2) <= 3, f"cursor={entry} {sizes}")
        alpha.inject("motion:30,0")
        moved = poll(2, lambda: beta.cursor()[0] >= 31)
        check("relative motion reaches beta", bool(moved), f"cursor={beta.cursor()}")

        time.sleep(0.25)
        marker_beta, marker_alpha = STATE / "marker-beta", STATE / "marker-alpha"
        marker_beta.unlink(missing_ok=True)
        marker_alpha.unlink(missing_ok=True)
        beta.hyprctl("keyword", "bind", f"SUPER, F12, exec, touch {marker_beta}")
        alpha.hyprctl("keyword", "bind", f"SUPER, F12, exec, touch {marker_alpha}")
        alpha.inject(f"key:{KEY_LEFTMETA}:down", f"mods:{MOD_LOGO}", f"key:{KEY_F12}:down", f"key:{KEY_F12}:up", f"key:{KEY_LEFTMETA}:up", "mods:0")
        check("SUPER+F12 typed on alpha fires the bind on beta", bool(poll(2, marker_beta.exists)))
        check("SUPER+F12 does not fire the bind on alpha", not marker_alpha.exists())

        beta_before = beta.cursor()
        for _ in range(10):
            alpha.inject("motion:-60,0")
            if poll(0.4, lambda: beta.cursor()[0] <= 1):
                break
        beta_at_edge = beta.cursor()
        returned = poll(3, lambda: alpha.state() == "idle" and beta.state() == "idle")
        check("crossing back beta's left edge returns control", bool(returned), f"alpha={alpha.state()} beta={beta.state()} before={beta_before} at_edge={beta_at_edge}")
        alpha_monitor = alpha.monitor()
        back = alpha.cursor()
        check("alpha cursor is back on its right edge", back[0] == alpha_monitor["width"] - 1, f"cursor={back}")

        leave_strip(alpha)
        check("crossing again after leaving the strip", cross(alpha, beta))
        alpha.inject(f"key:{KEY_LEFTCTRL}:down", f"mods:{MOD_CTRL}", f"key:{KEY_LEFTALT}:down", f"mods:{MOD_CTRL | MOD_ALT}", f"key:{KEY_ESC}:down", f"key:{KEY_ESC}:up", f"key:{KEY_LEFTALT}:up", f"mods:{MOD_CTRL}", f"key:{KEY_LEFTCTRL}:up", "mods:0")
        released = poll(3, lambda: alpha.state() == "idle" and beta.state() == "idle")
        check("ctrl+alt+escape releases control", bool(released), f"alpha={alpha.state()} beta={beta.state()}")

        leave_strip(alpha)
        check("crossing a third time", cross(alpha, beta))
        beta.stop_daemon(force=True)
        check("alpha returns to idle when beta dies", bool(poll(5, lambda: alpha.state() == "idle")), f"alpha={alpha.state()}")
        code = alpha.stop_daemon()
        check("SIGTERM stops alpha cleanly and removes the socket", code == 0 and not alpha.socket.exists(), f"exit={code}")
    finally:
        for machine in (alpha, beta):
            machine.stop_daemon(force=True)
            machine.stop_compositor()
            machine.log.close()
    failed = [name for name, passed in results if not passed]
    print(f"\n{len(results) - len(failed)}/{len(results)} checks passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(run())
