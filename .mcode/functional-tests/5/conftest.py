"""Shared helpers for milestone-5 functional tests (CLI2 parser, Filter, DOM,
Context dispatch, feedback, add/log/export/_get/count/ids family/_unique/
_zshattributes).

The same test files run against either side:

  * origin: C++ Taskwarrior 3.5.0 built from a throwaway copy of the origin repo
    (FT_SIDE=origin FT_TASK_BIN=<build>/src/task), e.g.
    `cmake -S <copy> -B /tmp/ft5-origin/build && cmake --build ... --target task_executable`
  * target: the Rust port (default: target/release/task)

Each test gets an isolated sandbox (own TASKRC + data.location, HOME, cwd).
Time is frozen with libfaketime (TZ=UTC) so entry/modified/end timestamps and
urgency are deterministic. Every executed command is appended to
FT_RECORD_FILE (/tmp/ft5_<side>.jsonl by default) with normalized stdout/stderr
(UUIDs -> positional <Un>, sandbox path -> <SANDBOX>, build hash -> <HASH>) so
origin and target can be compared case-by-case with `_compare.py`.
"""

import json
import os
import re
import subprocess
import time

import pytest

TARGET_REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
WORKSPACE = os.environ.get("WORKSPACE_DIR", os.path.dirname(TARGET_REPO))
SIDE = os.environ.get("FT_SIDE", "target")
TASK_BIN = os.environ.get("FT_TASK_BIN", os.path.join(TARGET_REPO, "target/release/task"))
RCDIR = os.path.join(WORKSPACE, "taskwarrior-opus47-dmallya93", "doc", "rc")
RECORD_FILE = os.environ.get("FT_RECORD_FILE", "/tmp/ft5_%s.jsonl" % SIDE)

UUID_ANY = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")

# 2009-02-13T12:00:00Z (Friday)
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
    return {"LD_PRELOAD": LIBFAKETIME, "FAKETIME": str(int(epoch)), "FAKETIME_FMT": "%s",
            "FAKETIME_DONT_FAKE_MONOTONIC": "1"}


class Sandbox:
    def __init__(self, root, rc_extra=""):
        self.root = str(root)
        self.home = os.path.join(self.root, "home")
        self.data = os.path.join(self.root, "data")
        os.makedirs(self.home)
        os.makedirs(self.data)
        self.taskrc = os.path.join(self.root, "test.rc")
        self.uuids = {}
        # default rc does `include default.theme`; C++ resolves it relative to CWD /
        # the rc file directory, so expose the reference doc/rc themes there.
        for name in os.listdir(RCDIR):
            os.symlink(os.path.join(RCDIR, name), os.path.join(self.root, name))
        with open(self.taskrc, "w") as fh:
            fh.write("data.location=%s\nconfirmation=off\nnews.version=3.5.0\n" % self.data)
            fh.write(rc_extra)

    def env(self, extra=None):
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": self.home,
               "LANG": "C.UTF-8", "TZ": "UTC", "TASKRC": self.taskrc}
        env.update(faketime_env())
        if extra:
            env.update(extra)
        return env

    def _label(self, u):
        """Stable, side-independent label for a UUID: the task's description
        (looked up once, unrecorded), else a positional placeholder."""
        if u not in self.uuids:
            p = subprocess.run([TASK_BIN, "rc.verbose=nothing", "_get", u + ".description"],
                               capture_output=True, text=True, env=self.env(), cwd=self.root,
                               timeout=60)
            d = p.stdout.strip() if p.returncode == 0 else ""
            self.uuids[u] = "<UUID:%s>" % d if d else "<U%d>" % len(self.uuids)
        return self.uuids[u]

    def norm(self, s):
        if s is None:
            return None
        s = s.replace(TASK_BIN, "<TASK>").replace(self.root, "<SANDBOX>")
        s = UUID_ANY.sub(lambda m: self._label(m.group(0)), s)
        # partial UUIDs (>= 8 hex chars) of known tasks, e.g. in filter args
        for u, lab in list(self.uuids.items()):
            for n in (13, 8):
                s = re.sub(r"(?<![0-9a-f-])%s(?![0-9a-f])" % re.escape(u[:n]),
                           "%s[:%d]" % (lab, n), s)
        s = re.sub(r"3\.5\.0 \([0-9a-f]+\)", "3.5.0 (<HASH>)", s)
        return s

    def run(self, args, extra_env=None, stdin=None):
        t0 = time.time()
        p = subprocess.run([TASK_BIN] + list(args), capture_output=True, text=True,
                           env=self.env(extra_env), cwd=self.root, timeout=60, input=stdin)
        p.duration_ms = (time.time() - t0) * 1000.0
        p.nout = self.norm(p.stdout)
        p.nerr = self.norm(p.stderr)
        p.args = list(args)
        return p


@pytest.fixture
def sb(tmp_path):
    return Sandbox(tmp_path)


@pytest.fixture
def task(sb, request):
    """task(*args) -> CompletedProcess; every call is recorded for comparison."""
    counter = {"n": 0}

    def _task(*args, extra_env=None, stdin=None, record=True, unordered=False):
        """unordered=True: the command's output order is unspecified (TDB2 / UUID
        order), so recorded stdout is normalized to sorted lines/words."""
        p = sb.run(args, extra_env, stdin)
        if record:
            out = p.nout
            if unordered and out:
                sep = "\n" if "\n" in out.rstrip("\n") else " "
                out = sep.join(sorted(out.strip("\n").split(sep))) + "\n"
            counter["n"] += 1
            rec = {
                "test": request.node.originalname or request.node.name,
                "node": request.node.name,
                "seq": counter["n"],
                "args": [sb.norm(a) for a in args],
                "stdout": out,
                "stderr": p.nerr,
                "exit": p.returncode,
                "duration_ms": round(p.duration_ms, 1),
            }
            with open(RECORD_FILE, "a") as fh:
                fh.write(json.dumps(rec, ensure_ascii=False, sort_keys=True) + "\n")
        return p
    return _task


def seed_basic(task):
    """Four pending tasks + one completed (logged) task with assorted attributes.

    ids: 1 'alpha one' project:home +work due:2009-02-20 priority:H
         2 'beta two'  project:home.garden +home
         3 'gamma three' project:work due:2009-02-10
         4 'delta four' (no attributes)
    logged: 'epsilon five' project:home
    """
    for args in (["add", "alpha", "one", "project:home", "+work", "due:2009-02-20", "priority:H"],
                 ["add", "beta", "two", "project:home.garden", "+home"],
                 ["add", "gamma", "three", "project:work", "due:2009-02-10"],
                 ["add", "delta", "four"],
                 ["log", "epsilon", "five", "project:home"]):
        p = task("rc.verbose=nothing", *args, record=False)
        assert p.returncode == 0, p.stderr
