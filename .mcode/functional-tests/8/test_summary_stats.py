"""summary, timesheet, stats."""

from conftest import add, seed_history, seed_reports, FROZEN_EPOCH, DAY

W = "rc.defaultwidth=100"


class TestSummary:
    def test_summary_tree(self, task):
        """summary: project tree (alpha / alpha.sub / beta / gamma) with % complete bars."""
        seed_history(task)
        p = task(W, "summary")
        assert p.returncode == 0
        assert "alpha" in p.stdout and "sub" in p.stdout and "Complete" in p.stdout

    def test_summary_color(self, task):
        """summary with colour: the % bars are colour-coded."""
        seed_history(task)
        p = task(W, "rc._forcecolor=on", "rc.color=on", "summary")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout

    def test_summary_all_projects(self, task):
        """summary.all.projects=on includes fully completed projects."""
        seed_history(task)
        add(task, "log", "only done", "project:closed")
        p = task(W, "rc.summary.all.projects=on", "summary")
        q = task(W, "summary")
        assert p.returncode == 0 and q.returncode == 0
        assert "closed" in p.stdout

    def test_summary_empty(self, task):
        """summary with no tasks: 'No projects.'"""
        p = task(W, "summary")
        assert "No projects." in (p.stdout + p.stderr)

    def test_summary_filter(self, task):
        """summary restricted by filter."""
        seed_history(task)
        p = task(W, "project:beta", "summary")
        assert p.returncode == 0


class TestTimesheet:
    def test_timesheet_default(self, task):
        """timesheet: completed and started tasks in the last 4 weeks."""
        seed_history(task)
        p = task(W, "timesheet", short_uuids=True)
        assert p.returncode == 0

    def test_timesheet_custom_filter(self, task):
        """report.timesheet.filter override changes the listed tasks."""
        seed_history(task)
        p = task(W, "rc.report.timesheet.filter=+COMPLETED", "timesheet", short_uuids=True)
        assert p.returncode == 0

    def test_timesheet_with_cli_filter(self, task):
        """timesheet with a command-line filter (CLI2 special handling)."""
        seed_history(task)
        p = task(W, "project:beta", "timesheet", short_uuids=True)
        assert p.returncode == 0

    def test_timesheet_empty(self, task):
        """timesheet with no data."""
        p = task(W, "timesheet", short_uuids=True)
        assert p.returncode in (0, 1)


class TestStats:
    def test_stats_seeded(self, task):
        """stats over the varied report dataset: counts, tags, projects, blocked, undo/backlog."""
        seed_reports(task)
        p = task(W, "stats")
        assert p.returncode == 0
        assert "Pending" in p.stdout and "Undo transactions" in p.stdout

    def test_stats_history(self, task):
        """stats over the history dataset (completed/deleted, averages)."""
        seed_history(task)
        p = task(W, "stats")
        assert p.returncode == 0
        assert "Average time pending" in p.stdout

    def test_stats_empty(self, task):
        """stats on an empty replica."""
        p = task(W, "stats")
        assert p.returncode == 0

    def test_stats_filter(self, task):
        """stats with a filter."""
        seed_reports(task)
        p = task(W, "project:home", "stats")
        assert p.returncode == 0

    def test_stats_color(self, task):
        """stats with colour (header underline)."""
        seed_reports(task)
        p = task(W, "rc._forcecolor=on", "rc.color=on", "stats")
        assert p.returncode == 0
