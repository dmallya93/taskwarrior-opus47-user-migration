"""`info` (attribute table, urgency breakdown, journal), `_urgency`,
`columns` / `_columns`, and `reports`."""

from conftest import seed_reports, make_tc_task, FROZEN_EPOCH


class TestInfo:
    def test_info_pending_task(self, task):
        """info on a pending task: attributes, virtual tags, urgency breakdown, journal."""
        seed_reports(task)
        p = task("1", "info")
        assert p.returncode == 0, p.stderr
        assert "Description" in p.stdout and "alpha one" in p.stdout
        assert "Urgency" in p.stdout

    def test_info_dependency_and_active(self, task):
        """info on blocked (eta) and active (theta) tasks shows deps / start."""
        seed_reports(task)
        p = task("5", "info")
        q = task("6", "info")
        assert p.returncode == 0 and q.returncode == 0
        assert "alpha one" in p.stdout     # 'This task blocked by'

    def test_info_completed_and_annotations(self, sb, task):
        """info on a completed task (by uuid) and an annotated task."""
        seed_reports(task)
        make_tc_task(sb, status="pending", description="mu annotated",
                     entry=str(FROZEN_EPOCH - 7200), modified=str(FROZEN_EPOCH - 60),
                     **{"annotation_%d" % (FROZEN_EPOCH - 3000): "first note"})
        p = task("status:completed", "info")
        q = task("description:mu", "info")
        assert p.returncode == 0 and q.returncode == 0
        assert "first note" in q.stdout

    def test_info_multiple_and_nomatch(self, task):
        """info with several ids; info that matches nothing => exit 1."""
        seed_reports(task)
        p = task("1-2", "info")
        q = task("99", "info")
        assert p.returncode == 0
        assert q.returncode == 1

    def test_info_verbose_nothing(self, task):
        """rc.verbose=nothing info output (no journal, no urgency details)."""
        seed_reports(task)
        p = task("rc.verbose=nothing", "3", "info")
        assert p.returncode == 0

    def test_urgency_helper(self, task):
        """`_urgency` prints 'task <id> urgency <value>' per filtered task."""
        seed_reports(task)
        p = task("_urgency", unordered=True)     # replica (UUID) order is unspecified
        q = task("1", "_urgency")
        assert p.returncode == 0 and q.returncode == 0
        assert "urgency" in q.stdout


class TestIntrospection:
    def test_columns_cmd(self, task):
        """`columns` lists every column with styles and examples."""
        p = task("columns")
        q = task("columns", "desc")
        assert p.returncode == 0 and q.returncode == 0
        assert "description" in p.stdout and "combined" in p.stdout

    def test_columns_with_uda(self, sb, task):
        """`columns` includes configured UDAs."""
        sb.append_rc("uda.size.type=string\nuda.size.label=Size\n")
        p = task("columns")
        assert p.returncode == 0
        assert "size" in p.stdout

    def test_completion_columns(self, task):
        """`_columns` prints the bare column names."""
        p = task("_columns")
        assert p.returncode == 0
        assert "description" in p.stdout.split()

    def test_reports_cmd(self, task):
        """`reports` lists every report with description and a count footnote."""
        p = task("reports")
        q = task("rc.report.foo.description=My report", "rc.report.foo.columns=id",
                 "reports")
        assert p.returncode == 0 and q.returncode == 0
        assert "My report" in q.stdout
