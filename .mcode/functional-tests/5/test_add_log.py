"""add / log commands: Task::modify (attributes, tags, text), feedback text and
project-change footnotes, verbosity tokens, validation errors."""

import json


def export_all(task):
    p = task("rc.json.array=on", "rc.verbose=nothing", "export", record=False)
    assert p.returncode == 0, p.stderr
    return json.loads(p.stdout)


class TestAdd:
    def test_add_feedback_and_project_footnote(self, task):
        """`add` prints 'Created task N.' and the onProjectChange footnote."""
        p1 = task("add", "first", "task")
        p2 = task("add", "second", "project:home")
        p3 = task("add", "third", "project:home")
        assert p1.returncode == 0 and p2.returncode == 0 and p3.returncode == 0
        # feedback on stdout, footnotes on stderr
        assert p1.stdout == "Created task 1.\n"
        assert "The project" not in p1.stderr
        assert p2.stdout == "Created task 2.\n"
        assert ("The project 'home' has changed.  Project 'home' is 0% complete "
                "(1 task remaining).\n") in p2.stderr
        assert p3.stdout == "Created task 3.\n"
        assert ("The project 'home' has changed.  Project 'home' is 0% complete "
                "(2 of 2 tasks remaining).\n") in p3.stderr

    def test_add_attributes_tags_and_modifications(self, task):
        """Attributes (abbreviated names, dates, priority), +tags, words and quoted text."""
        p = task("rc.verbose=nothing", "add", "Buy", "milk", "proj:shop", "pri:M",
                 "+errand", "+urgent", "due:2009-02-15", "scheduled:2009-02-14")
        assert p.returncode == 0, p.stderr
        assert p.stdout == ""
        t = export_all(task)
        assert len(t) == 1
        t = t[0]
        assert t["description"] == "Buy milk"
        assert t["project"] == "shop"
        assert t["priority"] == "M"
        assert sorted(t["tags"]) == ["errand", "urgent"]
        assert t["due"] == "20090215T000000Z"
        assert t["scheduled"] == "20090214T000000Z"
        assert t["entry"] == "20090213T120000Z"
        assert t["status"] == "pending"
        task("_get", "1.description", "1.project", "1.priority", "1.tags")

    def test_add_terminator(self, task):
        """Everything after `--` is description text, even attribute-like words."""
        p = task("rc.verbose=nothing", "add", "pay", "--", "project:x", "+tag", "due:tomorrow")
        assert p.returncode == 0, p.stderr
        t = export_all(task)[0]
        assert t["description"] == "pay project:x +tag due:tomorrow"
        assert "project" not in t and "tags" not in t and "due" not in t
        task("_get", "1.description")

    def test_add_empty_value_removes_attribute(self, task):
        """`project:` with an empty value leaves the attribute unset."""
        p = task("rc.verbose=nothing", "add", "noproj", "project:")
        assert p.returncode == 0, p.stderr
        t = export_all(task)[0]
        assert "project" not in t
        task("_get", "1.project")

    def test_add_verbose_new_uuid(self, task):
        """rc.verbose=new-uuid prints the UUID instead of the id."""
        p = task("rc.verbose=new-uuid", "add", "uuid", "please")
        assert p.returncode == 0
        u = task("_get", "1.uuid", record=False).stdout.strip()
        assert p.stdout == "Created task %s.\n" % u

    def test_add_without_description(self, task):
        """`add` with no description is an error (exit 2) and creates nothing."""
        p = task("add")
        q = task("add", "project:home")
        assert p.returncode != 0
        assert q.returncode != 0
        assert "description" in (p.stderr + p.stdout).lower()
        assert task("count", record=False).stdout == "0\n"

    def test_add_bad_date(self, task):
        """An unparseable date value is rejected."""
        p = task("add", "x", "due:notadate")
        assert p.returncode != 0
        assert task("count", record=False).stdout == "0\n"

    def test_add_bad_priority(self, task):
        """priority outside the allowed values (H/M/L) is rejected."""
        p = task("add", "x", "priority:Z")
        assert p.returncode != 0


class TestLog:
    def test_log_feedback(self, task):
        """`log` creates a completed task silently by default (no 'Logged task' without new-uuid)."""
        p = task("log", "did", "it")
        assert p.returncode == 0, p.stderr
        assert "Created task" not in p.stdout
        t = export_all(task)
        assert len(t) == 1 and t[0]["status"] == "completed"
        assert t[0]["end"] == "20090213T120000Z"
        task("rc.verbose=new-uuid", "log", "with", "uuid")

    def test_log_project_footnote(self, task):
        """log with project prints the 100% complete footnote."""
        task("rc.verbose=nothing", "add", "open", "project:p", record=False)
        p = task("log", "closed", "project:p")
        assert p.returncode == 0
        assert p.stdout == ""
        assert ("The project 'p' has changed.  Project 'p' is 50% complete "
                "(1 of 2 tasks remaining).\n") in p.stderr

    def test_log_rejects_wait_and_recur(self, task):
        """log refuses recurring and waiting tasks with the exact C++ errors."""
        p = task("log", "w", "wait:2009-03-01")
        q = task("log", "r", "due:2009-03-01", "recur:weekly")
        assert p.returncode != 0 and "You cannot log waiting tasks." in p.stderr
        assert q.returncode != 0 and "You cannot log recurring tasks." in q.stderr
        assert task("count", record=False).stdout == "0\n"
