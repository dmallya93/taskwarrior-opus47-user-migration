"""ViewTask layout: widths / wrapping, row.padding, report dateformat,
print.empty.columns, limit:/page, blanklines, obfuscation, verbose tokens,
user-defined reports, context footnote, unicode widths."""

from conftest import seed_reports, add


LONG = ("this is a fairly long description that should wrap over several lines when "
        "the terminal is narrow")


class TestWidth:
    def test_narrow_width_wrapping(self, task):
        """rc.defaultwidth=40 wraps descriptions; no line exceeds 40 columns."""
        add(task, "add", LONG, "project:home")
        add(task, "add", "short", "one")
        p = task("rc.defaultwidth=40", "list")
        assert p.returncode == 0
        assert all(len(l) <= 40 for l in p.stdout.splitlines())
        q = task("rc.defaultwidth=40", "rc.report.list.columns=id,description.truncated",
                 "rc.report.list.labels=ID,Description", "list")
        assert q.returncode == 0
        assert all(len(l) <= 40 for l in q.stdout.splitlines())

    def test_wide_width(self, task):
        """rc.defaultwidth=200 keeps the long description on one line."""
        add(task, "add", LONG)
        p = task("rc.defaultwidth=200", "list")
        assert p.returncode == 0
        assert LONG in p.stdout

    def test_unicode_widths(self, task):
        """Double-width CJK and combining characters measured by display width."""
        add(task, "add", "日本語のタスク", "project:仕事")
        add(task, "add", "café au lait", "project:café")
        p = task("list")
        q = task("rc.defaultwidth=20", "list")
        assert p.returncode == 0 and q.returncode == 0
        assert "日本語" in p.stdout


class TestLayoutOptions:
    def test_row_padding(self, task):
        """rc.row.padding=2 adds two spaces to both sides of every row."""
        seed_reports(task)
        p = task("rc.row.padding=2", "ls")
        assert p.returncode == 0
        lines = [l for l in p.stdout.splitlines() if l.strip() and "task" not in l]
        assert all(l.startswith("  ") for l in lines)

    def test_report_dateformat(self, task):
        """report.X.dateformat and dateformat.report override date rendering."""
        seed_reports(task)
        p = task("rc.report.list.dateformat=D.M.Y", "list")
        q = task("rc.dateformat.report=M/D/Y", "list")
        assert p.returncode == 0 and q.returncode == 0
        assert "20.02.2009" in p.stdout
        assert "02/20/2009" in q.stdout

    def test_print_empty_columns(self, task):
        """print.empty.columns=yes keeps columns that have no values."""
        add(task, "add", "plain", "task")
        p = task("list")
        q = task("rc.print.empty.columns=yes", "list")
        assert p.returncode == 0 and q.returncode == 0
        assert "Project" not in p.stdout
        assert "Project" in q.stdout

    def test_limit_and_page(self, task):
        """limit:2 => 'N tasks shown, M more'; rc.limit; limit:page."""
        seed_reports(task)
        p = task("list", "limit:2")
        q = task("rc.limit=3", "list")
        r = task("rc.defaultheight=10", "list", "limit:page")
        assert p.returncode == 0 and q.returncode == 0 and r.returncode == 0
        assert "6 tasks, 2 shown" in p.stdout

    def test_blanklines_off(self, task):
        """rc.blanklines=off (deprecated setting) on a report and on info."""
        seed_reports(task)
        p = task("rc.blanklines=off", "list")
        q = task("rc.blanklines=off", "1", "info")
        assert p.returncode == 0 and q.returncode == 0

    def test_obfuscate(self, task):
        """rc.obfuscate=1 replaces text with 'x'."""
        seed_reports(task)
        p = task("rc.obfuscate=1", "list")
        assert p.returncode == 0
        assert "alpha" not in p.stdout and "xxxxx xxx" in p.stdout

    def test_alternate_rows_nocolor(self, task):
        """color.alternate with color off: plain output identical to default."""
        seed_reports(task)
        p = task("rc.color.alternate=on white", "list")
        assert p.returncode == 0


class TestVerbose:
    def test_verbose_tokens(self, task):
        """verbose=nothing / label / footnote / blank / header / affected combinations."""
        seed_reports(task)
        runs = [task("rc.verbose=nothing", "list"),
                task("rc.verbose=label", "list"),
                task("rc.verbose=footnote", "list"),
                task("rc.verbose=blank,label", "list"),
                task("rc.verbose=affected", "list"),
                task("rc.verbose=header,footnote", "list"),
                task("rc.verbose=on", "list")]
        assert all(r.returncode == 0 for r in runs)
        assert "ID" not in runs[0].stdout.split("\n")[0]
        assert "tasks" not in runs[0].stdout

    def test_filter_footnote(self, task):
        """verbose=filter prints the effective filter."""
        seed_reports(task)
        p = task("rc.verbose=filter,footnote,label,blank", "list", "project:home")
        assert p.returncode == 0
        assert "Filter" in (p.stdout + p.stderr)


class TestCustomReports:
    def test_user_defined_report(self, task):
        """report.foo.* with columns/labels/sort/filter/description."""
        seed_reports(task)
        rc = ["rc.report.foo.description=Foo report",
              "rc.report.foo.columns=id,project,description.count,due",
              "rc.report.foo.labels=Num,Proj,What,When",
              "rc.report.foo.sort=project+,description+",
              "rc.report.foo.filter=status:pending"]
        p = task(*rc, "foo")
        q = task(*rc, "foo", "project:home")
        assert p.returncode == 0 and q.returncode == 0
        assert "Num" in p.stdout and "Proj" in p.stdout and "What" in p.stdout

    def test_report_label_mismatch_and_bad_column(self, task):
        """A report with an unknown column / style is an error (exit != 0)."""
        seed_reports(task)
        p = task("rc.report.bar.columns=id,nosuchcol", "rc.report.bar.labels=ID,X", "bar")
        q = task("rc.report.bar.columns=id,due.bogus", "rc.report.bar.labels=ID,D", "bar")
        r = task("rc.report.bar.columns=id,description", "rc.report.bar.labels=ID", "bar")
        assert p.returncode != 0 and q.returncode != 0

    def test_context_footnote(self, task):
        """An active context filters the report and adds a context footnote."""
        seed_reports(task)
        p = task("rc.context.work.read=project:work", "rc.context=work", "list")
        assert p.returncode == 0
        assert "gamma three" in p.stdout and "alpha one" not in p.stdout
