#!/usr/bin/env python3
"""Mortiflix FilmCraft: a fresh install from the Linux tarball launches to first-run setup.

Extracts the release tarball, installs it with its install.sh into a temporary prefix, then starts
the installed app as a brand-new user (temporary HOME and XDG folders, no MFX_* variables) and
drives it over the control channel:

  1. the app finds the Mortiflix bundle beside itself (lib/mortiflix-filmcraft) and the bundled
     service connects (its own Node; nothing from this machine's PATH or settings),
  2. the first-run setup window is on screen (opened by the app itself on a first launch),
  3. screenshots of both go to --shots.

Needs a display: run it under `xvfb-run -a -s "-screen 0 1920x1080x24"` on a headless machine.
Never installs anything outside the temporary folder, and stops only the process it started.

Usage: test_install.py TARBALL [--port 19810] [--shots DIR] [--keep] [--setup-optional]
  --setup-optional  pass when the setup window does not open by itself yet (it is then opened with
                    the mortiflix.setup command for the screenshot, and reported)
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tarfile
import tempfile
import time


def control(port, timeout=60.0):
    """Connect to the app's control channel, retrying while it starts."""
    deadline = time.monotonic() + timeout
    while True:
        try:
            sock = socket.create_connection(("127.0.0.1", port), timeout=5)
            sock.settimeout(60)
            return sock, sock.makefile("r", encoding="utf-8")
        except OSError:
            if time.monotonic() > deadline:
                raise
            time.sleep(0.25)


class App:
    def __init__(self, port):
        self.sock, self.lines = control(port)
        self.next_id = 0

    def call(self, method, params=None):
        self.next_id += 1
        req = {"id": self.next_id, "method": method, "params": params or {}}
        self.sock.sendall((json.dumps(req) + "\n").encode())
        while True:
            line = self.lines.readline()
            if not line:
                raise RuntimeError(f"control channel closed during {method}")
            msg = json.loads(line)
            if msg.get("id") == self.next_id:
                if not msg.get("ok"):
                    raise RuntimeError(f"{method}: {msg.get('error')}")
                return msg.get("result")

    def command(self, command, params=None):
        return self.call("engine.execute", {"command": command, "params": params or {}})

    def element_ids(self, prefix=""):
        res = self.call("ui.elements", {"prefix": prefix})
        items = res.get("elements", res) if isinstance(res, dict) else res
        return [e.get("id", "") for e in items or [] if isinstance(e, dict)]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("tarball", type=Path)
    ap.add_argument("--port", type=int, default=19810)
    ap.add_argument("--shots", type=Path, default=None)
    ap.add_argument("--keep", action="store_true", help="keep the temporary install for a look")
    ap.add_argument("--setup-optional", action="store_true")
    args = ap.parse_args()

    tmp = Path(tempfile.mkdtemp(prefix="mortiflix-install-"))
    shots = (args.shots or tmp / "shots").resolve()
    shots.mkdir(parents=True, exist_ok=True)
    proc = None
    failures = []
    try:
        # ---- install ------------------------------------------------------------------------------
        with tarfile.open(args.tarball) as tar:
            tar.extractall(tmp / "extract", filter="data")
        (top,) = [p for p in (tmp / "extract").iterdir() if p.is_dir()]
        prefix = tmp / "prefix"
        home = tmp / "home"
        home.mkdir()
        env = {k: v for k, v in os.environ.items() if not k.startswith(("MFX_", "MORTIFLIX_", "FILMCRAFT_", "XDG_"))}
        env.update(HOME=str(home), XDG_DATA_HOME=str(home / ".local/share"), XDG_CONFIG_HOME=str(home / ".config"),
                   XDG_CACHE_HOME=str(home / ".cache"), XDG_STATE_HOME=str(home / ".local/state"))
        if "XDG_RUNTIME_DIR" in os.environ:
            env["XDG_RUNTIME_DIR"] = os.environ["XDG_RUNTIME_DIR"]
        env.pop("WAYLAND_DISPLAY", None)  # X (Xvfb) only: never a window on the user's screen
        out = subprocess.run([str(top / "install.sh"), "--prefix", str(prefix)], env=env, check=True,
                             capture_output=True, text=True).stdout
        print(out.strip())
        bundle = prefix / "lib/mortiflix-filmcraft"
        for path in ("bin/filmcraft", "bin/filmcraft-cli", "lib/mortiflix-filmcraft/service/bin/mortiflix-service.mjs",
                     "lib/mortiflix-filmcraft/node/bin/node", "lib/mortiflix-filmcraft/bin/ffmpeg",
                     "share/applications/ai.storyteller.filmcraft.desktop"):
            if not (prefix / path).exists():
                failures.append(f"missing after install: {path}")
        print("installed:", ", ".join(sorted(p.name for p in bundle.iterdir())) if bundle.exists() else "no bundle")
        print(subprocess.run([str(prefix / "bin/filmcraft-cli"), "--version"], env=env, capture_output=True,
                             text=True).stdout.strip())

        # ---- first launch -------------------------------------------------------------------------
        log = open(tmp / "app.log", "w")
        proc = subprocess.Popen([str(prefix / "bin/filmcraft"), "--control", str(args.port), "--no-recover"],
                                env=env, stdout=log, stderr=subprocess.STDOUT, cwd=home)
        print(f"started the installed app, pid {proc.pid}")
        app = App(args.port)
        app.call("ui.resize", {"width": 1920, "height": 1080})

        status = None
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            st = app.command("mortiflix.state")
            status = st.get("service", {})
            if status.get("state") == "connected":
                break
            time.sleep(0.5)
        print("service:", json.dumps(status))
        if status.get("state") != "connected":
            failures.append(f"the bundled service did not connect: {status}")

        time.sleep(1.5)  # a few frames, so the first-run window is drawn
        st = app.command("mortiflix.state")
        ids = app.element_ids("mfx.setup")
        shown = bool(st.get("state", {}).get("showSetup", st.get("state", {}).get("show_setup"))) or any(
            i.startswith("mfx.setup") for i in ids)
        app.call("ui.screenshot", {"path": str(shots / "first-launch.png")})
        print("first-run setup shown by itself:", shown)
        if not shown:
            if args.setup_optional:
                app.command("mortiflix.setup")
                time.sleep(1.0)
                app.call("ui.screenshot", {"path": str(shots / "setup-opened.png")})
                print("setup window opened with mortiflix.setup (--setup-optional)")
            else:
                failures.append("the first-run setup window did not open on a fresh install")

        data = home / ".local/share"
        made = sorted(str(p.relative_to(home)) for p in data.rglob("*") if p.is_file())[:20] if data.exists() else []
        print("data written by the first launch:", made)
        print("screenshots:", shots)
    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        if args.keep:
            print("kept:", tmp)
        else:
            shutil.rmtree(tmp, ignore_errors=True)

    if failures:
        print("FAIL:\n  " + "\n  ".join(failures))
        return 1
    print("PASS: a fresh install launches with the bundled service connected")
    return 0


if __name__ == "__main__":
    sys.exit(main())
