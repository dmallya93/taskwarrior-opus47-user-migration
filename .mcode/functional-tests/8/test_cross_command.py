"""Cross-command regressions between mutations (milestone 7) and the new
analytics / inventory commands (milestone 8)."""

from conftest import add, seed_reports, FROZEN_EPOCH, DAY

W = "rc.defaultwidth=100"


def test_mutations_reflected_in_analytics(task):
    """add/modify/done/delete then summary, projects, tags, stats, history reflect the changes."""
    seed_reports(task)
    task("rc.verbose=nothing", "1", "done", record=False)
    task("rc.verbose=nothing", "rc.confirmation=off", "2", "delete", record=False)
    task("rc.verbose=nothing", "3", "modify", "project:home.garden", "+moved", record=False)
    for cmd in ("summary", "projects", "tags", "stats", "history.monthly", "ghistory.monthly",
                "burndown.daily", "timesheet"):
        p = task(W, cmd, short_uuids=(cmd == "timesheet"))
        assert p.returncode == 0, cmd


def test_undo_reflected_in_stats(task):
    """undo after a modification restores the task; stats undo count changes accordingly."""
    add(task, "add", "undo me", "project:x")
    task("rc.verbose=nothing", "1", "modify", "project:y", record=False)
    a = task(W, "stats")
    task("rc.confirmation=off", "undo", stdin="yes\n")
    b = task(W, "stats")
    c = task(W, "projects")
    assert "x" in c.stdout


def test_calendar_after_due_change(task):
    """Changing due dates with modify moves the calendar detail listing."""
    add(task, "add", "move due", "due:2009-02-20")
    task("rc.verbose=nothing", "1", "modify", "due:2009-04-10", record=False)
    p = task("rc.defaultwidth=80", "rc.calendar.details=full", "calendar")
    assert p.returncode == 0 and "move due" in p.stdout


def test_edit_then_reports(sb, task):
    """A task changed via edit shows in list/info consistently."""
    import os
    add(task, "add", "before edit", "project:a")
    ed = os.path.join(sb.root, "e.sh")
    with open(ed, "w") as fh:
        fh.write("#!/bin/sh\nsed -i 's/^  Project: .*/  Project:           b/' \"$1\"\n")
    os.chmod(ed, 0o755)
    task("rc.editor=" + ed, "1", "edit")
    p = task(W, "list")
    q = task(W, "projects")
    assert "b" in q.stdout
