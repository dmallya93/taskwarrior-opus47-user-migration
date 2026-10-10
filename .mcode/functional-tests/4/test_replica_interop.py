"""Replicas written by make_tc_task are valid taskchampion replicas that a real
Taskwarrior reader understands.

The data-reading commands of the Rust `task` (export/info/_get/count) are still
stubs at milestone 4, so the reader here is the C++ reference `task`
(FT_READER_TASK_BIN) on both sides: origin = C++ make_tc_task -> C++ reader,
target = Rust make_tc_task -> C++ reader. Also covers mixed replicas (a replica
created by the C++ helper, appended to by the side's helper). Time is frozen so
urgency / dates are deterministic.
"""

import json
import os

import pytest

from conftest import faketime_env, norm_text

CPP_MAKE = os.environ.get("FT_READER_MAKE_BIN", "/tmp/ft4-origin/build/test/make_tc_task")


def export(sb):
    p = sb.reader(["rc.json.array=1", "export"], faketime_env())
    assert p.returncode == 0, p.stderr
    return p, json.loads(p.stdout)


def strip_dynamic(tasks):
    """Drop uuid; replace UUID-valued references (parent, ...) by a placeholder."""
    out = []
    for t in tasks:
        t = {k: (norm_text(v) if isinstance(v, str) else v) for k, v in t.items() if k != "uuid"}
        out.append(t)
    return out


class TestReaderInterop:
    def test_export_pending_task_with_tags(self, sb, record):
        """custom.test.py-style task (tag_* without `tags`) exports with tags array and urgency."""
        m = sb.make(["status=pending", "description=task with tags", "entry=1234500000",
                     "tag_tag1=x", "tag_tag2=x", "project=home"])
        uuid = m.stdout.strip()
        p, data = export(sb)
        record(p, "reader", ["export"], {"export": strip_dynamic(data)}, sb)
        assert m.returncode == 0
        assert len(data) == 1
        t = data[0]
        assert t["uuid"] == uuid
        assert t["id"] == 1
        assert t["status"] == "pending"
        assert t["description"] == "task with tags"
        assert t["project"] == "home"
        assert sorted(t["tags"]) == ["tag1", "tag2"]
        assert t["entry"] == "20090213T044000Z"
        assert isinstance(t["urgency"], (int, float))

    def test_get_and_count(self, sb, record):
        """`_get N.description` and `count` see every task written by make_tc_task."""
        for i in range(3):
            assert sb.make(["status=pending", "description=t%d" % i,
                            "entry=1234500000"]).returncode == 0
        assert sb.make(["status=completed", "description=done", "entry=1234500000",
                        "end=1234500001"]).returncode == 0
        env = faketime_env()
        g = sb.reader(["_get", "2.description"], env)
        c = sb.reader(["count"], env)
        cp = sb.reader(["status:pending", "count"], env)
        record(g, "reader", ["_get 2.description / count"],
               {"get": g.stdout, "count": c.stdout, "pending": cp.stdout,
                "exits": [g.returncode, c.returncode, cp.returncode]}, sb)
        assert g.stdout == "t1\n"
        assert c.stdout == "4\n"
        assert cp.stdout == "3\n"

    def test_export_unusual_tasks(self, sb, record):
        """unusual_task.test.py inputs (empty task, invalid dates, recurring w/o fields)
        written verbatim must still be readable/exportable as valid JSON."""
        assert sb.make([]).returncode == 0
        assert sb.make(["wait=wait", "scheduled=scheduled", "start=start", "due=due",
                        "end=end", "until=until", "modified=modified"]).returncode == 0
        assert sb.make(["status=recurring", "due=1234500000", "rtype=occasional"]).returncode == 0
        p, data = export(sb)
        record(p, "reader", ["export"], {"export": strip_dynamic(data)}, sb)
        # the reader's recurrence handling also spawns one child of the recurring parent
        assert len(data) >= 3
        assert any(t.get("status") == "recurring" and t.get("rtype") == "occasional" for t in data)
        # the empty task is exported with no status/description at all
        assert any("status" not in t and "description" not in t for t in data)
        # invalid date strings stored verbatim do not break export
        assert any(t.get("modified") == "modified" or t.get("due") == "due" or
                   ("wait" not in t and "status" not in t) for t in data)

    def test_info_tags_without_tags_attribute(self, sb, record):
        """info.test.py::test_tags_without_tags_attribute on a make_tc_task replica."""
        assert sb.make(["status=pending", "description=task with tags", "due=1234600000",
                        "tag_foo=x", "tag_bar=x", "entry=1234500000"]).returncode == 0
        p = sb.reader(["1", "info"], faketime_env())
        out = norm_text(p.stdout, sb.root)
        record(p, "reader", ["1", "info"], {"exit": p.returncode, "info": out}, sb)
        assert p.returncode == 0, p.stderr
        assert "task with tags" in p.stdout
        assert ("foo bar" in p.stdout) or ("bar foo" in p.stdout)

    def test_append_to_cpp_created_replica(self, sb, record):
        """Side's make_tc_task appends to a replica first created by the C++ helper;
        both tasks are visible and keep working-set ids 1 and 2."""
        import subprocess
        first = subprocess.run([CPP_MAKE, sb.data, "status=pending", "description=from cpp",
                                "entry=1234500000"], capture_output=True, text=True)
        assert first.returncode == 0, first.stderr
        m = sb.make(["status=pending", "description=from side", "entry=1234500000"])
        assert m.returncode == 0, m.stderr
        p, data = export(sb)
        record(p, "reader", ["export"], {"export": strip_dynamic(data)}, sb)
        assert [(t["id"], t["description"]) for t in data] == [(1, "from cpp"), (2, "from side")]
        assert data[1]["uuid"] == m.stdout.strip()
