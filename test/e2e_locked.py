#!/usr/bin/env python3
"""Real session-lock/socket regression in nested Hyprland, never the host session.

SIGUSR1 unlocks only the hyprlock child created in a nested compositor. Password
entry remains a separate physical acceptance test. Run after e2e_all_edges.py,
not concurrently: both reuse its isolated alpha/beta fixture.
"""
import json
import shutil
import signal
import subprocess
import time
import e2e_all_edges as base

OPPOSITE = dict(left="right", right="left", top="bottom", bottom="top")
MOTION = dict(left=(-40, 0), right=(40, 0), top=(0, -40), bottom=(0, 40))


def locked(machine):
    return json.loads(machine.hyprctl("-j", "locked"))["locked"]


def lock(machine):
    assert machine.env["HYPRLAND_INSTANCE_SIGNATURE"] != base.os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
    config = machine.dir / "hyprlock.conf"
    config.write_text("general {\n hide_cursor = false\n}\nbackground {\n color = rgb(202020)\n}\n")
    log = open(machine.dir / "hyprlock.log", "w")
    process = subprocess.Popen(["hyprlock", "--config", str(config), "--grace", "0", "--immediate-render"], env=machine.env, stdout=log, stderr=subprocess.STDOUT)
    log.close()
    assert base.poll(5, lambda: locked(machine)), (machine.dir / "hyprlock.log").read_text()
    time.sleep(0.6)  # idle lock polling is 500 ms
    return process


def unlock(machine, process):
    process.send_signal(signal.SIGUSR1)
    process.wait(timeout=5)
    assert base.poll(3, lambda: not locked(machine))
    time.sleep(0.6)


def returned(source, target):
    # StopGrab may expose the local edge strip again, arming normal local push
    # resistance. Pushing has no remote keyboard/mouse grab and is not a handoff.
    return source.state() in ("idle", "pushing") and target.state() == "idle"


def main():
    shutil.rmtree(base.STATE, ignore_errors=True)
    alpha, beta = base.Machine("alpha"), base.Machine("beta")
    lockers = []
    injectors = []
    try:
        for machine in (alpha, beta):
            machine.start_compositor()
        # Opening beta can resize alpha's host window; start daemons only once
        # both nested outputs exist, as in the ordinary four-edge fixture.
        time.sleep(0.5)
        for machine in (alpha, beta):
            machine.inject("abs:500,350")
            assert machine.start_daemon()
        assert base.poll(15, lambda: any(p["name"] == beta.service_name for p in alpha.ipc("Discover")["Discovered"]))
        assert alpha.ipc({"Pair": {"name": beta.service_name}}) == "PinRequired"
        pin = base.poll(3, lambda: beta.status()["pending_pin"])
        assert alpha.ipc({"SubmitPin": {"pin": pin}}) == "Ok"
        for machine, peer in ((alpha, beta), (beta, alpha)):
            assert machine.cli("peer", "set", peer.service_name, "--side", "all")[0] == 0
        assert base.poll(15, lambda: all(p["connected"] for p in alpha.status()["peers"]))
        time.sleep(1)
        # Keep each virtual pointer alive. Creating/destroying a pointer for each
        # motion includes startup/shutdown latency and changes compositor focus.
        for machine in (alpha, beta):
            injector = machine.persistent_injector()
            injectors.append(injector)
            machine.inject = lambda *steps, injector=injector: base.persistent_step(injector, *steps)
        for source, target in ((alpha, beta), (beta, alpha)):
            locker = lock(target)
            lockers.append(locker)
            for side, entry in OPPOSITE.items():
                source.inject("abs:500,350")
                assert base.cross(source, target, side)
                time.sleep(0.35)
                for forbidden in (edge for edge in OPPOSITE if edge != entry):
                    x, y = base.point_on_edge(target, forbidden)
                    target.inject(f"abs:{x},{y}")
                    dx, dy = MOTION[forbidden]
                    source.inject(f"motion:{dx},{dy}")
                    time.sleep(0.12)
                    base.check(f"{source.name} {side}: locked {forbidden} cannot return", target.state() == "controlled")
                # Direct positioning alone must not return; the next source motion does.
                x, y = base.point_on_edge(target, entry)
                target.inject(f"abs:{x},{y}")
                time.sleep(0.25)
                base.check(f"{source.name} {side}: stationary entry does not return", target.state() == "controlled")
                dx, dy = MOTION[entry]
                start = time.monotonic()
                source.inject(f"motion:{dx},{dy}")
                while time.monotonic() - start < 0.25 and not returned(source, target):
                    time.sleep(0.005)
                elapsed = (time.monotonic() - start) * 1000
                base.check(f"{source.name} {side}: locked return within 250ms", returned(source, target) and elapsed <= 250, f"{elapsed:.1f}ms source={source.state()} target={target.state()}")
                base.check(f"{target.name} remains locked after return", locked(target))
                inward_x, inward_y = MOTION[side]
                source.inject(f"motion:{-inward_x},{-inward_y}")
                base.check("local inward movement clears any edge resistance", bool(base.poll(1, lambda: source.state() == "idle")))
                time.sleep(0.85)
            source.inject("abs:500,350")
            assert base.cross(source, target, "right")
            unlock(target, locker)
            lockers.remove(locker)
            base.check("unlock preserves active control", source.state() == "controlling" and target.state() == "controlled")
            base.check("surface return resumes after unlock", base.return_control(source, target, "left"))
            source.inject("abs:500,350")
            assert base.cross(source, target, "right")
            locker = lock(target)
            lockers.append(locker)
            base.check("locking destination during control preserves session", target.state() == "controlled")
            time.sleep(0.35)
            source.inject("motion:-4000,0")
            base.check("destination locked mid-session can return", bool(base.poll(1, lambda: returned(source, target))))
            unlock(target, locker)
            lockers.remove(locker)
            source.inject("abs:500,350")
            assert base.cross(source, target, "right")
            locker = lock(source)
            lockers.append(locker)
            base.check("locking captured source releases both machines", source.state() == "idle" and target.state() == "idle")
            unlock(source, locker)
            lockers.remove(locker)
    finally:
        for injector in injectors:
            if injector.poll() is None:
                injector.terminate()
                injector.wait(timeout=5)
        for locker in lockers:
            if locker.poll() is None:
                locker.send_signal(signal.SIGUSR1)
                locker.wait(timeout=5)
        alpha.stop()
        beta.stop()
    passed = sum(passed for _, passed in base.RESULTS)
    print(f"{passed}/{len(base.RESULTS)} checks passed")
    return int(passed != len(base.RESULTS))


if __name__ == "__main__":
    raise SystemExit(main())
