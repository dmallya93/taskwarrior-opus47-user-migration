"""DOM references via `_get` (rc.*, tw.*, context.*, system.*, task-relative
id/uuid references, date elements, tags, annotations) and error handling."""

import os
import platform
import subprocess

from conftest import TARGET_REPO, seed_basic

MAKE_BIN = os.environ.get("FT_MAKE_TC_TASK_BIN",
                          os.path.join(TARGET_REPO, "target/release/make_tc_task"))


def get(task, *refs, **kw):
    return task("_get", *refs, **kw)


class TestDomGet:
    def test_get_rc_values(self, task):
        """`_get rc.X` returns config values, including rc. overrides; unknown -> empty."""
        p = get(task, "rc.abbreviation.minimum", "rc.confirmation", "rc.json.array")
        assert p.returncode == 0
        assert p.stdout == "2 off 1\n"
        q = task("rc.foo.bar=baz", "_get", "rc.foo.bar")
        assert q.stdout == "baz\n"
        r = get(task, "rc.no.such.setting")
        assert r.returncode == 0 and r.stdout == "\n"

    def test_get_task_attributes(self, task):
        """`_get N.attr` for description/project/priority/status/tags/id/urgency."""
        seed_basic(task)
        p = get(task, "1.description", "1.project", "1.priority", "1.status", "1.tags", "1.id")
        assert p.returncode == 0
        assert p.stdout == "alpha one home H pending work 1\n"
        q = get(task, "2.project", "4.project", "2.tags")
        assert q.stdout == "home.garden  home\n"
        r = get(task, "1.urgency")
        assert r.returncode == 0 and float(r.stdout) > 0

    def test_get_by_uuid(self, task):
        """`_get <uuid>.attr` and `<uuid>.id`."""
        seed_basic(task)
        u = get(task, "3.uuid", record=False).stdout.strip()
        p = get(task, u + ".description", u + ".project", u + ".id")
        assert p.stdout == "gamma three work 3\n"

    def test_get_date_elements(self, task):
        """`N.due` (ISO in UTC) and date elements year/month/day/weekday/julian/hour."""
        seed_basic(task)
        p = get(task, "1.due")
        assert p.stdout.strip() == "2009-02-20T00:00:00"
        q = get(task, "1.due.year", "1.due.month", "1.due.day", "1.due.week",
                "1.due.weekday", "1.due.julian", "1.due.hour", "1.due.minute", "1.due.second")
        assert q.returncode == 0
        assert q.stdout == "2009 2 20 7 5 51 0 0 0\n"
        r = get(task, "1.entry.year", "4.due", "4.due.year")
        assert r.stdout.startswith("2009 ")

    def test_get_tags_n(self, task):
        """`N.tags.<tag>` returns the tag when present, else empty."""
        seed_basic(task)
        p = get(task, "1.tags.work", "1.tags.home", "2.tags.home")
        assert p.stdout == "work  home\n"

    def test_get_annotations(self, task, sb):
        """annotations.count / annotations.N.description / .entry (task with two
        annotations written into the replica by make_tc_task)."""
        task("rc.verbose=nothing", "add", "plain", record=False)
        p = get(task, "1.annotations.count")
        assert p.returncode == 0 and p.stdout == "0\n"
        m = subprocess.run(["make_tc_task", sb.data, "status=pending", "description=annotated",
                            "entry=1234500000", "annotation_1234500100=first note",
                            "annotation_1234500200=second note"],
                           executable=MAKE_BIN, capture_output=True, text=True, env=sb.env())
        assert m.returncode == 0, m.stderr
        q = get(task, "2.annotations.count", "2.annotations.1.description",
                "2.annotations.2.description")
        assert q.returncode == 0
        assert q.stdout == "2 first note second note\n"
        r = get(task, "2.annotations.1.entry", "2.annotations.1.entry.year")
        assert r.returncode == 0
        assert r.stdout == "2009-02-13T04:41:40 2009\n"

    def test_get_system_tw_context(self, task):
        """tw.version, system.os, context.program, tw.program, context.args."""
        p = get(task, "tw.version")
        assert p.stdout == "3.5.0\n"
        q = get(task, "system.os")
        assert q.returncode == 0
        if platform.system() == "Linux":
            assert q.stdout == "Linux\n"
        r = get(task, "context.program", "tw.program")
        assert r.returncode == 0
        assert r.stdout.split()[0].endswith("task")
        s = get(task, "context.args")
        assert s.returncode == 0 and "_get" in s.stdout

    def test_get_missing_task(self, task):
        """Reference to a non-existent task id yields an empty value."""
        seed_basic(task)
        p = get(task, "99.description")
        assert p.returncode == 0 and p.stdout == "\n"

    def test_get_errors(self, task):
        """Non-DOM word -> "'x' is not a DOM reference."; no args -> 'No DOM reference specified.'"""
        p = get(task, "notadomref")
        assert p.returncode != 0
        assert "'notadomref' is not a DOM reference." in p.stderr
        q = get(task)
        assert q.returncode != 0
        assert "No DOM reference specified." in q.stderr

    def test_calc_dom(self, task):
        """`task calc` resolves DOM references (rc.*, tw.version)."""
        seed_basic(task)
        # the config value is a string, so '+' concatenates ("2" + 1 -> "21")
        p = task("calc", "rc.abbreviation.minimum + 1")
        assert p.returncode == 0
        assert p.stdout.strip() == "21"
        q = task("calc", "1.due + 1d")
        assert q.returncode == 0
        task("calc", "1.priority")
