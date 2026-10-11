"""done / delete: feedback, confirmation, unblocked notices, dependency chain
prompts, recurring siblings, nag."""

from conftest import add, export

CONF = ["rc.confirmation=on"]


def both(p):
    return p.stdout + p.stderr


class TestDone:
    def test_done_single(self, task):
        """done: 'Completed task 1 'alpha'.' + 'Completed 1 task.' and status/end stored."""
        add(task, "add", "alpha", "project:home")
        add(task, "add", "beta", "project:home")
        p = task("1", "done")
        assert p.returncode == 0
        assert "Completed task 1 'alpha'." in both(p) and "Completed 1 task." in both(p)
        e = export(task)
        assert '"status":"completed"' in e.stdout and '"end":' in e.stdout

    def test_done_already_completed(self, task):
        """done on a completed task (by uuid): 'Task ... is neither pending nor waiting.'"""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "done", record=False)
        p = task("status:completed", "done")
        assert "Completed 0 tasks." in both(p)

    def test_done_unblocks(self, task):
        """Completing a blocking task prints the 'Unblocked' notice for the dependent."""
        add(task, "add", "blocker")
        add(task, "add", "blocked", "depends:1")
        p = task("1", "done")
        assert "Unblocked 2 'blocked'." in both(p)

    def test_done_blocked_dependency_chain(self, task):
        """Completing a task in the middle of a chain with dependency.confirmation prompts to fix links."""
        add(task, "add", "one")
        add(task, "add", "two", "depends:1")
        add(task, "add", "three", "depends:2")
        p = task(*CONF, "rc.dependency.confirmation=on", "2", "done", stdin="yes\n")
        assert p.returncode == 0
        export(task)

    def test_done_bulk_all(self, task):
        """done on 3 tasks with rc.bulk=2: confirm4 prompts, 'all' completes them all."""
        for d in ("one", "two", "three"):
            add(task, "add", d)
        p = task(*CONF, "rc.bulk=2", "1-3", "done", stdin="all\n")
        assert "Completed 3 tasks." in both(p)

    def test_done_with_modification(self, task):
        """done with modifications applies them before completion."""
        add(task, "add", "alpha")
        p = task("1", "done", "priority:H", "+finished")
        assert p.returncode == 0
        export(task)

    def test_done_nag(self, task):
        """nag message printed when completing a task that is not the most urgent."""
        add(task, "add", "low", "priority:L")
        add(task, "add", "high", "priority:H", "due:yesterday")
        p = task("rc.nag=NAG! Do the urgent one.", "1", "done")
        assert "NAG! Do the urgent one." in both(p)

    def test_done_recurring_instance(self, task):
        """Completing a recurring instance; recurrence on generates further instances."""
        add(task, "add", "rec", "recur:weekly", "due:2009-02-10")
        task("rc.recurrence=on", "rc.verbose=nothing", "list", record=False)
        p = task("rc.recurrence=on", "2", "done")
        assert p.returncode == 0
        export(task)


class TestDelete:
    def test_delete_confirm_yes(self, task):
        """delete needs confirmation: 'Delete task 1 'alpha'? (yes/no)' then 'Deleted 1 task.'"""
        add(task, "add", "alpha")
        p = task(*CONF, "1", "delete", stdin="yes\n")
        assert "Delete task 1 'alpha'?" in both(p) and "Deleted 1 task." in both(p)
        export(task)

    def test_delete_confirm_no(self, task):
        """Answering no: 'Task not deleted.' and 'Deleted 0 tasks.'"""
        add(task, "add", "alpha")
        p = task(*CONF, "1", "delete", stdin="no\n")
        assert "Task not deleted." in both(p)
        assert "Deleted 0 tasks." in both(p)

    def test_delete_already_deleted(self, task):
        """Deleting an already deleted task reports 'is not deletable'."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "delete", record=False)
        p = task("status:deleted", "delete")
        assert "is not deletable." in both(p)

    def test_delete_recurring_siblings(self, task):
        """Deleting a recurring instance with recurrence.confirmation=prompt asks to delete siblings."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-11")
        task("rc.recurrence=on", "rc.verbose=nothing", "list", record=False)
        p = task(*CONF, "rc.recurrence.confirmation=prompt", "2", "delete", stdin="yes\nyes\n")
        assert p.returncode == 0
        export(task)

    def test_delete_recurring_siblings_no(self, task):
        """Declining the sibling prompt deletes only the selected instance."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-11")
        task("rc.recurrence=on", "rc.verbose=nothing", "list", record=False)
        p = task(*CONF, "rc.recurrence.confirmation=prompt", "2", "delete", stdin="yes\nno\n")
        assert p.returncode == 0
        export(task)

    def test_delete_bulk_quit(self, task):
        """Bulk delete with quit."""
        for d in ("one", "two", "three"):
            add(task, "add", d)
        p = task(*CONF, "rc.bulk=2", "1-3", "delete", stdin="yes\nquit\n")
        assert "Deleted 1 task." in both(p)

    def test_delete_unblocks(self, task):
        """Deleting a blocking task reports unblocked tasks."""
        add(task, "add", "blocker", "project:p")
        add(task, "add", "blocked", "depends:1")
        p = task("1", "delete")
        assert "Unblocked 2 'blocked'." in both(p)
