"""Shared helpers for milestone-4 functional tests (Task model / TDB2 on native
taskchampion / make_tc_task).

The same test files run against either side:

  * origin: C++ Taskwarrior 3.5.0 built from a throwaway copy of the origin repo
    (FT_SIDE=origin FT_TASK_BIN=<build>/src/task FT_MAKE_TC_TASK_BIN=<build>/test/make_tc_task)
    e.g. `cmake -S <copy> -B /tmp/ft4-origin/build && cmake --build ... --target task_executable make_tc_task`
  * target: the Rust port (default: target/release/{task,make_tc_task})

Most user-facing commands that read data (export, info, list, ...) are still stubs
in the target at this milestone, so replicas written by the *target* make_tc_task
are additionally read back by an independent reader: Python's sqlite3 on the
taskchampion tables and, when available, the C++ reference `task`
(FT_READER_TASK_BIN, default /tmp/ft4-origin/build/src/task).

Every executed case is appended to FT_RECORD_FILE (/tmp/ft4_<side>.jsonl by
default) with a normalized "observed" payload so origin and target can be
compared case-by-case with `_compare.py`.
"""

import json
import os
import re
import sqlite3
import subprocess
import time

import pytest

# Repo root is three levels above this file (.mcode/functional-tests/4/).
TARGET_REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
WORKSPACE = os.environ.get("WORKSPACE_DIR", os.path.dirname(TARGET_REPO))
SIDE = os.environ.get("FT_SIDE", "target")
TASK_BIN = os.environ.get("FT_TASK_BIN", os.path.join(TARGET_REPO, "target/release/task"))
MAKE_BIN = os.environ.get("FT_MAKE_TC_TASK_BIN",
                          os.path.join(TARGET_REPO, "target/release/make_tc_task"))
READER_TASK_BIN = os.environ.get("FT_READER_TASK_BIN", "/tmp/ft4-origin/build/src/task")
RCDIR = os.path.join(WORKSPACE, "taskwarrior-opus47-dmallya93", "doc", "rc")
RECORD_FILE = os.environ.get("FT_RECORD_FILE", "/tmp/ft4_%s.jsonl" % SIDE)

UUID_RE = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
UUID_ANY = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")

# 2009-02-13T12:00:00Z
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


def norm_text(s, sandbox_root=None):
    if s is None:
        return None
    if sandbox_root:
        s = s.replace(sandbox_root, "<SANDBOX>")
    s = UUID_ANY.sub("<UUID>", s)
    # build git short hash in `_version` output (origin vs target commit)
    s = re.sub(r"3\.5\.0 \([0-9a-f]+\)", "3.5.0 (<HASH>)", s)
    # wall-clock timestamps of make_tc_task operations in the `info` journal
    s = re.sub(r"(?m)^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} (?=\S)", "<TS> ", s)
    return s


def read_replica(path):
    """Snapshot the taskchampion SQLite replica: tasks (uuid -> dict), working_set
    (id -> uuid), operations (list of decoded JSON), version row."""
    if not os.path.exists(path):
        return None
    con = sqlite3.connect("file:%s?mode=ro" % path, uri=True)
    try:
        tables = sorted(r[0] for r in con.execute(
            "select name from sqlite_master where type='table'"))
        tasks = {u: json.loads(d) for u, d in con.execute("select uuid, data from tasks")}
        ws = [(i, u) for i, u in con.execute("select id, uuid from working_set order by id")]
        ops = [json.loads(d) for (d,) in con.execute("select data from operations order by id")]
        synced = [s for (s,) in con.execute("select synced from operations order by id")]
        version = list(con.execute("select major, minor from version"))
    finally:
        con.close()
    return {"tables": tables, "tasks": tasks, "working_set": ws, "operations": ops,
            "synced": synced, "version": version}


def normalize_replica(snap, uuid_order=None):
    """Replace UUIDs by positional placeholders and drop operation timestamps."""
    if snap is None:
        return None
    mapping = {}
    for u in (uuid_order or []):
        mapping.setdefault(u, "<U%d>" % len(mapping))
    for op in snap["operations"]:
        for v in op.values():
            if isinstance(v, dict) and "uuid" in v:
                mapping.setdefault(v["uuid"], "<U%d>" % len(mapping))

    def m(u):
        return mapping.setdefault(u, "<U%d>" % len(mapping))

    ops = []
    for op in snap["operations"]:
        (kind, body), = op.items()
        if isinstance(body, dict):
            body = dict(body)
            if "uuid" in body:
                body["uuid"] = m(body["uuid"])
            if "timestamp" in body:
                body["timestamp"] = "<TS>"
        ops.append({kind: body})
    return {
        "tables": snap["tables"],
        "tasks": {m(u): d for u, d in sorted(snap["tasks"].items(), key=lambda kv: m(kv[0]))},
        "working_set": [[i, m(u)] for i, u in snap["working_set"]],
        "operations": ops,
        "synced": snap["synced"],
        "version": snap["version"],
    }


class Sandbox:
    def __init__(self, root):
        self.root = str(root)
        self.home = os.path.join(self.root, "home")
        self.data = os.path.join(self.root, "data")
        os.makedirs(self.home)
        os.makedirs(self.data)
        self.taskrc = os.path.join(self.root, "test.rc")
        self.db = os.path.join(self.data, "taskchampion.sqlite3")
        # default rc does `include default.theme`; C++ resolves it relative to CWD.
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

    def run(self, argv, extra_env=None):
        t0 = time.time()
        p = subprocess.run(argv, capture_output=True, text=True, env=self.env(extra_env),
                           cwd=self.root, timeout=60)
        p.duration_ms = (time.time() - t0) * 1000.0
        return p

    def task(self, args, extra_env=None):
        return self.run([TASK_BIN] + list(args), extra_env)

    def reader(self, args, extra_env=None):
        """Independent reader (the C++ reference task) for replicas written by either side."""
        if not os.path.exists(READER_TASK_BIN):
            pytest.fail("reference reader task binary missing: %s" % READER_TASK_BIN)
        return self.run([READER_TASK_BIN] + list(args), extra_env)

    def make(self, args, datadir=None):
        """make_tc_task argv[0] is the bare name; DATADIR defaults to the sandbox data dir."""
        argv = list(args) if datadir is False else [datadir or self.data] + list(args)
        t0 = time.time()
        p = subprocess.run(["make_tc_task"] + argv, executable=MAKE_BIN, capture_output=True,
                           text=True, env=self.env(), cwd=self.root, timeout=60)
        p.duration_ms = (time.time() - t0) * 1000.0
        return p


@pytest.fixture
def sb(tmp_path):
    return Sandbox(tmp_path)


@pytest.fixture
def record(request):
    """record(proc, prog, args, observed=None) -> appends one case to FT_RECORD_FILE.

    `observed` is the normalized, side-independent payload that _compare.py compares."""
    def _record(proc, prog, args, observed=None, sandbox=None):
        root = sandbox.root if sandbox else None
        rec = {
            "test": request.node.originalname or request.node.name,
            "node": request.node.name,
            "prog": prog,
            "args": [norm_text(a, root) for a in args],
            "stdout": proc.stdout if proc is not None else None,
            "stderr": proc.stderr if proc is not None else None,
            "exit": proc.returncode if proc is not None else None,
            "observed": observed,
            "duration_ms": round(getattr(proc, "duration_ms", 0.0), 1) if proc else 0.0,
        }
        with open(RECORD_FILE, "a") as fh:
            fh.write(json.dumps(rec, ensure_ascii=False, sort_keys=True) + "\n")
        return rec
    return _record
