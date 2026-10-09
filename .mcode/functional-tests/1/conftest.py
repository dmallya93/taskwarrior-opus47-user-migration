"""Shared helpers for milestone-1 functional tests of the Rust `task` binary.

The origin (C++ Taskwarrior 3.5.0) is never built or executed in this project,
so the origin oracle below derives the expected bytes directly from the C++
source in the origin repo:

  * VERSION from CMakeLists.txt (same regex as test/version.test.py),
  * the compiled-in default .taskrc text from src/Context.cpp,
  * libshared Configuration::parse/load semantics (comments, trim, include
    search order, Path::expand, json::decode, nesting limit, malformed lines),
  * CmdVersion / CmdShowRaw / CmdConfig / Context / main.cpp message strings.

Every test computes an `expected` (origin-derived) result and the `actual`
result from running target/release/task, records both into a JSONL file
(FT_RECORD_FILE) for result reporting, and asserts byte equality.
"""

import datetime
import json
import os
import re
import subprocess
import time

import pytest

WORKSPACE = os.environ.get("WORKSPACE_DIR", "/l2l/workspace")
TARGET_REPO = os.path.join(WORKSPACE, "taskwarrior-opus47-user-migration")
ORIGIN_REPO = os.path.join(WORKSPACE, "taskwarrior-opus47-dmallya93")
TASK_BIN = os.environ.get("FT_TASK_BIN", os.path.join(TARGET_REPO, "target/release/task"))
RECORD_FILE = os.environ.get("FT_RECORD_FILE", "/tmp/ft1_records.jsonl")

# TASK_RCDIR equivalent: the bundled theme/holiday files (identical copies of origin doc/rc).
ORIGIN_RCDIR = os.path.join(ORIGIN_REPO, "doc", "rc")


# --------------------------------------------------------------------------
# Origin oracle (derived from C++ source)
# --------------------------------------------------------------------------
def origin_version():
    ver = re.compile(r'^ *VERSION ([0-9]+\.[0-9]+\.[0-9]+[^"]*)$')
    with open(os.path.join(ORIGIN_REPO, "CMakeLists.txt")) as fh:
        for line in fh:
            m = ver.match(line)
            if m:
                return m.group(1).strip()
    raise ValueError("no VERSION in CMakeLists.txt")


VERSION = origin_version()


def _c_unescape(s):
    out, i = [], 0
    table = {"n": "\n", "t": "\t", '"': '"', "\\": "\\", "r": "\r", "'": "'"}
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            out.append(table.get(s[i + 1], s[i + 1]))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def origin_defaults_text():
    """The verbatim `configurationDefaults` string from origin src/Context.cpp."""
    src = open(os.path.join(ORIGIN_REPO, "src", "Context.cpp"), encoding="utf-8").read()
    start = src.index("std::string configurationDefaults =")
    end = src.index('";\n', start) + 2
    block = src[start:end]
    lits = re.findall(r'"((?:[^"\\]|\\.)*)"', block)
    return "".join(_c_unescape(x) for x in lits)


WS = " \t\n\f\r"


def path_expand(value, env):
    """libshared Path::expand: leading ~ and $VAR expansion."""
    res, i = [], 0
    while i < len(value):
        c = value[i]
        if i == 0 and c == "~":
            slash = value.find("/", 1)
            if slash == -1:
                slash = len(value)
            if slash == 1:
                res.append(env.get("HOME", ""))
            else:
                res.append("/home/" + value[1:slash])
            i = slash
            continue
        if c == "$":
            end = i + 1
            while end < len(value) and (value[end].isalnum() or value[end] == "_"):
                end += 1
            if end == i + 1:
                res.append("$")
            else:
                res.append(env.get(value[i + 1:end], ""))
            i = end
            continue
        res.append(c)
        i += 1
    return "".join(res)


def json_decode(s):
    out, pos = [], 0
    simple = {'"': '"', "\\": "\\", "/": "/", "b": "\b", "f": "\f", "n": "\n", "r": "\r", "t": "\t"}
    while pos < len(s):
        if s[pos] == "\\":
            pos += 1
            if pos >= len(s):
                out.append("\\")
            elif s[pos] in simple:
                out.append(simple[s[pos]])
            elif s[pos] == "u":
                out.append(chr(int(s[pos + 1:pos + 5], 16)))
                pos += 4
            else:
                out.append("\\" + s[pos])
            pos += 1
        else:
            out.append(s[pos])
            pos += 1
    return "".join(out)


def json_encode(s):
    m = {'"': '\\"', "\\": "\\\\", "/": "\\/", "\b": "\\b", "\f": "\\f",
         "\n": "\\n", "\r": "\\r", "\t": "\\t"}
    return "".join(m.get(c, c) for c in s)


class OracleError(Exception):
    pass


def cfg_parse(cfg, text, nest, file_path, env, cwd):
    if not text:
        return
    for line in text.split("\n"):
        pound = line.find("#")
        if pound != -1:
            line = line[:pound]
        line = line.strip(WS)
        if not line:
            continue
        eq = line.find("=")
        if eq != -1:
            key = line[:eq].strip(WS)
            val = path_expand(line[eq + 1:].strip(WS), env)
            cfg[key] = json_decode(val)
            continue
        inc = line.find("include")
        if inc == -1:
            raise OracleError("Malformed entry '%s' in config file." % line)
        included = path_expand(line[inc + 7:].strip(WS), env)
        resolved = included
        if not included.startswith("/"):
            if os.path.exists(os.path.join(cwd, included)):
                resolved = os.path.join(cwd, included)
            else:
                found = None
                if file_path:
                    d = os.path.dirname(os.path.realpath(file_path))
                    cand = os.path.join(d, included)
                    if os.path.exists(cand):
                        found = cand
                if found is None:
                    cand = os.path.join(ORIGIN_RCDIR, included)
                    if os.path.exists(cand):
                        found = cand
                if found is None:
                    raise OracleError(
                        "Could not find file in CWD, directory of config file or search paths '%s'."
                        % included)
                resolved = found
        cfg_load(cfg, resolved, nest + 1, env, cwd)


def cfg_load(cfg, path, nest, env, cwd):
    if nest > 10:
        raise OracleError("Configuration files may only be nested to 10 levels.")
    if os.path.exists(path) and os.access(path, os.R_OK):
        with open(path, encoding="utf-8") as fh:
            contents = fh.read()
        if contents:
            cfg_parse(cfg, contents, nest, path, env, cwd)


def origin_config(rc_path, env, overrides=(), cwd="/", taskdata=None, forcecolor=False):
    """Effective config after C++ Context::initialize for a non-TTY stdout."""
    cfg = {}
    cfg_parse(cfg, origin_defaults_text(), 1, "", env, cwd)
    cfg_load(cfg, rc_path, 1, env, cwd)
    for raw in overrides:  # CLI2::applyOverrides
        if raw.startswith("rc."):
            m = re.search(r"[:=]", raw[3:])
            if m:
                sep = 3 + m.start()
                cfg[raw[3:sep]] = raw[sep + 1:]
    if taskdata is not None:
        cfg["data.location"] = taskdata
    # Context::color(): stdout is not a tty
    cfg["detection"] = "off"
    cfg["color"] = "off"
    if forcecolor or cfg.get("_forcecolor", "").lower() in ("1", "on", "yes", "y", "true", "t"):
        cfg["color"] = "on"
    return cfg


def origin_show(cfg):
    return "".join("%s=%s\n" % (k, cfg[k]) for k in sorted(cfg))


def wrap(text, width):
    """libshared extractLine word wrapping (ASCII, single spaces)."""
    lines, cur = [], ""
    for word in text.split(" "):
        if not cur:
            cur = word
        elif len(cur) + 1 + len(word) <= width:
            cur += " " + word
        else:
            lines.append(cur)
            cur = word
    if cur:
        lines.append(cur)
    return lines


DISCLAIMER = ("Taskwarrior may be copied only under the terms of the MIT license, which may be "
              "found in the Taskwarrior source kit.")
LINK = ("Documentation for Taskwarrior can be found using 'man task', 'man taskrc', 'man "
        "task-color', 'man task-sync' or at https://taskwarrior.org")


def origin_version_banner(width=80, bold=False):
    if width == 0:
        width = 65536
    b = (lambda s: "\033[1m%s\033[0m" % s) if bold else (lambda s: s)
    table = lambda t: "".join(l.rstrip(" ") + "\n" for l in wrap(t, width))
    return ("\n" + "%s %s built for Linux\n" % (b("task"), b(VERSION))
            + "Copyright (C) 2006 - %d T. Babej, P. Beckingham, F. Hernandez.\n" % datetime.date.today().year
            + "\n" + table(DISCLAIMER) + "\n" + table(LINK) + "\n")


# --------------------------------------------------------------------------
# Target runner + sandbox
# --------------------------------------------------------------------------
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
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as fh:
            fh.write(content)
        return path

    def read_rc(self, path=None):
        with open(path or self.taskrc) as fh:
            return fh.read()

    def base_env(self, taskrc=True):
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": self.home, "LANG": "C"}
        if taskrc:
            env["TASKRC"] = self.taskrc
        return env

    def run(self, args, stdin=None, env=None, cwd=None):
        env = self.base_env() if env is None else env
        t0 = time.time()
        p = subprocess.run([TASK_BIN] + list(args), input=stdin if stdin is not None else "",
                           capture_output=True, text=True, env=env, cwd=cwd or self.root, timeout=30)
        p.duration_ms = (time.time() - t0) * 1000.0
        return p


@pytest.fixture
def sb(tmp_path):
    return Sandbox(tmp_path)


@pytest.fixture
def check(request):
    """check(proc, args, exp_stdout, exp_stderr, exp_exit, stdin=None, extra=None)

    Records origin-derived expectation and target actual, then asserts equality.
    exp_stdout / exp_stderr may be None to skip comparing that stream.
    """
    def _check(proc, args, exp_stdout, exp_stderr, exp_exit, stdin=None, extra=None):
        rec = {
            "name": request.node.name,
            "args": list(args),
            "stdin": stdin,
            "expected_stdout": exp_stdout,
            "expected_stderr": exp_stderr,
            "expected_exit": exp_exit,
            "actual_stdout": proc.stdout,
            "actual_stderr": proc.stderr,
            "actual_exit": proc.returncode,
            "duration_ms": round(getattr(proc, "duration_ms", 0.0), 1),
            "extra": extra,
        }
        mism = []
        if exp_stdout is not None and proc.stdout != exp_stdout:
            mism.append("stdout")
        if exp_stderr is not None and proc.stderr != exp_stderr:
            mism.append("stderr")
        if proc.returncode != exp_exit:
            mism.append("exit")
        if extra and extra.get("ok") is False:
            mism.append(extra.get("what", "extra"))
        rec["mismatch"] = mism
        with open(RECORD_FILE, "a") as fh:
            fh.write(json.dumps(rec) + "\n")
        assert proc.returncode == exp_exit, (proc.returncode, proc.stderr)
        if exp_stdout is not None:
            assert proc.stdout == exp_stdout
        if exp_stderr is not None:
            assert proc.stderr == exp_stderr
        if extra:
            assert extra.get("ok") is not False, extra
    return _check
