"""CLI2 / Task::modify edge cases: ambiguous attribute abbreviations, numeric
descriptions, reserved virtual tags, UDA type validation, named dates,
rc.dateformat input, depends/wait virtual tags, alias chains, `--` placement."""

import json


def ids(task, *filt, **kw):
    p = task("rc.verbose=nothing", *filt, "_ids", **kw)
    assert p.returncode == 0, p.stderr
    return [int(x) for x in p.stdout.split()]


class TestModifyEdgeCases:
    def test_ambiguous_attribute_abbreviation_is_text(self, task):
        """`d:` / `de:` are ambiguous (depends/description/due) -> kept as description text."""
        p = task("add", "three", "d:2009-03-01")
        q = task("add", "four", "de:2009-03-01")
        assert p.returncode == 0 and q.returncode == 0
        r = task("_get", "1.description", "1.due", "2.description", "2.due")
        assert r.stdout == "three d:2009-03-01  four de:2009-03-01 \n"

    def test_numeric_descriptions(self, task):
        """`add 13` / `add 1.5` create tasks with numeric descriptions (not ID filters)."""
        assert task("add", "13").stdout == "Created task 1.\n"
        assert task("add", "1.5").stdout == "Created task 2.\n"
        assert task("_get", "1.description", "2.description").stdout == "13 1.5\n"

    def test_reserved_and_empty(self, task):
        """Virtual tags cannot be added; empty description -> 'Additional text must be provided.'"""
        p = task("add", "twelve", "+PENDING")
        assert p.returncode == 2
        assert ("Virtual tags (including 'PENDING') are reserved and may not be added or "
                "removed.") in p.stderr
        q = task("add", "")
        assert q.returncode == 2 and "Additional text must be provided." in q.stderr
        assert task("count", record=False).stdout == "0\n"

    def test_uda_numeric(self, task):
        """Numeric UDA: valid value stored, invalid rejected, .above:/equality filters."""
        uda = "rc.uda.size.type=numeric"
        assert task(uda, "add", "sized", "size:5").returncode == 0
        p = task(uda, "add", "sized2", "size:abc")
        assert p.returncode == 2
        assert "The value 'abc' is not a valid numeric value." in p.stderr
        assert ids(task, uda, "size.above:3") == [1]
        assert ids(task, uda, "size:5") == [1]
        assert ids(task, uda, "size.below:3") == []
        data = json.loads(task(uda, "rc.verbose=nothing", "export").stdout)
        assert data[0]["size"] == 5

    def test_named_dates_and_dateformat(self, task):
        """due:eom, scheduled:sow, due:tomorrow+2d, rc.dateformat input parsing."""
        task("add", "one", "due:eom")
        task("add", "two", "scheduled:sow")
        task("add", "nine", "due:tomorrow+2d")
        task("add", "ten", "rc.dateformat=d/m/Y", "due:01/03/2009")
        p = task("_get", "1.due", "2.scheduled", "3.due", "4.due")
        assert p.stdout == ("2009-02-28T23:59:59 2009-02-09T00:00:00 2009-02-16T00:00:00 "
                            "2009-03-01T00:00:00\n")
        q = task("calc", "4.due - 4.entry")
        assert q.returncode == 0

    def test_depends_and_wait_virtual_tags(self, task):
        """depends -> +BLOCKED/+BLOCKING; wait -> status waiting, +WAITING; +READY."""
        task("add", "a")
        task("add", "b", "wait:2009-02-14", "+x")
        task("add", "c", "depends:1,2")
        assert task("_get", "2.status", "3.depends").returncode == 0
        assert ids(task, "+WAITING") == [2]
        assert ids(task, "+BLOCKED") == [3]
        assert ids(task, "+BLOCKING") == [1, 2]
        assert task("rc.verbose=nothing", "+READY", "count").stdout == "1\n"
        assert task("rc.verbose=nothing", "status:waiting", "count").stdout == "1\n"

    def test_completed_add_shows_uuid(self, task):
        """add status:completed -> feedback shows the UUID (id is 0)."""
        p = task("add", "eleven", "status:completed")
        u = task("_uuids", record=False).stdout.strip()
        assert p.stdout == "Created task %s.\n" % u


class TestParserEdgeCases:
    def test_alias_chain(self, task):
        """Aliases expand recursively (a1 -> a2 -> count)."""
        task("rc.verbose=nothing", "add", "x", record=False)
        p = task("rc.alias.a1=a2", "rc.alias.a2=count", "a1")
        assert p.returncode == 0 and p.stdout == "1\n"

    def test_terminator_positions(self, task):
        """`-- count` and `count -- +y`; `add -- -dash` then /-dash/ pattern."""
        task("rc.verbose=nothing", "add", "x", "+y", record=False)
        a = task("--", "count")
        b = task("count", "--", "+y")
        assert a.returncode == b.returncode
        p = task("rc.verbose=nothing", "add", "--", "-dash")
        assert p.returncode == 0
        assert task("_get", "2.description").stdout == "-dash\n"
        task("_ids", "/-dash/")
