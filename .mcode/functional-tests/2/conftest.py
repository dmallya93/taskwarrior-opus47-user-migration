"""Shared helpers for milestone-2 functional tests (Datetime / Duration / JSON / Lexer wiring).

The same test files run against either binary:

  * origin: the C++ Taskwarrior 3.5.0 built from a copy of the origin repo by the
    origin lifecycle (FT_SIDE=origin FT_TASK_BIN=$ORIGIN_BUILD_DIR/build/src/task)
  * target: the Rust port (default: target/release/task)

Every test asserts the spec-derived behaviour AND appends each executed case
(args, env deltas, stdout, stderr, exit code) to FT_RECORD_FILE
(/tmp/ft2_<side>.jsonl by default) so origin and target can be compared
case-by-case with `_compare.py`.

Time is pinned with libfaketime in "frozen absolute epoch" mode
(FAKETIME=<epoch>, FAKETIME_FMT=%s) so the fake instant is independent of TZ;
the TZ variable then decides how that instant is rendered locally.
"""

import json
import os
import pty
import select
import subprocess
import time

import pytest

WORKSPACE = os.environ.get("WORKSPACE_DIR", "/l2l/workspace")
TARGET_REPO = os.path.join(WORKSPACE, "taskwarrior-opus47-user-migration")
SIDE = os.environ.get("FT_SIDE", "target")
TASK_BIN = os.environ.get("FT_TASK_BIN", os.path.join(TARGET_REPO, "target/release/task"))
RECORD_FILE = os.environ.get("FT_RECORD_FILE", "/tmp/ft2_%s.jsonl" % SIDE)

VERSION = "3.5.0"
WEEKSTART_ERROR = "The 'weekstart' configuration variable may only contain 'Sunday' or 'Monday'.\n"

_FAKETIME_CANDIDATES = [
    "/usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1",
    "/usr/lib/aarch64-linux-gnu/faketime/libfaketime.so.1",
    "/usr/lib/faketime/libfaketime.so.1",
]
LIBFAKETIME = next((p for p in _FAKETIME_CANDIDATES if os.path.exists(p)), None)


def faketime_env(epoch):
    if LIBFAKETIME is None:
        pytest.fail("libfaketime not installed")
    return {
        "LD_PRELOAD": LIBFAKETIME,
        "FAKETIME": str(int(epoch)),
        "FAKETIME_FMT": "%s",
        "FAKETIME_DONT_FAKE_MONOTONIC": "1",
    }


class Sandbox:
    def __init__(self, root):
        self.root = str(root)
        self.home = os.path.join(self.root, "home")
        self.data = os.path.join(self.root, "data")
        os.makedirs(self.home)
        os.makedirs(self.data)
        self.taskrc = os.path.join(self.root, "test.rc")
        self.write_rc("data.location=%s\n" % self.data)

    def write_rc(self, content, path=None):
        path = path or self.taskrc
        with open(path, "w") as fh:
            fh.write(content)
        return path

    def env(self, extra=None, taskrc=True):
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": self.home, "LANG": "C"}
        if taskrc:
            env["TASKRC"] = self.taskrc
        if extra:
            env.update(extra)
        return env

    def run(self, args, extra_env=None, stdin=""):
        t0 = time.time()
        p = subprocess.run([TASK_BIN] + list(args), input=stdin, capture_output=True, text=True,
                           env=self.env(extra_env), cwd=self.root, timeout=30)
        p.duration_ms = (time.time() - t0) * 1000.0
        return p

    def run_pty(self, args, env):
        """Run with stdout/stderr on a pseudo-terminal (interactive first run)."""
        t0 = time.time()
        pid, fd = pty.fork()
        if pid == 0:  # child
            try:
                os.chdir(self.root)
                os.execve(TASK_BIN, [TASK_BIN] + list(args), env)
            finally:
                os._exit(127)
        out = b""
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                r, _, _ = select.select([fd], [], [], 1)
                if not r:
                    continue
                chunk = os.read(fd, 4096)
                if not chunk:
                    break
                out += chunk
            except OSError:
                break
        _, status = os.waitpid(pid, 0)
        os.close(fd)

        class R:
            pass
        res = R()
        res.returncode = os.waitstatus_to_exitcode(status)
        res.stdout = out.decode("utf-8", "replace")
        res.stderr = ""
        res.duration_ms = (time.time() - t0) * 1000.0
        return res


@pytest.fixture
def sb(tmp_path):
    return Sandbox(tmp_path)


@pytest.fixture
def record(request):
    """record(proc, args, env_delta=None, extra=None) -> appends one case to FT_RECORD_FILE."""
    def _record(proc, args, env_delta=None, extra=None):
        rec = {
            "test": request.node.originalname or request.node.name,
            "node": request.node.name,
            "args": list(args),
            "env": env_delta or {},
            "stdout": proc.stdout,
            "stderr": proc.stderr,
            "exit": proc.returncode,
            "duration_ms": round(getattr(proc, "duration_ms", 0.0), 1),
            "extra": extra,
        }
        with open(RECORD_FILE, "a") as fh:
            fh.write(json.dumps(rec) + "\n")
        return rec
    return _record
