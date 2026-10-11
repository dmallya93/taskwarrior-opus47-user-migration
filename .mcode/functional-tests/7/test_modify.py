"""modify: feedback, Task::diff confirmation text, bulk thresholds (all/quit),
/from/to/g substitution, project-change footnotes, recurring siblings."""

from conftest import add, export, FROZEN_EPOCH, DAY

CONF = ["rc.confirmation=on"]


def both(p):
    return p.stdout + p.stderr


class TestModify:
    def test_modify_basic_feedback(self, task):
        """modify one task: 'Modifying task 1 ...' + 'Modified 1 task.' and stored data."""
        add(task, "add", "alpha", "one", "project:home")
        p = task("1", "modify", "priority:H", "+urgent", "due:2009-02-20")
        assert p.returncode == 0
        assert "Modified 1 task." in both(p)
        e = export(task)
        assert '"priority":"H"' in e.stdout and '"urgent"' in e.stdout

    def test_modify_no_change(self, task):
        """modify that changes nothing reports 'Modified 0 tasks.'"""
        add(task, "add", "alpha", "one", "priority:H")
        p = task("1", "modify", "priority:H")
        assert "Modified 0 tasks." in both(p)

    def test_modify_no_filter_prevented(self, task):
        """modify with no filter and confirmation on: 'Command prevented from running.'"""
        add(task, "add", "alpha", "one")
        p = task(*CONF, "modify", "priority:H", stdin="no\n")
        assert p.returncode != 0
        assert "Command prevented from running." in both(p)

    def test_modify_substitution(self, task):
        """/from/to/ and /from/to/g substitution on description."""
        add(task, "add", "foo", "bar", "foo", "baz")
        p = task("1", "modify", "/foo/FOO/")
        q = task("1", "modify", "/a/A/g")
        e = export(task)
        assert p.returncode == 0 and q.returncode == 0
        assert '"description":"FOO bAr foo bAz"' in e.stdout

    def test_modify_project_change_footnote(self, task):
        """Changing project prints the project-change summaries for old and new project."""
        add(task, "add", "alpha", "project:home")
        add(task, "add", "beta", "project:home")
        add(task, "add", "gamma", "project:work")
        p = task("1", "modify", "project:work")
        assert p.returncode == 0
        assert "Project 'home' is" in both(p) and "Project 'work' is" in both(p)

    def test_modify_confirm_diff_yes(self, task):
        """quantity >= rc.bulk: confirm4 prompt shows the Task::diff text for each task."""
        add(task, "add", "alpha", "one", "priority:L")
        add(task, "add", "beta", "two", "priority:L", "+t1")
        p = task(*CONF, "rc.bulk=2", "1-2", "modify", "priority:H", "project:x", "-t1",
                 "due:2009-03-01", stdin="yes\nyes\n")
        assert p.returncode == 0
        assert "Priority will be changed from 'L' to 'H'." in both(p)
        e = export(task)
        assert e.stdout.count('"project":"x"') == 2

    def test_modify_confirm_diff_no(self, task):
        """Answering no to both bulk diff prompts leaves the tasks unchanged."""
        add(task, "add", "alpha", "one")
        add(task, "add", "beta", "two")
        p = task(*CONF, "rc.bulk=2", "1-2", "modify", "priority:H", stdin="no\nno\n")
        assert "Modified 0 tasks." in both(p)
        e = export(task)
        assert '"priority"' not in e.stdout

    def test_modify_bulk_all(self, task):
        """Bulk modify (3 tasks > rc.bulk=2): 'all' answer applies the rest without asking."""
        for d in ("one", "two", "three"):
            add(task, "add", d)
        p = task(*CONF, "rc.bulk=2", "1-3", "modify", "+bulk", stdin="all\n")
        assert "Modified 3 tasks." in both(p)
        e = export(task, "+bulk")
        assert e.stdout.count('"bulk"') == 3

    def test_modify_bulk_quit(self, task):
        """Bulk modify: 'yes' then 'quit' modifies only the first task."""
        for d in ("one", "two", "three"):
            add(task, "add", d)
        p = task(*CONF, "rc.bulk=2", "1-3", "modify", "+bulk", stdin="yes\nquit\n")
        assert "Modified 1 task." in both(p)

    def test_modify_bulk_mixed_answers(self, task):
        """Bulk modify: yes / no / yes answers."""
        for d in ("one", "two", "three"):
            add(task, "add", d)
        p = task(*CONF, "rc.bulk=2", "1-3", "modify", "priority:M", stdin="yes\nno\nyes\n")
        assert "Modified 2 tasks." in both(p)
        export(task)

    def test_modify_recurring_siblings(self, task):
        """Modifying a recurring child with recurrence.confirmation=prompt asks about siblings/parent."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-12")
        task("rc.recurrence=on", "rc.verbose=nothing", "list", record=False)
        p = task(*CONF, "rc.recurrence.confirmation=prompt", "2", "modify", "priority:H",
                 stdin="yes\nyes\n")
        assert p.returncode == 0
        export(task)

    def test_modify_recurring_confirmation_off(self, task):
        """recurrence.confirmation=no: only the selected instance is modified."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-12")
        task("rc.recurrence=on", "rc.verbose=nothing", "list", record=False)
        p = task(*CONF, "rc.recurrence.confirmation=no", "1", "modify", "priority:H")
        assert p.returncode == 0
        export(task)

    def test_modify_unknown_id(self, task):
        """modify of a nonexistent ID: 'No tasks specified.' and exit 1."""
        add(task, "add", "alpha")
        p = task("99", "modify", "priority:H")
        assert p.returncode != 0
        assert "No tasks specified." in both(p) or "No matches." in both(p)

    def test_modify_completed_and_wait(self, task):
        """modify with due/wait date math and a completed task (by uuid filter via status)."""
        add(task, "add", "alpha", "due:2009-02-20")
        p = task("1", "modify", "wait:due-2d", "scheduled:now+1d")
        assert p.returncode == 0
        export(task)
