#!/usr/bin/env python3
"""Nested-Hyprland acceptance test for the four-edge `all` peer placement.

Requires the debug binary and examples (`cargo build --examples`) and a running
host Wayland session.  It only starts nested Hyprland instances.
"""
import json
import os
import select
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
RUNTIME = Path(os.environ["XDG_RUNTIME_DIR"])
STATE = RUNTIME / "opendesk-e2e-all"
TAG = secrets.token_hex(2)
BINARY = ROOT / "target/debug/opendesk"
INJECT = ROOT / "target/debug/examples/inject"
DRAG_SOURCE = ROOT / "target/debug/examples/drag_source"
DROP_TARGET = ROOT / "target/debug/examples/drop_target"
BTN_LEFT = 272
PORTS = {"alpha": 47841, "beta": 47842}
KEY_ESC, KEY_LEFTCTRL, KEY_LEFTALT = 1, 29, 56
MOD_CTRL, MOD_ALT = 4, 8
THRESHOLD = 70
RESULTS = []


def check(name, passed, detail=""):
    RESULTS.append((name, passed))
    print(f"{'PASS' if passed else 'FAIL'}: {name}{f' ({detail})' if detail else ''}", flush=True)


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
        self.env = dict(os.environ)
        self.dir = STATE / name
        self.dir.mkdir(parents=True, exist_ok=True)
        self.socket = self.dir / "ipc.sock"
        self.dnd_dir = self.dir / "dnd"
        self.log = open(self.dir / "daemon.log", "w")
        self.daemon = None
        self.injector = None
        self.service_name = f"{name}-{TAG}"

    def start_compositor(self):
        output = subprocess.run(
            [ROOT / "test/run-nested-hyprland.sh", self.name],
            check=True, capture_output=True, text=True,
            env={**os.environ, "OPENDESK_NESTED_CONFIG": str(ROOT / "test/hyprland-release-probe.lua")},
        ).stdout
        for line in output.splitlines():
            if line.startswith("export "):
                key, value = line[7:].split("=", 1)
                self.env[key] = value
        self.env["OPENDESK_CONFIG"] = str(self.dir / "config.toml")
        self.env["OPENDESK_IPC_SOCKET"] = str(self.socket)
        self.env["RUST_LOG"] = "debug"
        (self.dir / "config.toml").write_text(
            f'[general]\nname = "{self.service_name}"\nport = {PORTS[self.name]}\n'
            f'dnd_dir = "{self.dnd_dir}"\n'
        )

    def start_daemon(self):
        self.daemon = subprocess.Popen([BINARY, "daemon"], env=self.env, stdout=self.log, stderr=subprocess.STDOUT)
        return poll(5, self.socket.exists)

    def stop(self):
        if self.injector and self.injector.poll() is None:
            self.injector.stdin.close()
            self.injector.wait(timeout=5)
        if self.daemon and self.daemon.poll() is None:
            self.daemon.send_signal(signal.SIGKILL)
            self.daemon.wait(timeout=10)
        subprocess.run([ROOT / "test/stop-nested-hyprland.sh", self.name], capture_output=True)
        self.log.close()

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
        result = subprocess.run([BINARY, *arguments], env=self.env, capture_output=True, text=True)
        return result.returncode, (result.stdout + result.stderr).strip()

    def hyprctl(self, *arguments):
        return subprocess.run(["hyprctl", *arguments], env=self.env, check=True, capture_output=True, text=True).stdout.strip()

    def monitor(self):
        return json.loads(self.hyprctl("monitors", "-j"))[0]

    def cursor(self):
        x, y = self.hyprctl("cursorpos").split(",")
        return float(x), float(y)

    def inject(self, *steps):
        if self.injector is None:
            self.injector = self.persistent_injector()
        persistent_step(self.injector, *steps)

    def persistent_injector(self):
        process = subprocess.Popen([INJECT, "--stdin"], env=self.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        if injector_response(process) != "inject ready":
            raise RuntimeError("persistent injector did not become ready")
        return process

    def copy(self, text=None, mime=None, data=None):
        if data is None:
            subprocess.run(["wl-copy", text], env=self.env, check=True)
        else:
            subprocess.run(["wl-copy", "-t", mime], env=self.env, input=data, check=True)

    def paste(self, mime=None):
        command = ["wl-paste", "-n"] if mime is None else ["wl-paste", "-t", mime]
        return subprocess.run(command, env=self.env, capture_output=True).stdout


def point_on_edge(machine, side, fraction=0.5):
    monitor = machine.monitor()
    width, height = monitor["width"], monitor["height"]
    if side == "left":
        return 0, round(height * fraction)
    if side == "right":
        return width - 1, round(height * fraction)
    if side == "top":
        return round(width * fraction), 0
    return round(width * fraction), height - 1


def move_to_edge(machine, side, fraction=0.5):
    x, y = point_on_edge(machine, side, fraction)
    machine.inject(f"abs:{x},{y}")


def cross(source, target, side, fraction=0.5):
    move_to_edge(source, side, fraction)
    pushed = poll(2, lambda: source.state() == "pushing")
    dx, dy = {"left": (-THRESHOLD, 0), "right": (THRESHOLD, 0), "top": (0, -THRESHOLD), "bottom": (0, THRESHOLD)}[side]
    source.inject(f"motion:{dx},{dy}")
    active = poll(3, lambda: source.state() == "controlling" and target.state() == "controlled")
    return pushed and active


def edge_at(machine):
    x, y = machine.cursor()
    monitor = machine.monitor()
    near = []
    if x <= 3:
        near.append("left")
    if x >= monitor["width"] - 3:
        near.append("right")
    if y <= 3:
        near.append("top")
    if y >= monitor["height"] - 3:
        near.append("bottom")
    return near


def return_control(source, controlled, return_edge):
    time.sleep(0.35)  # receiver grace: do not turn arrival into an immediate return
    move_to_edge(controlled, return_edge)
    return poll(3, lambda: source.state() == "idle" and controlled.state() == "idle")


def received_file(directory, name):
    return next(directory.rglob(name), None) if directory.exists() else None


def line_seen(process, needle):
    buffer = getattr(process, "_buffer", "")
    while True:
        ready, _, _ = select.select([process.stdout], [], [], 0)
        if not ready:
            break
        chunk = os.read(process.stdout.fileno(), 4096).decode(errors="replace")
        if not chunk:
            break
        buffer += chunk
    process._buffer = buffer
    return needle in buffer


def injector_response(process):
    if not select.select([process.stdout], [], [], 5)[0]:
        raise TimeoutError("persistent injector response timed out")
    return process.stdout.readline().strip()


def persistent_step(process, *steps):
    for step in steps:
        process.stdin.write(f"{step}\n")
        process.stdin.flush()
        if injector_response(process) != "ok":
            raise RuntimeError(f"persistent injector rejected {step}")


def release(source):
    source.inject(
        f"key:{KEY_LEFTCTRL}:down", f"mods:{MOD_CTRL}",
        f"key:{KEY_LEFTALT}:down", f"mods:{MOD_CTRL | MOD_ALT}",
        f"key:{KEY_ESC}:down", f"key:{KEY_ESC}:up", f"key:{KEY_LEFTALT}:up",
        f"mods:{MOD_CTRL}", f"key:{KEY_LEFTCTRL}:up", "mods:0",
    )
    return poll(3, lambda: source.state() == "idle")


def run():
    shutil.rmtree(STATE, ignore_errors=True)
    alpha, beta = Machine("alpha"), Machine("beta")
    try:
        for machine in (alpha, beta):
            machine.start_compositor()
        beta_monitor = beta.monitor()
        beta.hyprctl("eval", f'hl.monitor({{ output = "{beta_monitor["name"]}", mode = "1024x640@60", position = "0x0", scale = 1 }})')
        resized = poll(3, lambda: beta.monitor()["width"] == 1024 and beta.monitor()["height"] == 640)
        check("nested beta monitor changes to a different resolution", bool(resized), json.dumps(beta.monitor()))
        for machine in (alpha, beta):
            monitor = machine.monitor()
            machine.inject(f"abs:{monitor['width'] // 2},{monitor['height'] // 2}")
        for machine in (alpha, beta):
            check(f"{machine.name}: daemon starts", machine.start_daemon())

        found = poll(15, lambda: any(peer["name"] == beta.service_name for peer in alpha.ipc("Discover")["Discovered"]))
        check("alpha discovers beta", bool(found))
        check("pair request requires a PIN", alpha.ipc({"Pair": {"name": beta.service_name}}) == "PinRequired")
        pin = poll(3, lambda: beta.status()["pending_pin"])
        check("beta exposes a pairing PIN", bool(pin))
        check("pairing with beta PIN succeeds", alpha.ipc({"SubmitPin": {"pin": pin}}) == "Ok")
        code, output = alpha.cli("peer", "set", beta.service_name, "--side", "all")
        check("alpha: --side all configures every edge", code == 0, output)
        alpha_status = json.dumps(alpha.status())
        check("alpha: status renders all placement", '"side": "all"' in alpha_status, alpha_status)
        linked = poll(15, lambda: all(peer["connected"] for peer in alpha.status()["peers"]) and all(peer["connected"] for peer in beta.status()["peers"]))
        check("all-mode peers connect", bool(linked))

        check("a peer without reciprocal placement receives an all-mode handoff", cross(alpha, beta, "right"), f"alpha={alpha.state()} beta={beta.state()}")
        check("the unplaced receiver exposes only the recorded return edge", edge_at(beta) == ["left"], f"beta_edges={edge_at(beta)}")
        check("the unplaced receiver can return through its recorded edge", return_control(alpha, beta, "left"), f"alpha={alpha.state()} beta={beta.state()}")

        code, output = beta.cli("peer", "set", alpha.service_name, "--side", "all")
        check("beta: --side all configures every edge", code == 0, output)
        beta_status = json.dumps(beta.status())
        check("beta: status renders all placement", '"side": "all"' in beta_status, beta_status)

        text = f"all-text-{TAG}"
        alpha.copy(text=text)
        check("text clipboard synchronizes in all mode", bool(poll(3, lambda: beta.paste().decode(errors="replace") == text)))
        png = bytes.fromhex("89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de0000000c4944415478da63a8b79f05000299015926bfe5800000000049454e44ae426082")
        beta.copy(mime="image/png", data=png)
        check("PNG clipboard synchronizes byte-identically in all mode", bool(poll(3, lambda: alpha.paste("image/png") == png)))

        for source, target in ((alpha, beta), (beta, alpha)):
            for side in ("left", "right", "top", "bottom"):
                fraction = 0.28 if side in ("left", "right") else 0.72
                target_side = {"left": "right", "right": "left", "top": "bottom", "bottom": "top"}[side]
                check(f"crossing {source.name} {side} transfers control", cross(source, target, side, fraction), f"source={source.state()} target={target.state()}")
                x, y = target.cursor()
                target_monitor = target.monitor()
                expected = target_monitor["height"] * fraction if side in ("left", "right") else target_monitor["width"] * fraction
                actual = y if side in ("left", "right") else x
                check(f"{source.name} {side} arrival uses reciprocal edge and proportional coordinate", target_side in edge_at(target) and abs(actual - expected) <= 4, f"cursor={(x, y)} target={target_side} expected={expected}")
                time.sleep(0.35)
                for forbidden in (edge for edge in ("left", "right", "top", "bottom") if edge != target_side):
                    wrong_x, wrong_y = point_on_edge(target, forbidden)
                    target.inject(f"abs:{target_monitor['width'] // 2},{target_monitor['height'] // 2}", f"abs:{wrong_x},{wrong_y}")
                    time.sleep(0.1)
                    check(f"{target.name} rejects {forbidden} return after {source.name} {side}", source.state() == "controlling" and target.state() == "controlled")
                check(f"return through {target.name} {target_side} restores idle", return_control(source, target, target_side), f"source={source.state()} target={target.state()}")

        check("right crossing starts a controlled session", cross(alpha, beta, "right"))
        return_drag_path = STATE / "return-drag.bin"
        return_drag_content = os.urandom(1024) + f"return-drag-{TAG}".encode()
        return_drag_path.write_bytes(return_drag_content)
        return_drag = subprocess.Popen([DRAG_SOURCE, str(return_drag_path)], env=beta.env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        return_target = subprocess.Popen([DROP_TARGET], env=alpha.env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        alpha_gesture = alpha.persistent_injector()
        try:
            check("controlled beta drag source is ready", bool(poll(4, lambda: return_drag.poll() is None and line_seen(return_drag, "drag_source ready"))))
            check("alpha drop target is ready for the return drag", bool(poll(4, lambda: return_target.poll() is None and line_seen(return_target, "drop_target ready"))))
            beta_x, beta_y = beta.cursor()
            persistent_step(alpha_gesture, f"motion:{100 - beta_x},{100 - beta_y}", f"button:{BTN_LEFT}:down")
            check("same alpha pointer starts the beta file drag", bool(poll(3, lambda: line_seen(return_drag, "drag started"))))
            beta_x, beta_y = beta.cursor()
            persistent_step(alpha_gesture, f"motion:0,{-beta_y}")
            time.sleep(0.7)
            check("a controlled beta drag at non-entry top stays active", alpha.state() == "controlling" and beta.state() == "controlled" and return_drag.poll() is None, f"alpha={alpha.state()} beta={beta.state()} drag_exit={return_drag.poll()}")
            beta_x, beta_y = beta.cursor()
            persistent_step(alpha_gesture, f"motion:{-beta_x},{beta.monitor()['height'] / 2 - beta_y}")
            check("a controlled beta drag at recorded left entry returns control", bool(poll(3, lambda: alpha.state() == "idle" and beta.state() == "idle")), f"alpha={alpha.state()} beta={beta.state()}")
            returned_file = poll(5, lambda: received_file(alpha.dnd_dir, return_drag_path.name))
            check("a controlled beta drag through its entry transfers byte-identically back to alpha", bool(returned_file) and returned_file.read_bytes() == return_drag_content, f"path={returned_file}")
            alpha_monitor = alpha.monitor()
            persistent_step(alpha_gesture, f"abs:{alpha_monitor['width'] - 100},{alpha_monitor['height'] - 100}", f"button:{BTN_LEFT}:up")
            check("alpha drop target accepts the returned file and its virtual button is released", bool(poll(5, lambda: return_target.poll() == 0)), f"exit={return_target.poll()}")
            alpha_gesture.stdin.write("quit\n")
            alpha_gesture.stdin.flush()
            alpha_gesture.wait(timeout=5)
        finally:
            if alpha_gesture.poll() is None:
                alpha_gesture.terminate()
            if return_drag.poll() is None:
                return_drag.terminate()
                return_drag.wait(timeout=5)
            if return_target.poll() is None:
                return_target.terminate()
                return_target.wait(timeout=5)

        alpha_monitor = alpha.monitor()
        alpha.inject(f"abs:{alpha_monitor['width'] - 1},0", "motion:0,-70")
        corner_active = poll(3, lambda: alpha.state() == "controlling" and beta.state() == "controlled")
        recorded = edge_at(beta)
        check("a top-right corner starts one exclusive controlled session", bool(corner_active) and {"right", "bottom"}.issubset(recorded), f"alpha={alpha.state()} beta={beta.state()} beta_edges={recorded}")
        time.sleep(0.35)
        beta.inject(f"abs:{beta.monitor()['width'] // 2},{beta.monitor()['height'] // 2}")
        move_to_edge(beta, "right")
        right_returns = poll(0.8, lambda: alpha.state() == "idle" and beta.state() == "idle")
        if right_returns:
            alpha.inject(f"abs:{alpha_monitor['width'] - 1},0", "motion:0,-70")
            second_corner = poll(3, lambda: alpha.state() == "controlling" and beta.state() == "controlled")
            time.sleep(0.35)
            beta.inject(f"abs:{beta.monitor()['width'] // 2},{beta.monitor()['height'] // 2}")
            move_to_edge(beta, "bottom")
            bottom_rejected = alpha.state() == "controlling" and beta.state() == "controlled"
            corner_returned = second_corner and bottom_rejected and return_control(alpha, beta, "right")
            detail = f"owner=right bottom_rejected={bottom_rejected}"
        else:
            bottom_rejected = alpha.state() == "controlling" and beta.state() == "controlled"
            corner_returned = bottom_rejected and return_control(alpha, beta, "bottom")
            detail = f"owner=bottom right_rejected={bottom_rejected}"
        check("corner records one return edge and rejects its adjacent edge", corner_returned, f"{detail} alpha={alpha.state()} beta={beta.state()}")

        payload = STATE / "all-drag.bin"
        content = os.urandom(4096) + f"all-drag-{TAG}".encode()
        payload.write_bytes(content)
        drag = subprocess.Popen([DRAG_SOURCE, str(payload)], env=alpha.env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        drop_target = subprocess.Popen([DROP_TARGET], env=beta.env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        forward_gesture = alpha.persistent_injector()
        try:
            time.sleep(0.4)
            check("beta drop target is ready for the forward drag", bool(poll(4, lambda: drop_target.poll() is None and line_seen(drop_target, "drop_target ready"))))
            persistent_step(forward_gesture, "abs:100,100", f"button:{BTN_LEFT}:down")
            check("same alpha pointer starts the forward file drag", bool(poll(3, lambda: line_seen(drag, "drag started"))))
            edge_x, edge_y = point_on_edge(alpha, "bottom")
            persistent_step(forward_gesture, f"abs:{edge_x},{edge_y}")
            check("file drag crosses an all-mode bottom edge", bool(poll(5, lambda: alpha.state() == "controlling" and beta.state() == "controlled")), f"alpha={alpha.state()} beta={beta.state()}")
            received = poll(5, lambda: received_file(beta.dnd_dir, payload.name))
            check("all-mode file drag is received byte-identically", bool(received) and received.read_bytes() == content, f"path={received}")
            beta_monitor = beta.monitor()
            beta_x, beta_y = beta.cursor()
            persistent_step(forward_gesture, f"motion:{beta_monitor['width'] - 100 - beta_x},{beta_monitor['height'] - 100 - beta_y}")
            beta_at_target = poll(2, lambda: beta.cursor()[0] >= beta_monitor["width"] - 210 and beta.cursor()[1] >= beta_monitor["height"] - 210)
            check("forwarded motion reaches the beta drop target", bool(beta_at_target), f"cursor={beta.cursor()}")
            persistent_step(forward_gesture, f"button:{BTN_LEFT}:up")
            check("beta drop target accepts the forwarded file and its virtual button is released", bool(poll(5, lambda: drop_target.poll() == 0)), f"exit={drop_target.poll()}")
            marker_alpha, marker_beta = STATE / "key-alpha", STATE / "key-beta"
            for machine, marker in ((alpha, marker_alpha), (beta, marker_beta)):
                machine.hyprctl("eval", f'hl.bind("SUPER + F12", hl.dsp.exec_cmd("touch {marker}"))')
            persistent_step(forward_gesture, "key:125:down", "mods:64", "key:88:down", "key:88:up", "key:125:up", "mods:0")
            check("keyboard shortcuts still reach beta after file drop", bool(poll(3, marker_beta.exists)))
            check("post-drop remote shortcut does not execute locally", not marker_alpha.exists())
            check("emergency release recovers from all-mode drag", release(alpha), f"alpha={alpha.state()} beta={beta.state()}")
        finally:
            if forward_gesture.poll() is None:
                forward_gesture.terminate()
                forward_gesture.wait(timeout=5)
            if drag.poll() is None:
                drag.terminate()
                drag.wait(timeout=5)
            if drop_target.poll() is None:
                drop_target.terminate()
                drop_target.wait(timeout=5)
        emergency_source = subprocess.Popen([DRAG_SOURCE, str(payload)], env=alpha.env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        emergency_pointer = alpha.persistent_injector()
        try:
            ready = poll(3, lambda: line_seen(emergency_source, "drag_source ready"))
            check("emergency-during-drag source is ready", bool(ready))
            persistent_step(emergency_pointer, "abs:100,100", f"button:{BTN_LEFT}:down")
            check("emergency scenario starts a held drag", bool(poll(3, lambda: line_seen(emergency_source, "drag started"))))
            edge_x, edge_y = point_on_edge(alpha, "bottom")
            persistent_step(emergency_pointer, f"abs:{edge_x},{edge_y}")
            check("emergency scenario enters remote control with button held", bool(poll(5, lambda: alpha.state() == "controlling" and beta.state() == "controlled")))
            check("CtrlAltEsc releases both machines during held file drag", release(alpha) and bool(poll(3, lambda: beta.state() == "idle")))
            persistent_step(emergency_pointer, f"button:{BTN_LEFT}:up")
        finally:
            for process in (emergency_pointer, emergency_source):
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
    finally:
        alpha.stop()
        beta.stop()
    failed = [name for name, passed in RESULTS if not passed]
    print(f"\n{len(RESULTS) - len(failed)}/{len(RESULTS)} checks passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(run())
