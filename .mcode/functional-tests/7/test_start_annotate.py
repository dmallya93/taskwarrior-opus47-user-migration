"""start / stop (journal.time), annotate / denotate, append / prepend, duplicate."""

from conftest import add, export, FROZEN_EPOCH

CONF = ["rc.confirmation=on"]


def both(p):
    return p.stdout + p.stderr


class TestStartStop:
    def test_start_stop(self, task):
        """start sets start; stop removes it; feedback lines."""
        add(task, "add", "alpha")
        p = task("1", "start")
        assert "Starting task 1 'alpha'." in both(p) and "Started 1 task." in both(p)
        q = task("1", "stop", epoch=FROZEN_EPOCH + 600)
        assert "Stopping task 1 'alpha'." in both(q) and "Stopped 1 task." in both(q)
        export(task)

    def test_start_already_started(self, task):
        """start on an active task: 'Task 1 'alpha' already started.'"""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "start", record=False)
        p = task("1", "start")
        assert "already started." in both(p)

    def test_stop_not_started(self, task):
        """stop on an inactive task: 'Task 1 'alpha' not started.'"""
        add(task, "add", "alpha")
        p = task("1", "stop")
        assert "not started." in both(p)

    def test_journal_time_annotations(self, task):
        """rc.journal.time=on adds 'Started task' / 'Stopped task' annotations."""
        add(task, "add", "alpha")
        task("rc.journal.time=on", "1", "start")
        task("rc.journal.time=on", "1", "stop", epoch=FROZEN_EPOCH + 60)
        e = export(task)
        assert "Started task" in e.stdout and "Stopped task" in e.stdout

    def test_journal_time_custom_text(self, task):
        """journal.time.start.annotation / stop.annotation custom texts."""
        add(task, "add", "alpha")
        task("rc.journal.time=on", "rc.journal.time.start.annotation=Go", "1", "start")
        task("rc.journal.time=on", "rc.journal.time.stop.annotation=Halt", "1", "stop",
             epoch=FROZEN_EPOCH + 60)
        e = export(task)
        assert '"description":"Go"' in e.stdout and '"description":"Halt"' in e.stdout

    def test_start_blocked_dependency_reminder(self, task):
        """Starting a blocked task prints the dependency reminder."""
        add(task, "add", "blocker")
        add(task, "add", "blocked", "depends:1")
        p = task("rc.dependency.reminder=on", "2", "start")
        assert p.returncode == 0
        assert "blocked" in both(p).lower()

    def test_start_with_mods_and_done_stops(self, task):
        """start with modifications; completing an active task removes start."""
        add(task, "add", "alpha")
        task("1", "start", "priority:M")
        task("1", "done", epoch=FROZEN_EPOCH + 120)
        export(task)


class TestAnnotate:
    def test_annotate(self, task):
        """annotate adds an annotation: 'Annotating task 1 ...' 'Annotated 1 task.'"""
        add(task, "add", "alpha")
        p = task("1", "annotate", "a", "note")
        assert "Annotated 1 task." in both(p)
        e = export(task)
        assert '"description":"a note"' in e.stdout

    def test_annotate_missing_text(self, task):
        """annotate without text: 'Additional text must be provided.'"""
        add(task, "add", "alpha")
        p = task("1", "annotate")
        assert p.returncode != 0
        assert "Additional text must be provided." in both(p)

    def test_denotate_exact_and_partial(self, task):
        """denotate removes by exact text, then by partial match."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "annotate", "first note", record=False)
        task("rc.verbose=nothing", "1", "annotate", "second note", record=False,
             epoch=FROZEN_EPOCH + 1)
        p = task("1", "denotate", "first note")
        q = task("1", "denotate", "sec")
        assert "Denotated 1 task." in both(p) and "Denotated 1 task." in both(q)
        export(task)

    def test_denotate_not_found(self, task):
        """denotate with text that matches nothing: 'Did not find any matching annotation to be deleted for ...'"""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "annotate", "note", record=False)
        p = task("1", "denotate", "zzz")
        assert "Did not find any matching annotation" in both(p)

    def test_denotate_no_annotations(self, task):
        """denotate a task without annotations."""
        add(task, "add", "alpha")
        p = task("1", "denotate")
        assert p.returncode != 0 or "0 tasks" in both(p)

    def test_append_prepend(self, task):
        """append / prepend modify description and report counts."""
        add(task, "add", "middle")
        p = task("1", "append", "end", "+tag")
        q = task("1", "prepend", "start")
        assert "Appended 1 task." in both(p) and "Prepended 1 task." in both(q)
        e = export(task)
        assert '"description":"start middle end"' in e.stdout

    def test_append_missing_text(self, task):
        """append without text errors."""
        add(task, "add", "middle")
        p = task("1", "append")
        assert p.returncode != 0
        assert "Additional text must be provided." in both(p)

    def test_append_bulk_all(self, task):
        """append to 3 tasks with rc.bulk=2 and 'all'."""
        for d in ("one", "two", "three"):
            add(task, "add", d)
        p = task(*CONF, "rc.bulk=2", "1-3", "append", "X", stdin="all\n")
        assert "Appended 3 tasks." in both(p)


class TestDuplicate:
    def test_duplicate(self, task):
        """duplicate: 'Duplicated task 1 'alpha'.' 'Created task 2.' 'Duplicated 1 task.'"""
        add(task, "add", "alpha", "project:home", "+t")
        p = task("1", "duplicate", "priority:H")
        assert "Duplicated 1 task." in both(p)
        e = export(task)
        assert e.stdout.count('"description":"alpha"') == 2

    def test_duplicate_recurring(self, task):
        """duplicate a recurring instance creates a new recurring template."""
        add(task, "add", "rec", "recur:weekly", "due:2009-02-16")
        task("rc.recurrence=on", "rc.verbose=nothing", "list", record=False)
        p = task("2", "duplicate")
        assert p.returncode == 0
        export(task)

    def test_duplicate_completed(self, task):
        """duplicate a completed task produces a pending task without end."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "done", record=False)
        p = task("status:completed", "duplicate")
        assert p.returncode == 0
        export(task)
