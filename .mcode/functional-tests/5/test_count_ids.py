"""count, ids, uuids, _ids, _uuids, _zshids, _zshuuids, _unique, _zshattributes."""

import re

from conftest import seed_basic

UUID = r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"


class TestCount:
    def test_count(self, task):
        """count prints the number of matching tasks (all statuses), exit 0 even for 0."""
        p = task("count")
        assert p.returncode == 0 and p.stdout == "0\n"
        seed_basic(task)
        assert task("count").stdout == "5\n"
        assert task("status:pending", "count").stdout == "4\n"
        q = task("project:nothere", "count")
        assert q.returncode == 0 and q.stdout == "0\n"


class TestIds:
    def test_ids(self, task):
        """`ids` prints compressed ranges of pending ids (filtered)."""
        seed_basic(task)
        p = task("ids")
        assert p.returncode == 0 and p.stdout == "1-4\n"
        assert task("1,2,4", "ids").stdout == "1-2 4\n"
        assert task("project:home", "ids").stdout == "1-2\n"
        assert task("project:none", "ids").stdout == "\n"

    def test_underscore_ids(self, task):
        """`_ids` one id per line; `_zshids` id:description lines."""
        seed_basic(task)
        assert task("_ids").stdout == "1\n2\n3\n4\n"
        assert task("+work", "_ids").stdout == "1\n"
        # output order follows the filter (TDB2) order, which is unspecified in C++
        p = task("_zshids", unordered=True)
        assert p.returncode == 0
        assert sorted(p.stdout.splitlines()) == ["1:alpha one", "2:beta two", "3:gamma three",
                                                 "4:delta four"]

    def test_zshids_colon(self, task):
        """`_zshids` replaces ':' in descriptions with ','."""
        task("rc.verbose=nothing", "add", "a", "--", "x:y", record=False)
        p = task("_zshids")
        assert p.stdout == "1:a x,y\n"

    def test_uuids(self, task):
        """uuids space separated (sorted), _uuids one per line, _zshuuids uuid:description."""
        seed_basic(task)
        p = task("uuids", unordered=True)
        assert p.returncode == 0
        lst = p.stdout.split()
        assert len(lst) == 5 and lst == sorted(lst)
        assert all(re.fullmatch(UUID, u) for u in lst)
        q = task("_uuids", unordered=True)
        assert q.stdout.split("\n")[:-1] == sorted(q.stdout.split())
        r = task("project:work", "_zshuuids")
        assert re.fullmatch(UUID + r":gamma three\n", r.stdout)
        s = task("+work", "uuids")
        assert s.stdout == task("_get", "1.uuid", record=False).stdout


class TestUnique:
    def test_unique(self, task):
        """`_unique <attr>` distinct sorted values; filters apply; bad attribute errors."""
        seed_basic(task)
        p = task("_unique", "project")
        assert p.returncode == 0
        assert p.stdout == "home\nhome.garden\nwork\n"
        assert task("_unique", "priority").stdout == "H\n"
        assert task("+home", "_unique", "project").stdout == "home.garden\n"
        assert task("_unique", "proj").stdout == p.stdout
        assert task("_unique", "status").stdout == "completed\npending\n"
        q = task("_unique", "nosuchattr")
        assert q.returncode != 0
        r = task("_unique")
        assert r.returncode != 0


class TestZshAttributes:
    def test_zshattributes(self, task):
        """`_zshattributes` lists every column as name:name, sorted, incl. UDAs."""
        p = task("_zshattributes")
        assert p.returncode == 0
        lines = p.stdout.rstrip("\n").split("\n")
        assert "project:project" in lines and "due:due" in lines
        assert lines == sorted(lines)
        q = task("rc.uda.ftrun_size.type=numeric", "_zshattributes")
        assert "ftrun_size:ftrun_size" in q.stdout.split("\n")
