"""Shared helpers for milestone-7 functional tests (mutation commands, import,
hooks, recurrence, undo, sync, purge, Task::diff).

The same test files run against either side:

  * origin: C++ Taskwarrior 3.5.0 built from a throwaway copy of the origin repo
    (FT_SIDE=origin FT_TASK_BIN=<build>/src/task), e.g.
    `cmake -S <copy> -B /tmp/ft7-origin/build && cmake --build ... --target task_executable`
  * target: the Rust port (default: target/release/task)

Each test gets an isolated sandbox (own TASKRC + data.location, HOME, cwd).
Time is frozen with libfaketime (TZ=UTC) so entry/modified timestamps, ages,
relative dates and urgency are deterministic. Every executed command is
appended to FT_RECORD_FILE (/tmp/ft7_<side>.jsonl by default) with normalized
stdout/stderr (UUIDs -> description labels, sandbox path -> <SANDBOX>, build
hash -> <HASH>) so origin and target can be compared case-by-case with
`_compare.py`.
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
RECORD_FILE = os.environ.get("FT_RECORD_FILE", "/tmp/ft7_%s.jsonl" % SIDE)

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
        self.epoch = FROZEN_EPOCH
        # default rc does `include default.theme`; C++ resolves it relative to CWD /
        # the rc file directory, so expose the reference doc/rc themes there.
        for name in os.listdir(RCDIR):
            os.symlink(os.path.join(RCDIR, name), os.path.join(self.root, name))
        with open(self.taskrc, "w") as fh:
            # recurrence generation is opted into per test with rc.recurrence=on
            fh.write("data.location=%s\nconfirmation=off\nnews.version=3.5.0\nrecurrence=off\n"
                     % self.data)
            fh.write(rc_extra)

    def append_rc(self, text):
        with open(self.taskrc, "a") as fh:
            fh.write(text)

    def env(self, extra=None, epoch=None):
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": self.home,
               "LANG": "C.UTF-8", "TZ": "UTC", "TASKRC": self.taskrc}
        env.update(faketime_env(epoch if epoch is not None else self.epoch))
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
        s = re.sub(r"3\.5\.0 \([0-9a-f]+\)", "3.5.0 (<HASH>)", s)
        return s

    def short_uuid_map(self):
        """{short uuid: label} for every task, so 8-char uuid columns normalize."""
        p = subprocess.run([TASK_BIN, "rc.verbose=nothing", "rc.json.array=on", "export"],
                           capture_output=True, text=True, env=self.env(), cwd=self.root,
                           timeout=60)
        out = {}
        if p.returncode == 0 and p.stdout.strip():
            for t in json.loads(p.stdout):
                out[t["uuid"][:8]] = "<S:%s>" % t["description"][:6]
        return out

    def run(self, args, extra_env=None, stdin=None, epoch=None):
        t0 = time.time()
        p = subprocess.run([TASK_BIN] + list(args), capture_output=True, text=True,
                           env=self.env(extra_env, epoch), cwd=self.root, timeout=60,
                           input=stdin)
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

    def _task(*args, extra_env=None, stdin=None, record=True, short_uuids=False,
              epoch=None, unordered=False):
        """short_uuids=True: 8-hex-char short UUIDs of existing tasks (uuid.short
        column) are replaced by stable labels before recording.
        unordered=True: output order is unspecified (replica order), so the
        recorded stdout is normalized to sorted lines."""
        before = sb.short_uuid_map() if short_uuids else {}
        p = sb.run(args, extra_env, stdin, epoch)
        if short_uuids:
            m = dict(before)
            m.update(sb.short_uuid_map())
            for k, v in m.items():
                p.nout = p.nout.replace(k, v)
        if record:
            out = p.nout
            if unordered and out:
                out = "\n".join(sorted(l.rstrip(",") for l in out.strip("\n").split("\n"))) + "\n"
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


def add(task, *args, epoch=None):
    p = task("rc.verbose=nothing", *args, record=False, epoch=epoch)
    assert p.returncode == 0, (args, p.stdout, p.stderr)
    return p


DAY = 86400


def seed_reports(task):
    """A deterministic, varied dataset (frozen now = 2009-02-13 12:00 UTC).

    Entries are created at different (fake) times so age/newest/oldest differ.
    ids: 1 'alpha one'   project:home +work due:2009-02-20 priority:H   (entry now-5d)
         2 'beta two'    project:home.garden +home                      (entry now-4d)
         3 'gamma three' project:work due:2009-02-10 (overdue)          (entry now-3d)
         4 'delta four'  (no attributes)                                (entry now-2d)
         5 'eta seven'   depends:1 project:work priority:L scheduled:2009-02-12
         6 'theta eight' start:2009-02-13T10:00:00 (active) priority:M
         7 'iota nine'   wait:2009-03-01 (waiting)
         8 'kappa ten'   recur:weekly due:2009-02-16 (recurring template + instance)
    logged (completed): 'epsilon five' project:home end=now-1d
    """
    now = FROZEN_EPOCH
    add(task, "add", "alpha", "one", "project:home", "+work", "due:2009-02-20", "priority:H",
        epoch=now - 5 * DAY)
    add(task, "add", "beta", "two", "project:home.garden", "+home", epoch=now - 4 * DAY)
    add(task, "add", "gamma", "three", "project:work", "due:2009-02-10", epoch=now - 3 * DAY)
    add(task, "add", "delta", "four", epoch=now - 2 * DAY)
    add(task, "log", "epsilon", "five", "project:home", epoch=now - 1 * DAY)
    add(task, "add", "eta", "seven", "depends:1", "project:work", "priority:L",
        "scheduled:2009-02-12", epoch=now - 3600)
    add(task, "add", "theta", "eight", "start:2009-02-13T10:00:00", "priority:M",
        epoch=now - 1800)
    add(task, "add", "iota", "nine", "wait:2009-03-01", epoch=now - 900)
    add(task, "add", "kappa", "ten", "recur:weekly", "due:2009-02-16", epoch=now - 600)


MAKE_BIN = os.environ.get("FT_MAKE_TC_TASK_BIN",
                          os.path.join(TARGET_REPO, "target/release/make_tc_task"))


def make_tc_task(sb, **props):
    """Write a task straight into the replica (seed helper, same tool for both
    sides) - used for annotations / deleted tasks that add/log cannot create."""
    args = [MAKE_BIN, sb.data] + ["%s=%s" % (k, v) for k, v in props.items()]
    p = subprocess.run(args, capture_output=True, text=True, env=sb.env(), cwd=sb.root,
                       timeout=60)
    assert p.returncode == 0, p.stderr
    return p.stdout.strip()


def seed_recurrence(sb):
    """A recurring template with mask '-' plus its pending child (imask 0),
    written directly (recurrence generation is out of milestone-6 scope)."""
    now = FROZEN_EPOCH
    tpl = make_tc_task(sb, status="recurring", description="lambda weekly", recur="weekly",
                       due=str(now - 2 * DAY), mask="-", rtype="periodic",
                       entry=str(now - 3 * DAY), modified=str(now - 3 * DAY))
    make_tc_task(sb, status="pending", description="lambda weekly", recur="weekly",
                 due=str(now - 2 * DAY), parent=tpl, imask="0", rtype="periodic",
                 entry=str(now - 3 * DAY), modified=str(now - 3 * DAY))


HOOK_FIXTURES = os.path.join(WORKSPACE, "taskwarrior-opus47-dmallya93", "test", "test_hooks")


def install_hook(sb, name, body=None, src=None):
    """Install a hook script into <data.location>/hooks (default discovery dir).
    Either copy a reference fixture (src=name in test/test_hooks) or write body."""
    hooks = os.path.join(sb.data, "hooks")
    os.makedirs(hooks, exist_ok=True)
    path = os.path.join(hooks, name)
    if src is not None:
        with open(os.path.join(HOOK_FIXTURES, src)) as fh:
            body = fh.read()
    with open(path, "w") as fh:
        fh.write(body)
    os.chmod(path, 0o755)
    return path


def export(task, *filt):
    """Recorded export (stored data comparison); replica order of id-0 tasks is
    UUID-dependent, so lines are compared sorted."""
    return task("rc.verbose=nothing", "rc.json.array=on", *filt, "export", unordered=True)
