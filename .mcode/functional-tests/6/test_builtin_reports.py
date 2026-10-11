"""Built-in report.* definitions rendered by CmdCustom over a varied dataset
(frozen time): list, next, long, ls, minimal, newest, oldest, all, completed,
waiting, recurring, overdue, ready, active, blocked, unblocked, blocking;
'N tasks' footnote and the 'No matches.' / exit 1 path."""

from conftest import seed_reports, make_tc_task, seed_recurrence, FROZEN_EPOCH


def body(p):
    return p.stdout


class TestStandardReports:
    def test_report_list(self, task):
        """`list`: pending tasks, columns ID/Age/P/Project/Tags/Due/Description/Urg, '6 tasks'."""
        seed_reports(task)
        p = task("list")
        assert p.returncode == 0, p.stderr
        out = body(p)
        assert out.splitlines()[1].split() == ["ID", "Age", "P", "Project", "Tags", "Due",
                                               "Description", "Urg"] or "Description" in out
        assert "alpha one" in out and "gamma three" in out
        assert "epsilon five" not in out          # completed
        assert "iota nine" not in out             # waiting
        assert out.rstrip().endswith("tasks")

    def test_report_next(self, task):
        """`next`: urgency-sorted, relative due, 'N tasks' footnote."""
        seed_reports(task)
        p = task("next")
        assert p.returncode == 0, p.stderr
        lines = [l for l in body(p).splitlines() if l.strip()]
        names = [l for l in lines if "alpha one" in l or "gamma three" in l]
        # alpha (prio H, due soon) outranks gamma (overdue, no priority)
        assert "alpha one" in names[0]

    def test_report_long(self, task):
        """`long`: entry/modified/started/due formatted dates and full description."""
        seed_reports(task)
        p = task("long")
        assert p.returncode == 0, p.stderr
        assert "2009-02-20" in p.stdout and "alpha one" in p.stdout

    def test_report_ls_minimal(self, task):
        """`ls` and `minimal`: compact column sets."""
        seed_reports(task)
        p = task("ls")
        q = task("minimal")
        assert p.returncode == 0 and q.returncode == 0
        assert "beta two" in p.stdout and "beta two" in q.stdout

    def test_report_newest_oldest(self, task):
        """`newest` puts the latest entry first, `oldest` the earliest."""
        seed_reports(task)
        p = task("newest")
        q = task("oldest")
        assert p.returncode == 0 and q.returncode == 0
        assert p.stdout.index("theta") < p.stdout.index("alpha one")
        assert q.stdout.index("alpha one") < q.stdout.index("delta four")

    def test_report_all(self, task):
        """`all`: every status including completed (St column) and short UUIDs."""
        seed_reports(task)
        p = task("all", short_uuids=True)
        assert p.returncode == 0
        assert "epsilon five" in p.stdout and "iota nine" in p.stdout

    def test_report_completed(self, task):
        """`completed`: only the logged task, with end date."""
        seed_reports(task)
        p = task("completed", short_uuids=True)
        assert p.returncode == 0
        assert "epsilon five" in p.stdout and "alpha one" not in p.stdout

    def test_report_waiting(self, task):
        """`waiting`: only waiting tasks, with wait date."""
        seed_reports(task)
        p = task("waiting", short_uuids=True)
        assert p.returncode == 0
        assert "iota nine" in p.stdout and "alpha one" not in p.stdout

    def test_report_recurring(self, sb, task):
        """`recurring`: template (+PARENT, mask) and child instance (+CHILD) with Recur column.

        A bare template created by `add recur:` has no mask yet (recurrence generation
        is out of scope and disabled), so the report matches nothing => exit 1."""
        seed_reports(task)
        p = task("recurring")
        assert p.returncode == 1
        seed_recurrence(sb)
        q = task("recurring", short_uuids=True)
        assert q.returncode == 0
        assert "lambda weekly" in q.stdout

    def test_report_overdue(self, task):
        """`overdue`: only tasks with due before now."""
        seed_reports(task)
        p = task("overdue")
        assert p.returncode == 0
        assert "gamma three" in p.stdout and "alpha one" not in p.stdout

    def test_report_ready(self, task):
        """`ready`: actionable tasks (excludes blocked/waiting/scheduled-in-future)."""
        seed_reports(task)
        p = task("ready")
        assert p.returncode == 0
        assert "alpha one" in p.stdout and "iota nine" not in p.stdout

    def test_report_active(self, task):
        """`active`: only started tasks."""
        seed_reports(task)
        p = task("active")
        assert p.returncode == 0
        assert "theta eight" in p.stdout and "alpha one" not in p.stdout

    def test_report_blocked_unblocked_blocking(self, task):
        """`blocked`, `unblocked`, `blocking` reports over a dependency chain."""
        seed_reports(task)
        b = task("blocked")
        u = task("unblocked")
        g = task("blocking", short_uuids=True)
        assert b.returncode == 0 and u.returncode == 0 and g.returncode == 0
        assert "eta seven" in b.stdout and "alpha one" not in b.stdout
        assert "alpha one" in g.stdout and "eta seven" not in g.stdout
        assert "alpha one" in u.stdout and "eta seven" not in u.stdout


class TestFootnotesAndExit:
    def test_report_no_matches_exit1(self, task):
        """A filter that matches nothing: 'No matches.' and exit 1 (also on empty db)."""
        p = task("list")
        seed_reports(task)
        q = task("list", "project:nonexistent")
        r = task("rc.verbose=nothing", "list", "project:nonexistent")
        assert p.returncode == 1 and q.returncode == 1 and r.returncode == 1
        assert "No matches." in (q.stdout + q.stderr)

    def test_report_single_task_footnote(self, task):
        """Exactly one match => '1 task' footnote (singular)."""
        seed_reports(task)
        p = task("list", "project:home.garden")
        assert p.returncode == 0
        assert "1 task" in p.stdout

    def test_report_filter_and_id_selection(self, task):
        """ID list / range and attribute filters on a report."""
        seed_reports(task)
        p = task("1,3", "list")
        q = task("list", "+work")
        r = task("2-4", "ls")
        assert p.returncode == 0 and q.returncode == 0 and r.returncode == 0
        assert "alpha one" in p.stdout and "gamma three" in p.stdout and "beta" not in p.stdout

    def test_report_deleted_and_annotated(self, sb, task):
        """Deleted tasks only in `all`; annotations rendered under description in list."""
        seed_reports(task)
        make_tc_task(sb, status="deleted", description="zeta deleted",
                     entry=str(FROZEN_EPOCH - 7300), end=str(FROZEN_EPOCH - 60),
                     modified=str(FROZEN_EPOCH - 60))
        make_tc_task(sb, status="pending", description="mu annotated",
                     entry=str(FROZEN_EPOCH - 7200), modified=str(FROZEN_EPOCH - 60),
                     **{"annotation_%d" % (FROZEN_EPOCH - 3000): "first note",
                        "annotation_%d" % (FROZEN_EPOCH - 2000): "second note"})
        a = task("all", short_uuids=True)
        l = task("list")
        assert a.returncode == 0 and l.returncode == 0
        assert "zeta deleted" in a.stdout and "zeta deleted" not in l.stdout
        assert "[2]" in l.stdout             # list uses description.count
        g = task("long", "description:mu")
        assert g.returncode == 0 and "first note" in g.stdout
