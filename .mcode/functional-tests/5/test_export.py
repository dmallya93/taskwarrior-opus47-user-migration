"""export command: JSON format, rc.json.array on/off, filters, limit:N,
report-based `export <report>` (report filter + sort), errors."""

import json

from conftest import seed_basic


class TestExport:
    def test_export_json_array_format(self, task):
        """Default (json.array=on): '[' line, one task per line joined by ',\\n', ']' line."""
        seed_basic(task)
        p = task("rc.verbose=nothing", "export")
        assert p.returncode == 0, p.stderr
        lines = p.stdout.split("\n")
        assert lines[0] == "["
        assert lines[-2] == "]" and lines[-1] == ""
        body = lines[1:-2]
        assert len(body) == 5
        assert all(l.endswith(",") for l in body[:-1])
        data = json.loads(p.stdout)
        # sorted by id,uuid: completed task (id 0) first, then 1..4
        assert [t["id"] for t in data] == [0, 1, 2, 3, 4]
        t1 = data[1]
        assert t1["description"] == "alpha one"
        assert t1["project"] == "home"
        assert t1["priority"] == "H"
        assert t1["tags"] == ["work"]
        assert t1["due"] == "20090220T000000Z"
        assert t1["entry"] == "20090213T120000Z"
        assert t1["modified"] == "20090213T120000Z"
        assert isinstance(t1["urgency"], (int, float))
        assert data[0]["status"] == "completed" and data[0]["end"] == "20090213T120000Z"

    def test_export_json_array_off(self, task):
        """rc.json.array=off: one JSON object per line, no brackets, no commas."""
        seed_basic(task)
        p = task("rc.verbose=nothing", "rc.json.array=off", "export")
        assert p.returncode == 0
        lines = p.stdout.rstrip("\n").split("\n")
        assert len(lines) == 5
        for l in lines:
            assert l.startswith("{") and l.endswith("}")
            json.loads(l)

    def test_export_empty(self, task):
        """No tasks: '[\\n]\\n' with json.array, empty output without."""
        p = task("rc.verbose=nothing", "export")
        q = task("rc.verbose=nothing", "rc.json.array=off", "export")
        assert p.returncode == 0 and p.stdout == "[\n]\n"
        assert q.returncode == 0 and q.stdout == ""

    def test_export_with_filter(self, task):
        """Filters before the command select tasks."""
        seed_basic(task)
        p = task("rc.verbose=nothing", "project:home", "export")
        data = json.loads(p.stdout)
        # project:home matches home and home.garden (left-match), incl. completed task
        assert sorted(t["description"] for t in data) == ["alpha one", "beta two", "epsilon five"]
        q = task("rc.verbose=nothing", "+work", "export")
        assert [t["description"] for t in json.loads(q.stdout)] == ["alpha one"]
        r = task("rc.verbose=nothing", "2", "export")
        assert [t["id"] for t in json.loads(r.stdout)] == [2]

    def test_export_limit(self, task):
        """limit:N handling in export (C++ semantics)."""
        seed_basic(task)
        # limit:N is consumed by getLimits; with rc.json.array the C++ export of a
        # filter-position limit yields an empty array; after the command it is taken as a
        # report name.
        p = task("rc.verbose=nothing", "limit:2", "export")
        assert p.returncode == 0 and p.stdout == "[\n]\n"
        q = task("rc.verbose=nothing", "export", "limit:2")
        assert q.returncode != 0
        assert "Unable to find report that matches 'limit:2'." in q.stderr

    def test_export_report(self, task):
        """`export <report>` applies the report's filter and sort (next: pending, urgency-)."""
        seed_basic(task)
        p = task("rc.verbose=nothing", "export", "next")
        assert p.returncode == 0, p.stderr
        data = json.loads(p.stdout)
        assert all(t["status"] == "pending" for t in data)
        urg = [t["urgency"] for t in data]
        assert urg == sorted(urg, reverse=True)
        assert len(data) == 4
        q = task("rc.verbose=nothing", "export", "completed")
        assert [t["description"] for t in json.loads(q.stdout)] == ["epsilon five"]
        # abbreviated report name
        r = task("rc.verbose=nothing", "export", "comple")
        assert r.stdout == q.stdout

    def test_export_unknown_report(self, task):
        """Unknown report name -> exact C++ error, non-zero exit."""
        seed_basic(task)
        p = task("rc.verbose=nothing", "export", "nosuchreport")
        assert p.returncode != 0
        assert "Unable to find report that matches 'nosuchreport'." in p.stderr

    def test_export_depends_array(self, task):
        """depends is exported as a JSON array of UUIDs (json.depends.array default on)."""
        task("rc.verbose=nothing", "add", "first", record=False)
        p = task("rc.verbose=nothing", "add", "second", "depends:1")
        assert p.returncode == 0, p.stderr
        u1 = task("_get", "1.uuid", record=False).stdout.strip()
        data = json.loads(task("rc.verbose=nothing", "2", "export").stdout)
        assert data[0]["depends"] == [u1]
