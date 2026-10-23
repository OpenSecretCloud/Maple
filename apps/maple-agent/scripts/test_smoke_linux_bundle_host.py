#!/usr/bin/env python3
"""Process-level fixtures for the isolated Linux bundle AppRun host smoke."""

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


SMOKE = Path(__file__).with_name("smoke-linux-bundle-host.sh")


FIXTURE = r'''import fcntl, hashlib, json, os, pathlib, socket, subprocess, sys, threading, time
record = pathlib.Path(RECORD)
behavior = BEHAVIOR
mode = sys.argv[1:]
if mode == ["--build-info"]:
    if behavior == "slow-identity-timeout":
        time.sleep(2)
    record.write_text(json.dumps({"environment": dict(os.environ), "scratch": os.getcwd()}))
    print(json.dumps({"data_namespace": "maple-agent-dev"}))
    sys.exit(0)
namespace = "maple-agent-dev"
store = pathlib.Path(os.environ["XDG_DATA_HOME"]) / namespace / "store-v1"
storage = [str(store.resolve()), namespace, str(os.geteuid())]
identity = hashlib.sha256(json.dumps(storage, separators=(",", ":")).encode()).hexdigest()[:24]
endpoint = pathlib.Path(f"/tmp/maple-agent-{os.geteuid()}") / identity
address = endpoint / "ipc.sock"
if mode == ["host", "serve"]:
    store.mkdir(mode=0o700, parents=True, exist_ok=True)
    endpoint.parent.mkdir(mode=0o700, exist_ok=True)
    endpoint.mkdir(mode=0o700, exist_ok=True)
    with (store / "owner.lock").open("a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(address))
        server.listen()
        record.with_suffix(".host").write_text(str(os.getpid()))
        record.with_suffix(".endpoint").write_text(str(endpoint))
        while True:
            connection, _ = server.accept()
            message = connection.recv(16)
            if message == b"stop":
                connection.close()
                if behavior == "refuse-stop":
                    continue
                break
            def drain(connection):
                while connection.recv(1024):
                    pass
                connection.close()
            threading.Thread(target=drain, args=(connection,), daemon=True).start()
        server.close()
        address.unlink()
    record.with_suffix(".stopped").write_text("stopped")
elif mode == ["host", "stop"]:
    connection = socket.socket(socket.AF_UNIX)
    try:
        connection.connect(str(address))
    except FileNotFoundError:
        sys.exit(1)
    connection.sendall(b"stop")
    connection.close()
    deadline = time.monotonic() + 5
    while not record.with_suffix(".stopped").exists() and time.monotonic() < deadline:
        time.sleep(0.01)
elif mode == ["acp"]:
    if behavior in ("timeout", "slow-identity-timeout"):
        time.sleep(30)
    if not address.exists():
        subprocess.Popen([sys.executable, __file__, "host", "serve"],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)
    deadline = time.monotonic() + 5
    while not address.exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    connection = socket.socket(socket.AF_UNIX)
    connection.connect(str(address))
    connection.sendall(b"acp")
    with record.with_suffix(".clients").open("a") as clients:
        clients.write("client\n")
    connection.close()
    if behavior == "reject":
        print("private-fixture-value", file=sys.stderr, flush=True)
    elif behavior == "diagnostics":
        log = store / "logs/maple-agent.log"
        log.parent.mkdir()
        log.write_text("fixture-log-start\n" + "fixture-log-line\n" * 2000 + "fixture-log-end\n")
        print("\x1b[31m" + "fixture-error-line\n" * 2000 + "\x1b[0m", file=sys.stderr, flush=True)
    elif behavior == "store-setup":
        print("Maple needs local store setup. Open Maple to continue.",
              file=sys.stderr, flush=True)
    else:
        print("Maple ACP requires sign-in. Sign in through Maple Desktop or `maple-agent login` first.",
              file=sys.stderr, flush=True)
    sys.exit(1)
else:
    sys.exit(2)
'''


class LinuxBundleHostSmokeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.fixture = self.root / "AppRun"
        self.record = self.root / "process.json"
        self.tools = self.root / "tools"
        self.tools.mkdir()
        # macOS fixture runs use Python's flock with the Linux CLI contract;
        # the network-disabled Ubuntu verifier has util-linux flock already.
        flock = self.tools / "flock"
        flock.write_text(f'''#!{sys.executable}
import fcntl, os, sys
with open(sys.argv[2], "a+") as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit(1)
''')
        flock.chmod(0o755)

    def run_smoke(self, behavior="success", environment=None, timeout="10"):
        self.fixture.write_text(
            f"#!{sys.executable}\nRECORD = {str(self.record)!r}\nBEHAVIOR = {behavior!r}\n" + FIXTURE
        )
        self.fixture.chmod(0o755)
        parent = dict(os.environ) if environment is None else environment.copy()
        parent.update(PATH=f"{self.tools}{os.pathsep}{os.environ['PATH']}", MAPLE_AGENT_SMOKE_TIMEOUT=timeout)
        return subprocess.run(["/bin/sh", str(SMOKE), str(self.fixture)], env=parent,
                              capture_output=True, text=True, timeout=20, check=False)

    def assert_cleaned(self, host_started=True):
        state = json.loads(self.record.read_text())
        self.assertFalse(Path(state["scratch"]).exists())
        if host_started:
            self.assertTrue(self.record.with_suffix(".stopped").exists())
        return state

    def test_sign_in_guard_twice_reuses_host_stops_and_isolates_credentials(self):
        parent = dict(os.environ, APPIMAGE="/unrelated.AppImage", APPDIR="/unrelated",
                      APPIMAGE_EXTRACT_AND_RUN="1", NO_CLEANUP="1", HOME="/existing-home",
                      GITHUB_TOKEN="fixture-secret", BWS_ACCESS_TOKEN="fixture-secret",
                      MAPLE_API_KEY="fixture-secret", LD_PRELOAD="fixture-injection")
        result = self.run_smoke(environment=parent)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("releases ownership", result.stdout)
        self.assertIn("signed-out ACP guard", result.stdout)
        self.assertEqual(self.record.with_suffix(".clients").read_text().splitlines(), ["client", "client"])
        environment = self.assert_cleaned()["environment"]
        for key in ("APPIMAGE", "APPDIR", "APPIMAGE_EXTRACT_AND_RUN", "NO_CLEANUP",
                    "GITHUB_TOKEN", "BWS_ACCESS_TOKEN", "MAPLE_API_KEY", "LD_PRELOAD"):
            self.assertNotIn(key, environment)
        for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR"):
            self.assertTrue(Path(environment[key]).is_relative_to(environment["HOME"].rsplit("/", 1)[0]))

    def test_store_setup_guard_also_requires_the_same_host_and_clean_shutdown(self):
        result = self.run_smoke("store-setup")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.record.with_suffix(".clients").read_text().splitlines(), ["client", "client"])
        self.assert_cleaned()

    def test_unexpected_guard_stops_only_the_fixture_host_and_hides_output(self):
        result = self.run_smoke("reject")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("did not return an expected signed-out guard", result.stderr)
        self.assertNotIn("private-fixture-value", result.stderr)
        self.assert_cleaned()

    def test_opt_in_diagnostics_are_bounded_and_remove_terminal_controls(self):
        parent = dict(os.environ, MAPLE_AGENT_SMOKE_DIAGNOSTICS="1", GITHUB_TOKEN="fixture-secret")
        result = self.run_smoke("diagnostics", environment=parent)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ACP exit status: 1", result.stderr)
        self.assertIn("fixture-log-end", result.stderr)
        self.assertNotIn("fixture-log-start", result.stderr)
        self.assertNotIn("\x1b", result.stderr)
        self.assertNotIn("fixture-secret", result.stderr)
        self.assertLess(len(result.stderr), 17000)
        self.assert_cleaned()

    def test_client_timeout_is_bounded_and_does_not_stop_unrelated_process(self):
        unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        try:
            result = self.run_smoke("timeout", timeout="1")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("signed-out ACP probe timed out", result.stderr)
            self.assertIsNone(unrelated.poll())
            self.assert_cleaned(host_started=False)
        finally:
            unrelated.terminate()
            unrelated.wait(timeout=5)

    def test_cold_identity_discovery_does_not_use_the_short_client_timeout(self):
        result = self.run_smoke("slow-identity-timeout", timeout="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("signed-out ACP probe timed out", result.stderr)
        self.assert_cleaned(host_started=False)

    def test_failed_host_stop_retains_state_and_cannot_claim_success(self):
        try:
            result = self.run_smoke("refuse-stop", timeout="2")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("probe state was retained", result.stderr)
            self.assertNotIn("Verified", result.stdout)
            state = json.loads(self.record.read_text())
            self.assertTrue(Path(state["scratch"]).exists())
        finally:
            # This fixture deliberately refuses its normal shutdown command.
            # Stop only its recorded PID, then remove its retained test paths.
            host_record = self.record.with_suffix(".host")
            if host_record.exists():
                os.kill(int(host_record.read_text()), signal.SIGTERM)
                endpoint = Path(self.record.with_suffix(".endpoint").read_text())
                import fcntl
                state = json.loads(self.record.read_text())
                owner = Path(state["environment"]["XDG_DATA_HOME"]) / "maple-agent-dev/store-v1/owner.lock"
                with owner.open("r+") as lock:
                    deadline = time.monotonic() + 5
                    while True:
                        try:
                            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            break
                        except BlockingIOError:
                            if time.monotonic() >= deadline:
                                raise
                            time.sleep(0.01)
                shutil.rmtree(endpoint)
            if self.record.exists():
                state = json.loads(self.record.read_text())
                shutil.rmtree(state["scratch"], ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
