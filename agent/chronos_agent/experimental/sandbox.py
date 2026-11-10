"""Run an untrusted command inside bubblewrap with bounded resources (Linux only).

Boundary: new user/pid/ipc/uts/net namespaces, empty environment, read-only /usr, a private
/tmp, and only `workspace` writable (mounted at /work). Limits: address space, file size,
wall clock, captured output. This is process-level isolation on a shared kernel, not a VM:
a kernel or bubblewrap escape defeats it (see SandboxEscapeBench in NEXT_PHASES.md).
"""
import os
import shutil
import subprocess
import tempfile
from dataclasses import dataclass


@dataclass
class SandboxResult:
    returncode: int
    stdout: str
    stderr: str
    timed_out: bool
    truncated: bool


def _base_args(workspace):
    return ["bwrap", "--unshare-all", "--die-with-parent", "--clearenv",
            "--setenv", "PATH", "/usr/bin:/bin", "--setenv", "HOME", "/work",
            "--ro-bind", "/usr", "/usr", "--symlink", "usr/bin", "/bin",
            "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64",
            "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
            "--bind", str(workspace), "/work", "--remount-ro", "/", "--chdir", "/work"]


def available() -> bool:
    if os.name != "posix" or shutil.which("bwrap") is None:
        return False
    try:
        r = subprocess.run(_base_args("/tmp") + ["/usr/bin/true"], capture_output=True, timeout=10)
    except (OSError, subprocess.SubprocessError):
        return False
    return r.returncode == 0


def run(cmd, workspace, timeout_s=5, mem_mb=256, max_out=65536) -> SandboxResult:
    import resource  # POSIX only

    def limits():
        resource.setrlimit(resource.RLIMIT_AS, (mem_mb << 20, mem_mb << 20))
        resource.setrlimit(resource.RLIMIT_FSIZE, (max_out * 4, max_out * 4))
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        proc = subprocess.Popen(_base_args(workspace) + list(cmd), stdout=out, stderr=err,
                                stdin=subprocess.DEVNULL, preexec_fn=limits, start_new_session=True)
        timed_out = False
        try:
            proc.wait(timeout=timeout_s)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                os.killpg(proc.pid, 9)
            except ProcessLookupError:
                pass
            proc.wait()
        out.seek(0)
        err.seek(0)
        raw = out.read(max_out + 1)
        return SandboxResult(proc.returncode, raw[:max_out].decode(errors="replace"),
                             err.read(max_out).decode(errors="replace"), timed_out,
                             len(raw) > max_out or out.seek(0, 2) > max_out)
