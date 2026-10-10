"""TDB2 replica opening from `task` (Context::initialize step 7.5 + TDB2::open_replica).

C++ reference: src/Context.cpp (create_if_missing = !rc.exit.on.missing.db;
read_write = (rc.gc && (needsGC || needsRecurUpdate)) || !readOnly), src/TDB2.cpp
open_replica, test/read-only.test.py (read-only commands leave the DB untouched),
test/feature.559.test.py (rc.exit.on.missing.db).

Note on "lazy" opening: C++ 3.5.0 opens the replica for *every* registered
command in Context::initialize (taskchampion's OnDisk storage creates the file
when create_if_missing is true), so `task version` / `_show` / `calc` DO create
an empty taskchampion.sqlite3 in a fresh data dir -- only `--version` (handled
before initialize) does not. The tests assert the C++ behaviour (the oracle).
"""

import os
import stat
import time

import pytest

from conftest import norm_text, normalize_replica, read_replica

NON_DATA = [
    ["--version"],
    ["version"],
    ["_version"],
    ["_show"],
    ["calc", "1+1"],
    ["config", "nonexistent.setting"],
]
NON_DATA_IDS = [" ".join(a) for a in NON_DATA]


def seed(sb):
    """Populate the replica with the side's own make_tc_task (pending + completed)."""
    a = sb.make(["status=pending", "description=one", "entry=1234567890"])
    b = sb.make(["status=completed", "description=two", "entry=1234567890", "end=1234567899"])
    assert a.returncode == 0 and b.returncode == 0, (a.stderr, b.stderr)
    return [a.stdout.strip(), b.stdout.strip()]


def age_db(sb):
    old = time.time() - 3600
    os.utime(sb.db, (old, old))
    return os.stat(sb.db).st_mtime


class TestReplicaCreation:
    @pytest.mark.parametrize("args", NON_DATA, ids=NON_DATA_IDS)
    def test_db_creation_on_fresh_datadir(self, sb, record, args):
        """Fresh empty data dir: which commands create taskchampion.sqlite3 (C++ parity)."""
        p = sb.task(args)
        exists = os.path.exists(sb.db)
        snap = normalize_replica(read_replica(sb.db))
        obs = {"exit": p.returncode, "db_exists": exists,
               "empty_replica": bool(snap) and snap["tasks"] == {} and snap["operations"] == [],
               "stderr": norm_text(p.stderr, sb.root)}
        record(p, "task", args, obs, sb)
        expect_db = args != ["--version"]
        assert exists == expect_db
        if exists:
            assert obs["empty_replica"]

    def test_exit_on_missing_db(self, sb, record):
        """rc.exit.on.missing.db=1 with no DB -> exit 2 + taskchampion error, nothing created."""
        args = ["rc.exit.on.missing.db=1", "version"]
        p = sb.task(args)
        exists = os.path.exists(sb.db)
        obs = {"exit": p.returncode, "stdout": norm_text(p.stdout), "db_exists": exists,
               "stderr": norm_text(p.stderr, sb.root)}
        record(p, "task", args, obs, sb)
        assert p.returncode == 2
        assert not exists
        assert "unable to open database file: %s" % sb.db in p.stderr

    def test_exit_on_missing_db_with_existing_db(self, sb, record):
        """rc.exit.on.missing.db=1 with an existing DB -> command runs normally."""
        seed(sb)
        args = ["rc.exit.on.missing.db=1", "_version"]
        p = sb.task(args)
        record(p, "task", args, {"exit": p.returncode, "stdout": norm_text(p.stdout),
                                 "stderr": norm_text(p.stderr, sb.root)}, sb)
        assert p.returncode == 0, p.stderr
        assert p.stdout.startswith("3.5.0")

    def test_data_location_is_a_file(self, sb, record):
        """rc.data.location pointing at a regular file -> exit 2 with the taskchampion error."""
        f = os.path.join(sb.root, "notadir")
        open(f, "w").close()
        args = ["rc.data.location=%s" % f, "_version"]
        p = sb.task(args)
        obs = {"exit": p.returncode, "stdout": norm_text(p.stdout),
               "stderr": norm_text(p.stderr, sb.root)}
        record(p, "task", args, obs, sb)
        assert p.returncode == 2
        assert 'Cannot create directory "%s"' % f in p.stderr

    def test_data_location_missing_dir_created(self, sb, record):
        """create_if_missing: a missing nested data.location is created with an empty replica."""
        d = os.path.join(sb.root, "fresh", "deeper")
        args = ["rc.data.location=%s" % d, "_version"]
        p = sb.task(args)
        exists = os.path.exists(os.path.join(d, "taskchampion.sqlite3"))
        record(p, "task", args, {"exit": p.returncode, "stdout": norm_text(p.stdout),
                                 "db_exists": exists,
                                 "stderr": norm_text(p.stderr, sb.root)}, sb)
        assert p.returncode == 0, p.stderr
        assert exists


class TestReadOnlyOpen:
    @pytest.mark.parametrize("args", NON_DATA[1:] + [["rc.gc=1", "version"]],
                             ids=NON_DATA_IDS[1:] + ["rc.gc=1 version"])
    def test_readonly_command_does_not_modify_db(self, sb, record, args):
        """read-only.test.py: read-only commands leave the replica mtime and contents untouched."""
        uuids = seed(sb)
        before = normalize_replica(read_replica(sb.db), uuids)
        mtime = age_db(sb)
        p = sb.task(args)
        after = normalize_replica(read_replica(sb.db), uuids)
        modified = os.stat(sb.db).st_mtime != mtime
        record(p, "task", args, {"exit": p.returncode, "modified": modified,
                                 "unchanged": before == after}, sb)
        assert not modified
        assert before == after

    @pytest.mark.parametrize("args", [["version"], ["_version"], ["_show"], ["calc", "2*3"]],
                             ids=["version", "_version", "_show", "calc 2*3"])
    def test_readonly_command_on_readonly_db(self, sb, record, args):
        """A 0444 DB in a 0555 dir: C++ opens it (even for read-only commands) through
        taskchampion's SQLite storage, which sets journal_mode=WAL and fails -> exit 2 with
        the SQLite error on stderr and nothing on stdout. The port must behave the same."""
        seed(sb)
        os.chmod(sb.db, stat.S_IRUSR | stat.S_IRGRP | stat.S_IROTH)
        os.chmod(sb.data, stat.S_IRUSR | stat.S_IXUSR | stat.S_IRGRP | stat.S_IXGRP)
        try:
            p = sb.task(args)
        finally:
            os.chmod(sb.data, stat.S_IRWXU)
            os.chmod(sb.db, stat.S_IRUSR | stat.S_IWUSR)
        record(p, "task", args, {"exit": p.returncode, "stdout": p.stdout,
                                 "stderr": norm_text(p.stderr, sb.root)}, sb)
        assert p.returncode == 2
        assert p.stdout == ""
        assert p.stderr.endswith("Setting journal_mode=WAL: attempt to write a readonly database: "
                                 "Error code 1544: attempt to write a readonly database\n")
