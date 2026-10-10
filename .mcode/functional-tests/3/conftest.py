"""Shared helpers for milestone-3 functional tests (Variant / Eval / calc / task calc).

The same test files run against either side:

  * origin: C++ Taskwarrior 3.5.0 built from a throwaway copy of the origin repo
    (FT_SIDE=origin FT_CALC_BIN=<build>/src/calc FT_TASK_BIN=<build>/src/task)
  * target: the Rust port (default: target/release/{calc,task})

Every executed case (argv, env delta, stdout, stderr, exit code) is appended to
FT_RECORD_FILE (/tmp/ft3_<side>.jsonl by default) so origin and target can be
compared case-by-case with `_compare.py`.

Time-dependent expressions (now, today, yesterday, ...) are pinned with
libfaketime in frozen absolute-epoch mode and TZ=UTC.
"""

import json
import os
import subprocess
import time

import pytest

WORKSPACE = os.environ.get("WORKSPACE_DIR", "/l2l/workspace")
TARGET_REPO = os.path.join(WORKSPACE, "taskwarrior-opus47-user-migration")
SIDE = os.environ.get("FT_SIDE", "target")
CALC_BIN = os.environ.get("FT_CALC_BIN", os.path.join(TARGET_REPO, "target/release/calc"))
TASK_BIN = os.environ.get("FT_TASK_BIN", os.path.join(TARGET_REPO, "target/release/task"))
RCDIR = os.path.join(WORKSPACE, "taskwarrior-opus47-dmallya93", "doc", "rc")
RECORD_FILE = os.environ.get("FT_RECORD_FILE", "/tmp/ft3_%s.jsonl" % SIDE)

# 2009-02-13T12:00:00Z (a Friday, midday so today != now)
FROZEN_EPOCH = 1234526400

_FAKETIME_CANDIDATES = [
    "/usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1",
    "/usr/lib/aarch64-linux-gnu/faketime/libfaketime.so.1",
    "/usr/lib/faketime/libfaketime.so.1",
]
LIBFAKETIME = next((p for p in _FAKETIME_CANDIDATES if os.path.exists(p)), None)


def faketime_env(epoch=FROZEN_EPOCH):
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
        # The default rc does `include default.theme`; the C++ build's compiled-in TASK_RCDIR
        # (/usr/local/share/doc/task/rc) is not installed, so put the reference theme files in
        # the "directory of the config file" search location (same on both sides).
        for name in os.listdir(RCDIR):
            os.symlink(os.path.join(RCDIR, name), os.path.join(self.root, name))
        with open(self.taskrc, "w") as fh:
            fh.write("data.location=%s\nconfirmation=off\nverbose=nothing\n" % self.data)

    def env(self, extra=None):
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": self.home,
               "LANG": "C.UTF-8", "TZ": "UTC", "TASKRC": self.taskrc}
        if extra:
            env.update(extra)
        return env

    def _run(self, argv, extra_env=None):
        t0 = time.time()
        p = subprocess.run(argv, capture_output=True, text=True, env=self.env(extra_env),
                           cwd=self.root, timeout=30)
        p.duration_ms = (time.time() - t0) * 1000.0
        return p

    def calc(self, args, extra_env=None):
        """Run the calc binary. argv[0] is the bare name 'calc' so --help output is stable."""
        t0 = time.time()
        p = subprocess.run(["calc"] + list(args), executable=CALC_BIN, capture_output=True,
                           text=True, env=self.env(extra_env), cwd=self.root, timeout=30)
        p.duration_ms = (time.time() - t0) * 1000.0
        return p

    def task(self, args, extra_env=None):
        return self._run([TASK_BIN] + list(args), extra_env)


@pytest.fixture
def sb(tmp_path):
    return Sandbox(tmp_path)


@pytest.fixture
def record(request):
    """record(proc, prog, args, env_delta=None) -> appends one case to FT_RECORD_FILE."""
    def _record(proc, prog, args, env_delta=None):
        rec = {
            "test": request.node.originalname or request.node.name,
            "node": request.node.name,
            "prog": prog,
            "args": list(args),
            "env": {k: v for k, v in (env_delta or {}).items() if k != "LD_PRELOAD"},
            "stdout": proc.stdout,
            "stderr": proc.stderr,
            "exit": proc.returncode,
            "duration_ms": round(getattr(proc, "duration_ms", 0.0), 1),
        }
        with open(RECORD_FILE, "a") as fh:
            fh.write(json.dumps(rec, ensure_ascii=False) + "\n")
        return rec
    return _record
