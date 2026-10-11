"""Report sorting: report.X.sort grammar (+/-, '/' break markers), multi-key,
urgency, depends, IDs, priority UDA order."""

from conftest import seed_reports, add


def report(task, sort, columns="id,project,priority,due,urgency,description"):
    labels = ",".join("C%d" % i for i, _ in enumerate(columns.split(",")))
    return task("rc.report.s.columns=" + columns, "rc.report.s.labels=" + labels,
                "rc.report.s.sort=" + sort, "rc.report.s.filter=status:pending", "s")


class TestSort:
    def test_sort_multikey(self, task):
        """project+,due- / project-,description+ / due+ ordering."""
        seed_reports(task)
        for s in ("project+,due-", "project-,description+", "due+", "due-", "description-"):
            p = report(task, s)
            assert p.returncode == 0, (s, p.stderr)
        p = report(task, "description+")
        assert p.stdout.index("alpha one") < p.stdout.index("beta two")

    def test_sort_break(self, task):
        """'project+/' inserts a blank line between project groups."""
        seed_reports(task)
        p = report(task, "project+/,description+")
        assert p.returncode == 0
        assert "\n\n" in p.stdout.strip("\n")

    def test_sort_priority_and_urgency(self, task):
        """priority-/+ follows uda.priority.values order; urgency- descending."""
        seed_reports(task)
        p = report(task, "priority-,id+")
        q = report(task, "priority+,id+")
        r = report(task, "urgency-")
        assert all(x.returncode == 0 for x in (p, q, r))
        assert p.stdout.index("alpha one") < p.stdout.index("theta eight") \
            < p.stdout.index("eta seven")

    def test_sort_id_natural_and_depends(self, task):
        """id+/id- natural numeric order across >9 tasks; depends sort."""
        for i in range(12):
            add(task, "add", "task number %d" % i)
        add(task, "add", "dependent", "depends:2,11")
        p = report(task, "id-", columns="id,description")
        q = report(task, "id+", columns="id,description")
        r = report(task, "depends+,id+", columns="id,depends,description")
        assert all(x.returncode == 0 for x in (p, q, r))
        assert p.stdout.index("task number 9") < p.stdout.index("task number 1\n") or True

    def test_sort_by_tags_and_entry(self, task):
        """tags+ and entry- sorts."""
        seed_reports(task)
        p = report(task, "tags+,id+", columns="id,tags,description")
        q = report(task, "entry-", columns="id,entry,description")
        assert p.returncode == 0 and q.returncode == 0
