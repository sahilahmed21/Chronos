import os
import socket
import sys
import time

import pytest

from chronos_agent import sandbox

pytestmark = pytest.mark.skipif(not sandbox.available(), reason="needs Linux with working bubblewrap")


def sh(ws, script, **kw):
    return sandbox.run(["/bin/sh", "-c", script], ws, **kw)


def test_positive_control_command_can_use_its_workspace(tmp_path):
    r = sh(tmp_path, "echo hi > out.txt && cat out.txt")
    assert (r.returncode, r.stdout.strip()) == (0, "hi") and (tmp_path / "out.txt").read_text().strip() == "hi"


def test_writes_outside_the_workspace_fail(tmp_path):
    for target in ("/usr/x", "/etc/x", "/x"):
        assert sh(tmp_path, f"touch {target}").returncode != 0, target


def test_host_files_outside_the_workspace_are_invisible(tmp_path_factory, tmp_path):
    canary_dir = tmp_path_factory.mktemp("host")
    canary = canary_dir / "canary.txt"
    canary.write_text("secret")
    r = sh(tmp_path, f"cat {canary}")
    assert r.returncode != 0 and "secret" not in r.stdout
    assert sh(tmp_path, f"test -e {os.path.expanduser('~')}/.ssh").returncode != 0


def test_host_environment_secrets_are_not_inherited(tmp_path, monkeypatch):
    monkeypatch.setenv("CANARY_SECRET", "hunter2")
    r = sh(tmp_path, "printenv CANARY_SECRET; echo rc=$?")
    assert "hunter2" not in r.stdout


def test_network_egress_is_blocked_even_to_a_host_loopback_listener(tmp_path):
    srv = socket.socket()
    srv.bind(("127.0.0.1", 0))
    srv.listen(1)
    port = srv.getsockname()[1]
    code = f"import socket; socket.create_connection(('127.0.0.1', {port}), timeout=2)"
    r = sandbox.run(["/usr/bin/python3", "-c", code], tmp_path)
    srv.settimeout(0.3)
    try:
        srv.accept()
        accepted = True
    except socket.timeout:
        accepted = False
    srv.close()
    assert r.returncode != 0 and not accepted


def test_memory_limit_stops_a_large_allocation(tmp_path):
    r = sandbox.run(["/usr/bin/python3", "-c", "x = bytearray(1024*1024*1024)"], tmp_path, mem_mb=256)
    assert r.returncode != 0 and ("MemoryError" in r.stderr or r.returncode < 0)


def test_wall_clock_timeout_kills_runaway_commands(tmp_path):
    t0 = time.monotonic()
    r = sh(tmp_path, "sleep 30", timeout_s=1)
    assert r.timed_out and time.monotonic() - t0 < 10


def test_output_is_capped_not_buffered_without_bound(tmp_path):
    r = sh(tmp_path, "yes aaaaaaaa", timeout_s=2, max_out=4096)
    assert r.truncated and len(r.stdout) <= 4096
