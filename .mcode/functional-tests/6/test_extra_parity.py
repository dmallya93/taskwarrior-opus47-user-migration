"""Harder combinations: colour + narrow wrapping, annotations at narrow width,
bare-ID default to info, many tasks with next's page limit, rc.dateformat
relative styles at other frozen times."""

from conftest import seed_reports, make_tc_task, add, FROZEN_EPOCH, DAY

ON = ["rc.color=on", "rc._forcecolor=on"]


class TestCombos:
    def test_color_narrow_wrap(self, task):
        """color=on with alternate rows at width 50: colour spans wrapped lines."""
        seed_reports(task)
        p = task(*ON, "rc.defaultwidth=50", "rc.color.alternate=on gray3", "list")
        assert p.returncode == 0 and "\x1b[" in p.stdout

    def test_annotations_narrow(self, sb, task):
        """long report with annotations at width 45 wraps description + annotations."""
        make_tc_task(sb, status="pending",
                     description="an annotated task whose description is quite long indeed",
                     entry=str(FROZEN_EPOCH - 7200), modified=str(FROZEN_EPOCH - 60),
                     **{"annotation_%d" % (FROZEN_EPOCH - 3000):
                        "a long annotation that also needs wrapping at narrow widths"})
        p = task("rc.defaultwidth=45", "long")
        q = task("rc.defaultwidth=45", "list")
        assert p.returncode == 0 and q.returncode == 0

    def test_bare_id_is_info(self, task):
        """`task 1` with no command runs `information` for that task."""
        seed_reports(task)
        p = task("1")
        assert p.returncode == 0
        assert "alpha one" in p.stdout and "Urgency" in p.stdout

    def test_info_color(self, task):
        """info with color=on (header/label colours)."""
        seed_reports(task)
        p = task(*ON, "3", "info")
        assert p.returncode == 0

    def test_next_page_limit(self, task):
        """next has limit:page; with many tasks and defaultheight=15 the footnote reports truncation."""
        for i in range(30):
            add(task, "add", "bulk %02d" % i, "priority:%s" % "HML"[i % 3],
                epoch=FROZEN_EPOCH - (30 - i) * 3600)
        p = task("rc.defaultheight=15", "next")
        assert p.returncode == 0
        assert "30 tasks, truncated to 7 lines" in p.stdout

    def test_relative_dates_other_time(self, task):
        """Same data viewed at now+40d: relative/remaining/countdown/age styles shift."""
        seed_reports(task)
        later = FROZEN_EPOCH + 40 * DAY
        cols = "id,due.relative,due.remaining,due.countdown,due.age,entry.age,description"
        p = task("rc.report.x.columns=" + cols, "rc.report.x.labels=a,b,c,d,e,f,g",
                 "rc.report.x.sort=id+", "rc.report.x.filter=status:pending", "x",
                 epoch=later)
        q = task("next", epoch=later)
        assert p.returncode == 0 and q.returncode == 0
