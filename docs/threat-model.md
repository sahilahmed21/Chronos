# Threat model: sandboxed command execution (`agent/chronos_agent/experimental/sandbox.py`)

> **EXPERIMENTAL.** This is an extra, not part of Chronos' headline (deterministic simulation testing with measured oracle detection power). It is not claimed anywhere in the README. It is kept because its tests found a real gap (a writable root filesystem, fixed with `--remount-ro /`).

Scope: an agent's "run this command" tool. The command is untrusted (it may be written by a model that read prompt-injected text). Not in scope: the model API, the agent's own process, tools other than command execution.

## Boundary

`bwrap --unshare-all --die-with-parent --clearenv`, read-only `/usr`, read-only root, private `/tmp`, one writable bind mount (`/work`). Limits: address space (default 256 MB), output file size, wall clock (default 5 s), captured output (default 64 KB).

## Containment tests (executed 2026-11-10, WSL2 Ubuntu, bubblewrap from apt)

Source: `agent/tests/experimental/test_sandbox.py`. All 8 pass. On hosts without working bubblewrap or unprivileged user namespaces the tests skip (`sandbox.available()` probes this).

| Attack or failure | Expected | Observed |
|---|---|---|
| Control: write and read a file in the workspace | works | pass |
| Write to `/usr`, `/etc`, `/` | denied | pass (after a fix: root was a writable tmpfs until `--remount-ro /` was added; the first run of this test failed on `touch /x`) |
| Read a host file outside the workspace; check `~/.ssh` | not visible | pass |
| Read a host environment secret | not inherited | pass (`--clearenv`) |
| Connect to a TCP listener on the host's loopback | refused, listener sees no connection | pass (separate network namespace) |
| Allocate 1 GB with a 256 MB limit | fails | pass |
| `sleep 30` with a 1 s timeout | killed, returns within 10 s | pass |
| Endless output with a 4 KB cap | truncated, not buffered without bound | pass |

## What this does not protect against

- **Kernel or bubblewrap escape.** Namespaces share the host kernel. SandboxEscapeBench reports frontier models escaping default container setups with common misconfigurations and recommends nesting a sandbox inside a VM ([arXiv 2603.02277](https://arxiv.org/html/2603.02277), 2026-03; authors' claim, not reproduced here). Use a microVM for hostile multi-tenant workloads.
- **Data inside the workspace.** Anything mounted at `/work` is readable and writable by the command.
- **CPU and process count.** Only wall clock bounds CPU use. There is no process-count or disk-quota limit beyond the output cap.
- **Network egress is all-or-nothing.** Nothing is allowlisted; the command cannot reach the network at all.
- **Prompt injection itself.** The sandbox contains what a command can do. It does not decide whether a command should run. That decision is the harness's capability policy: in `world.py`, a privileged call driven by tool output is a `tainted_privileged_call` violation.
- **Windows hosts.** Not supported (tests skip). Run under WSL2 or Linux.
