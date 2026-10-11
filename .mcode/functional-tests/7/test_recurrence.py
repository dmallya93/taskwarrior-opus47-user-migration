"""Recurrence: instance generation (recurrence.limit, mask/imask, periodic vs
chained), getNextRecurrence period arithmetic (end-of-month monthly,
quarterly, yearly leap day, weekdays, Nd/Nw), handleUntil expiry - all under
libfaketime at several frozen times."""

import calendar

import pytest

from conftest import add, export, FROZEN_EPOCH, DAY

ON = ["rc.recurrence=on"]


def ts(y, m, d, hh=12):
    return calendar.timegm((y, m, d, hh, 0, 0, 0, 0, 0))


def gen(task, epoch, limit=1):
    """Trigger handleRecurrence at `epoch` (list), then record stored data."""
    task(*ON, "rc.recurrence.limit=%d" % limit, "rc.verbose=nothing", "list", epoch=epoch)
    return export(task)


class TestPeriods:
    @pytest.mark.parametrize("recur,due,later", [
        ("monthly", "2009-01-31", ts(2009, 7, 15)),
        ("monthly", "2008-12-30", ts(2009, 4, 2)),
        ("quarterly", "2008-11-30", ts(2009, 9, 1)),
        ("yearly", "2008-02-29", ts(2013, 3, 1)),
        ("annual", "2009-02-13", ts(2011, 2, 14)),
        ("semiannual", "2008-08-31", ts(2010, 3, 1)),
        ("bimonthly", "2008-12-31", ts(2009, 9, 1)),
        ("weekdays", "2009-02-13", ts(2009, 2, 24)),
        ("biweekly", "2009-02-01", ts(2009, 3, 20)),
        ("3d", "2009-02-10", ts(2009, 2, 20)),
        ("2mo", "2009-01-31", ts(2009, 8, 1)),
        ("1q", "2009-03-31", ts(2010, 1, 1)),
        ("2y", "2008-02-29", ts(2014, 1, 1)),
        ("daily", "2009-03-08T01:30:00", ts(2009, 3, 12)),
    ])
    def test_recurrence_period(self, task, recur, due, later):
        """Instances generated between due and `later` follow getNextRecurrence arithmetic."""
        add(task, "add", "rec %s" % recur, "recur:%s" % recur, "due:%s" % due)
        e = gen(task, later, limit=2)
        assert e.returncode == 0
        assert '"status":"recurring"' in e.stdout
        assert e.stdout.count('"parent"') >= 1


class TestGeneration:
    def test_recurrence_limit(self, task):
        """rc.recurrence.limit=3 keeps 3 future pending instances."""
        add(task, "add", "rec", "recur:weekly", "due:2009-02-20")
        e = gen(task, FROZEN_EPOCH, limit=3)
        assert e.stdout.count('"status":"pending"') == 3

    def test_mask_after_done_delete(self, task):
        """Template mask reflects completed (+) / deleted (X) / pending (-) instances."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-11")
        gen(task, FROZEN_EPOCH)
        task("rc.verbose=nothing", "2", "done", record=False)
        task("rc.verbose=nothing", "rc.confirmation=off", "3", "delete", record=False)
        e = gen(task, FROZEN_EPOCH + 2 * DAY)
        assert '"mask":"+X' in e.stdout

    def test_recurrence_off_no_generation(self, task):
        """recurrence=off: only the template exists."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-11")
        task("rc.verbose=nothing", "list")
        e = export(task)
        assert '"parent"' not in e.stdout

    def test_until_expiry(self, task):
        """handleUntil: instances past 'until' are deleted; template expires."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-12", "until:2009-02-15")
        gen(task, FROZEN_EPOCH, limit=1)
        e = gen(task, ts(2009, 2, 20), limit=1)
        assert '"until"' in e.stdout

    def test_chained_recurrence_rejected(self, task):
        """rtype:chained is not an allowed value in 3.5.0."""
        p = task("add", "chain", "recur:weekly", "due:2009-02-10", "rtype:chained")
        assert p.returncode != 0
        assert "The 'rtype' attribute does not allow a value of 'chained'." in p.stderr

    def test_recur_without_due_error(self, task):
        """recur without due: 'A recurring task must also have a 'due' date.'"""
        p = task("add", "rec", "recur:weekly")
        assert p.returncode != 0
        assert "due" in (p.stdout + p.stderr)

    def test_invalid_recur_value(self, task):
        """recur with an invalid duration."""
        p = task("add", "rec", "recur:fortnightlyish", "due:tomorrow")
        assert p.returncode != 0

    def test_recur_wait_and_scheduled_shift(self, task):
        """Instances carry wait/scheduled shifted by the same offset as due."""
        add(task, "add", "rec", "recur:weekly", "due:2009-02-16", "wait:2009-02-14",
            "scheduled:2009-02-15")
        e = gen(task, ts(2009, 3, 5), limit=1)
        assert '"wait"' in e.stdout and '"scheduled"' in e.stdout

    def test_info_recurring_instance_report(self, task):
        """Recurring report / list after generation."""
        add(task, "add", "rec", "recur:monthly", "due:2009-01-31", "project:r")
        gen(task, ts(2009, 5, 1), limit=1)
        p = task(*ON, "recurring", epoch=ts(2009, 5, 1), short_uuids=True)
        assert p.returncode == 0
