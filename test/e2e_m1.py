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

import secrets
ROOT = Path(__file__).resolve().parent.parent
RUN_TAG = secrets.token_hex(2)
RUNTIME = Path(os.environ["XDG_RUNTIME_DIR"])
STATE = RUNTIME / "opendesk-e2e"
BINARY = ROOT / "target/debug/opendesk"
INJECT = ROOT / "target/debug/examples/inject"
PORTS = {"alpha": 47831, "beta": 47832}
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
        self.service_name = f"{name}-{RUN_TAG}"
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
            f'[general]\nname = "{self.service_name}"\nport = {PORTS[self.name]}\n'
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

    def wl_copy(self, text=None, mime=None, data=None):
        if data is not None:
            subprocess.run(["wl-copy", "-t", mime], env=self.env, input=data, check=True)
        else:
            subprocess.run(["wl-copy", text], env=self.env, check=True)

    def wl_paste(self, mime=None):
        args = ["wl-paste", "-n"] if mime is None else ["wl-paste", "-t", mime]
        result = subprocess.run(args, env=self.env, capture_output=True)
        return result.stdout

    def stop_daemon(self, force=False):
        if self.daemon and self.daemon.poll() is None:
            self.daemon.send_signal(signal.SIGKILL if force else signal.SIGTERM)
            self.daemon.wait(timeout=10)
        return self.daemon.returncode if self.daemon else None

    def stop_compositor(self):
        subprocess.run([ROOT / "test/stop-nested-hyprland.sh", self.name], capture_output=True)


PUSH_THRESHOLD_PX = 60


def touch_edge(alpha):
    monitor = alpha.monitor()
    alpha.inject(f"abs:{monitor['width'] - 1},{monitor['height'] / 2}")
    return poll(2, lambda: alpha.state() == "pushing")


def cross(alpha, beta):
    touch_edge(alpha)
    alpha.inject(f"motion:{PUSH_THRESHOLD_PX + 10},0")
    crossed = poll(3, lambda: alpha.state() == "controlling" and beta.state() == "controlled")
    time.sleep(0.15)
    return crossed


def edge_bar_rows(machine, side):
    output = machine.monitor()
    ppm = subprocess.run(
        ["grim", "-t", "ppm", "-o", output["name"], "-"], env=machine.env, capture_output=True, check=True
    ).stdout
    header, _, pixels = ppm.partition(b"255\n")
    width, height = (int(v) for v in header.split()[1:3])
    def pixel(x, y):
        offset = (y * width + x) * 3
        return pixels[offset:offset + 3]
    reference = pixel(width // 2, height // 2)
    columns = range(width - 3, width) if side == "right" else range(0, 3)
    rows = 0
    for y in range(max(0, height // 2 - 250), min(height, height // 2 + 250)):
        if any(max(abs(a - b) for a, b in zip(pixel(x, y), reference)) > 40 for x in columns):
            rows += 1
    return rows


def screenshot(machine, name):
    path = STATE / f"{machine.name}-{name}.png"
    subprocess.run(["grim", "-o", machine.monitor()["name"], str(path)], env=machine.env, check=True)
    return path


def leave_strip(alpha):
    alpha.inject("motion:-25,0")
    time.sleep(0.2)


def run():
    shutil.rmtree(STATE, ignore_errors=True)
    alpha, beta = Machine("alpha"), Machine("beta")
    try:
        for machine in (alpha, beta):
            machine.start_compositor()
            machine.hyprctl("keyword", "layerrule", "no_anim on, match:namespace ^opendesk-bar$")
        time.sleep(1.0)
        for machine in (alpha, beta):
            check(f"{machine.name}: daemon started", machine.start_daemon())
        found = poll(15, lambda: any(p["name"] == beta.service_name for p in alpha.ipc("Discover")["Discovered"]))
        check("alpha discovers beta over mDNS", bool(found), json.dumps(alpha.ipc("Discover")))

        check("pair request asks for a PIN", alpha.ipc({"Pair": {"name": beta.service_name}}) == "PinRequired")
        pin = poll(3, lambda: beta.status()["pending_pin"])
        check("beta exposes the pending PIN in status", bool(pin), f"pin={pin}")
        wrong = alpha.ipc({"SubmitPin": {"pin": "000000"}})
        check("wrong PIN is rejected", "Error" in wrong, json.dumps(wrong))
        right = alpha.ipc({"SubmitPin": {"pin": pin}})
        check("right PIN pairs", right == "Ok", json.dumps(right))
        placement = {alpha.name: (beta.service_name, "right"), beta.name: (alpha.service_name, "left")}
        for machine in (alpha, beta):
            peer, side = placement[machine.name]
            code, output = machine.cli("peer", "set", peer, side)
            check(f"{machine.name}: peer set {peer} {side}", code == 0, output)
        connected = poll(15, lambda: all(p["connected"] for p in alpha.status()["peers"]) and all(p["connected"] for p in beta.status()["peers"]))
        check("both daemons report the link as connected", bool(connected), alpha.cli("status")[1])

        text_a = f"clip-a-{RUN_TAG}"
        alpha.wl_copy(text=text_a)
        check("text copied on alpha appears on beta", bool(poll(3, lambda: beta.wl_paste().decode(errors="replace") == text_a)), f"beta={beta.wl_paste()!r}")
        text_b = f"clip-b-{RUN_TAG}"
        beta.wl_copy(text=text_b)
        check("text copied on beta appears on alpha", bool(poll(3, lambda: alpha.wl_paste().decode(errors="replace") == text_b)), f"alpha={alpha.wl_paste()!r}")
        png = bytes.fromhex(
            "89504e470d0a1a0a0000000d4948445200000001000000010802000000907753"
            "de0000000c4944415478da63a8b79f05000299015926bfe5800000000049454e44ae426082"
        )
        alpha.wl_copy(mime="image/png", data=png)
        check("png copied on alpha appears byte-identical on beta", bool(poll(3, lambda: beta.wl_paste(mime="image/png") == png)), f"beta_len={len(beta.wl_paste(mime='image/png'))}")

        check("touching the edge starts pushing", touch_edge(alpha), f"alpha={alpha.state()}")
        monitor = alpha.monitor()
        alpha.inject(f"abs:{monitor['width'] // 2},{monitor['height'] // 2}")
        check("moving away from the edge cancels the push", bool(poll(2, lambda: alpha.state() == "idle")), f"alpha={alpha.state()}")
        leave_strip(alpha)
        check("touching the edge again starts pushing", touch_edge(alpha))
        alpha.inject("motion:30,0")
        time.sleep(0.3)
        half_rows = edge_bar_rows(alpha, "right")
        half_png = screenshot(alpha, "bar-half")
        check("half-way push shows a bar of about 130 px on the source edge", alpha.state() == "pushing" and 110 <= half_rows <= 150, f"rows={half_rows} state={alpha.state()} png={half_png}")
        alpha.inject("motion:40,0")
        crossed = poll(3, lambda: alpha.state() == "controlling" and beta.state() == "controlled")
        arrival_rows = edge_bar_rows(beta, "left")
        arrival_png = screenshot(beta, "arrival")
        check("pushing past the threshold crosses", bool(crossed), f"alpha={alpha.state()} beta={beta.state()}")
        check("arrival bar of about 220 px shows on the destination edge", 190 <= arrival_rows <= 250, f"rows={arrival_rows} png={arrival_png}")
        time.sleep(0.9)
        check("source progress bar is gone after crossing", edge_bar_rows(alpha, "right") == 0)
        faded_rows = edge_bar_rows(beta, "left")
        check("arrival bar fades out (cursor at the edge aside)", faded_rows < 40, f"rows={faded_rows}")
        time.sleep(0.15)
        entry, beta_monitor, alpha_monitor = beta.cursor(), beta.monitor(), alpha.monitor()
        sizes = f"alpha={alpha_monitor['width']}x{alpha_monitor['height']} beta={beta_monitor['width']}x{beta_monitor['height']}"
        check("beta cursor placed just inside its left edge at the proportional height", 1 <= entry[0] <= 3 and abs(entry[1] - beta_monitor["height"] / 2) <= 3, f"cursor={entry} {sizes}")
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

        time.sleep(0.4)
        beta_before = beta.cursor()
        beta.inject(f"abs:0,{beta_monitor['height'] / 2}")
        returned = poll(3, lambda: alpha.state() == "idle" and beta.state() == "idle")
        beta_at_edge = beta.cursor()
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
