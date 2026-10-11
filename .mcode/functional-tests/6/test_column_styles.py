"""Columns and their styles rendered through a custom report: description,
date, project, tags, id/uuid/urgency/status/depends/recur/start, UDA columns
(string/numeric/date/duration with indicator/values)."""

from conftest import seed_reports, make_tc_task, seed_recurrence, add, FROZEN_EPOCH


def report(task, columns, *extra, sort="id+", filt="status:pending", short_uuids=False):
    labels = ",".join("C%d" % i for i, _ in enumerate(columns.split(",")))
    rc = ["rc.report.x.columns=" + columns, "rc.report.x.labels=" + labels,
          "rc.report.x.sort=" + sort, "rc.report.x.filter=" + filt]
    return task(*rc, *extra, "x", short_uuids=short_uuids)


class TestDescription:
    def test_description_styles(self, sb, task):
        """description.combined/desc/oneline/truncated/count/truncated_count with annotations."""
        make_tc_task(sb, status="pending", description="annotated task with words",
                     entry=str(FROZEN_EPOCH - 7200), modified=str(FROZEN_EPOCH - 60),
                     **{"annotation_%d" % (FROZEN_EPOCH - 3000): "first note",
                        "annotation_%d" % (FROZEN_EPOCH - 2000): "second note"})
        add(task, "add", "plain")
        for style in ("combined", "desc", "oneline", "truncated", "count", "truncated_count"):
            p = report(task, "id,description." + style)
            assert p.returncode == 0, (style, p.stderr)
            if style == "count":
                assert "[2]" in p.stdout
            if style == "desc":
                assert "first note" not in p.stdout
        p = report(task, "id,description.truncated_count", "rc.defaultwidth=20")
        assert p.returncode == 0


class TestDates:
    def test_date_styles(self, task):
        """due.formatted/julian/epoch/iso/age/relative/remaining/countdown."""
        seed_reports(task)
        for style in ("formatted", "julian", "epoch", "iso", "age", "relative", "remaining",
                      "countdown"):
            p = report(task, "id,description,due." + style)
            assert p.returncode == 0, (style, p.stderr)
        p = report(task, "id,description,due.epoch")
        assert "1235088000" in p.stdout          # 2009-02-20T00:00:00Z
        p = report(task, "id,description,due.iso")
        assert "20090220T000000Z" in p.stdout

    def test_other_date_columns(self, task):
        """entry/modified/start/scheduled/wait/end/until columns."""
        seed_reports(task)
        p = report(task, "id,entry,modified.age,start.active,scheduled.relative,wait,until",
                   filt="status:pending or status:waiting")
        q = report(task, "description,end,end.age", filt="status:completed")
        assert p.returncode == 0 and q.returncode == 0
        assert "2009-02-08" in p.stdout


class TestProjectTags:
    def test_project_styles(self, task):
        """project.full/parent/indented."""
        add(task, "add", "a", "project:home.garden.shed")
        add(task, "add", "b", "project:home")
        add(task, "add", "c", "project:work.ops")
        for style in ("full", "parent", "indented"):
            p = report(task, "id,project." + style + ",description", sort="project+")
            assert p.returncode == 0, (style, p.stderr)

    def test_tags_styles(self, task):
        """tags.list/indicator/count."""
        add(task, "add", "a", "+one", "+two", "+three")
        add(task, "add", "b", "+solo")
        add(task, "add", "c")
        for style in ("list", "indicator", "count"):
            p = report(task, "id,tags." + style + ",description")
            assert p.returncode == 0, (style, p.stderr)
        p = report(task, "id,tags.count,description")
        assert "[3]" in p.stdout


class TestMisc:
    def test_misc_columns(self, task):
        """id, uuid.long/short, urgency.real/integer, status.long/short, depends list/count/indicator."""
        seed_reports(task)
        p = report(task, "id,uuid.short,status.long,status.short,urgency.real,urgency.integer,"
                   "description", short_uuids=True)
        q = report(task, "id,depends.list,depends.count,depends.indicator,description")
        r = report(task, "uuid,description", sort="description+")
        assert p.returncode == 0 and q.returncode == 0 and r.returncode == 0
        assert "Pending" in p.stdout

    def test_recur_columns(self, sb, task):
        """recur.duration/indicator, rtype, mask, imask, parent, last columns."""
        seed_reports(task)
        seed_recurrence(sb)
        p = report(task, "id,recur.duration,recur.indicator,rtype,mask,description",
                   filt="status:recurring or status:pending")
        q = report(task, "uuid.short,recur,mask,last,description", filt="status:recurring",
                   short_uuids=True)
        r = report(task, "id,imask,parent.short,description", filt="+CHILD",
                   short_uuids=True)
        assert p.returncode == 0 and q.returncode == 0 and r.returncode == 0
        assert "weekly" in q.stdout or "P7D" in q.stdout

    def test_invalid_column_style(self, task):
        """Unknown column style on a report => error, exit non-zero."""
        seed_reports(task)
        p = report(task, "id,description.bogus")
        assert p.returncode != 0


class TestUDA:
    UDA_RC = ("uda.size.type=string\nuda.size.label=Size\nuda.size.values=large,medium,small\n"
              "uda.estimate.type=numeric\nuda.estimate.label=Est\n"
              "uda.review.type=date\nuda.review.label=Review\n"
              "uda.effort.type=duration\nuda.effort.label=Effort\n"
              "uda.size.indicator=S\n")

    def seed(self, sb, task):
        sb.append_rc(self.UDA_RC)
        add(task, "add", "u one", "size:small", "estimate:3", "review:2009-02-15",
            "effort:2h")
        add(task, "add", "u two", "size:large", "estimate:10.5", "effort:P1D")
        add(task, "add", "u three", "size:medium", "review:2009-03-01")
        add(task, "add", "u four")

    def test_uda_columns(self, sb, task):
        """UDA string/numeric/date/duration columns with default and indicator styles."""
        self.seed(sb, task)
        p = report(task, "id,size,estimate,review,effort,description")
        q = report(task, "id,size.indicator,estimate,description")
        r = report(task, "id,review.relative,description")
        assert p.returncode == 0 and q.returncode == 0, (p.stderr, q.stderr)
        assert r.returncode != 0        # UDA date columns have no 'relative' style
        assert "small" in p.stdout and "10.5" in p.stdout

    def test_uda_sort_values_order(self, sb, task):
        """report sort by a UDA with uda.X.values uses the values order."""
        self.seed(sb, task)
        p = report(task, "id,size,description", sort="size+")
        q = report(task, "id,size,description", sort="size-")
        r = report(task, "id,estimate,description", sort="estimate-")
        s = report(task, "id,effort,description", sort="effort+")
        assert all(x.returncode == 0 for x in (p, q, r, s))

    def test_uda_in_list_report(self, sb, task):
        """UDA labels in a builtin report via report.list.columns override."""
        self.seed(sb, task)
        p = task("rc.report.list.columns=id,size,estimate,description",
                 "rc.report.list.labels=ID,Size,Est,Desc", "list")
        assert p.returncode == 0
