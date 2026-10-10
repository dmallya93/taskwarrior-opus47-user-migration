"""`make_tc_task DATADIR KEY=VALUE ..` -- test fixture helper that writes a task
straight into the taskchampion replica and prints the new UUID.

C++ reference: test/make_tc_task.cpp (usage() -> stderr, exit 1; replica opened
with create_if_missing=true, read_write=true *before* argument parsing; every
KEY=VALUE split at the first '=' and stored verbatim with TaskData::update),
test/basetest/task.py::Task.make_tc_task.
"""

import os

import pytest

from conftest import UUID_RE, norm_text, normalize_replica, read_replica

USAGE = "USAGE: make_tc_task DATADIR KEY=VALUE ..\n"


def observed_proc(p, sb):
    return {"exit": p.returncode, "stdout": norm_text(p.stdout, sb.root),
            "stderr": norm_text(p.stderr, sb.root)}


class TestMakeTcTaskUsage:
    def test_make_tc_task_no_args(self, sb, record):
        """No DATADIR -> usage on stderr, exit 1, nothing on stdout."""
        p = sb.make([], datadir=False)
        record(p, "make_tc_task", [], observed_proc(p, sb), sb)
        assert p.returncode == 1
        assert p.stdout == ""
        assert p.stderr == USAGE

    def test_make_tc_task_arg_without_equals(self, sb, record):
        """A property argument without '=' -> usage, exit 1, no task committed
        (the replica itself is still created because C++ opens it first)."""
        p = sb.make(["status=pending", "noequals"])
        snap = normalize_replica(read_replica(sb.db))
        record(p, "make_tc_task", ["<DATA>", "status=pending", "noequals"],
               {"proc": observed_proc(p, sb), "replica": snap}, sb)
        assert p.returncode == 1
        assert p.stdout == ""
        assert p.stderr == USAGE
        assert snap is not None, "replica should exist (opened before arg parsing)"
        assert snap["tasks"] == {}
        assert snap["operations"] == []

    def test_make_tc_task_datadir_is_file(self, sb, record):
        """DATADIR that is a regular file -> taskchampion error, non-zero exit."""
        path = os.path.join(sb.root, "afile")
        open(path, "w").close()
        p = sb.make(["description=x"], datadir=path)
        msg = 'Task Database Error: Cannot create directory "%s": File exists (os error 17)' % path
        record(p, "make_tc_task", ["<SANDBOX>/afile", "description=x"],
               {"nonzero": p.returncode != 0, "message_present": msg in p.stderr,
                "stdout": p.stdout}, sb)
        assert p.returncode != 0
        assert msg in p.stderr
        assert p.stdout == ""


class TestMakeTcTaskHappy:
    def test_make_tc_task_basic_pending(self, sb, record):
        """status/description/entry/tag -> prints a v4 UUID, task stored verbatim,
        pending task placed in the working set as id 1."""
        args = ["status=pending", "description=task with tags", "entry=1234567890",
                "tag_tag1=x", "tag_tag2=x"]
        p = sb.make(args)
        uuid = p.stdout.strip()
        snap = read_replica(sb.db)
        record(p, "make_tc_task", ["<DATA>"] + args,
               {"proc": observed_proc(p, sb), "replica": normalize_replica(snap, [uuid])}, sb)
        assert p.returncode == 0, p.stderr
        assert p.stderr == ""
        assert UUID_RE.match(uuid), p.stdout
        assert p.stdout == uuid + "\n"
        assert snap["tasks"] == {uuid: {"status": "pending", "description": "task with tags",
                                        "entry": "1234567890", "tag_tag1": "x", "tag_tag2": "x"}}
        assert snap["working_set"] == [(1, uuid)]
        kinds = [list(op)[0] for op in snap["operations"]]
        assert kinds == ["Create"] + ["Update"] * 5
        assert [op["Update"]["property"] for op in snap["operations"][1:]] == \
            ["status", "description", "entry", "tag_tag1", "tag_tag2"]
        assert all(op["Update"]["old_value"] is None for op in snap["operations"][1:])
        assert snap["synced"] == [0] * 6

    def test_make_tc_task_empty_task(self, sb, record):
        """No properties (unusual_task.test.py) -> an empty task is created."""
        p = sb.make([])
        uuid = p.stdout.strip()
        snap = read_replica(sb.db)
        record(p, "make_tc_task", ["<DATA>"],
               {"proc": observed_proc(p, sb), "replica": normalize_replica(snap, [uuid])}, sb)
        assert p.returncode == 0, p.stderr
        assert UUID_RE.match(uuid)
        assert snap["tasks"] == {uuid: {}}
        assert snap["working_set"] == []
        assert snap["operations"] == [{"Create": {"uuid": uuid}}]

    @pytest.mark.parametrize("args,expected", [
        (["description=a=b=c"], {"description": "a=b=c"}),
        (["description="], {"description": ""}),
        (["=value"], {"": "value"}),
        (["description=hé ☃ 日本"], {"description": "hé ☃ 日本"}),
        (["description=tab\\ttab", "status=pending"], {"description": "tab\\ttab", "status": "pending"}),
        (["description=" + "x" * 5000], {"description": "x" * 5000}),
        (["status=recurring", "due=1234567890", "rtype=occasional"],
         {"status": "recurring", "due": "1234567890", "rtype": "occasional"}),
        (["wait=wait", "scheduled=scheduled", "start=start", "due=due", "end=end",
          "until=until", "modified=modified"],
         {"wait": "wait", "scheduled": "scheduled", "start": "start", "due": "due",
          "end": "end", "until": "until", "modified": "modified"}),
        (["description=first", "description=second"], {"description": "second"}),
    ], ids=["value-with-equals", "empty-value", "empty-key", "unicode", "backslash-escape",
            "long-value", "recurring-invalid-rtype", "invalid-dates", "repeated-key"])
    def test_make_tc_task_values_verbatim(self, sb, record, args, expected):
        """Values are split at the first '=' and stored without any interpretation."""
        p = sb.make(args)
        uuid = p.stdout.strip()
        snap = read_replica(sb.db)
        record(p, "make_tc_task", ["<DATA>"] + [a[:80] for a in args],
               {"proc": observed_proc(p, sb), "replica": normalize_replica(snap, [uuid])}, sb)
        assert p.returncode == 0, p.stderr
        assert UUID_RE.match(uuid)
        assert snap["tasks"] == {uuid: expected}

    def test_make_tc_task_appends_to_existing_replica(self, sb, record):
        """Repeated calls append to the same replica; pending tasks get successive
        working-set ids, completed ones are not in the working set."""
        a = sb.make(["status=pending", "description=one"])
        b = sb.make(["status=completed", "description=two", "end=1234567899"])
        c = sb.make(["status=pending", "description=three"])
        uuids = [x.stdout.strip() for x in (a, b, c)]
        snap = read_replica(sb.db)
        record(c, "make_tc_task", ["<DATA>", "x3"],
               {"exits": [x.returncode for x in (a, b, c)],
                "replica": normalize_replica(snap, uuids)}, sb)
        assert [x.returncode for x in (a, b, c)] == [0, 0, 0]
        assert len(set(uuids)) == 3
        assert set(snap["tasks"]) == set(uuids)
        assert snap["working_set"] == [(1, uuids[0]), (2, uuids[2])]
        assert snap["tasks"][uuids[1]]["status"] == "completed"

    def test_make_tc_task_creates_missing_datadir(self, sb, record):
        """create_if_missing=true: a non-existent (nested) DATADIR is created."""
        d = os.path.join(sb.root, "new", "nested", "dir")
        p = sb.make(["description=x"], datadir=d)
        uuid = p.stdout.strip()
        snap = read_replica(os.path.join(d, "taskchampion.sqlite3"))
        record(p, "make_tc_task", ["<SANDBOX>/new/nested/dir", "description=x"],
               {"proc": observed_proc(p, sb), "replica": normalize_replica(snap, [uuid])}, sb)
        assert p.returncode == 0, p.stderr
        assert snap is not None
        assert snap["tasks"] == {uuid: {"description": "x"}}
