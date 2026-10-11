"""calendar: month grid layout, y/month/year arguments, calendar.details /
holidays / legend / offset / weekstart, holiday include files, colours."""

from conftest import add, FROZEN_EPOCH, DAY

W = "rc.defaultwidth=80"


def seed_due(task):
    add(task, "add", "cal due one", "due:2009-02-20", "project:home")
    add(task, "add", "cal due two", "due:2009-03-05", "priority:H")
    add(task, "add", "cal overdue", "due:2009-02-10")
    add(task, "add", "cal recurring", "due:2009-02-16", "recur:weekly")


class TestCalendarGrid:
    def test_calendar_default(self, task):
        """Default calendar: 3 months starting with the current month (Feb 2009), week numbers."""
        p = task(W, "calendar")
        assert p.returncode == 0
        assert "February 2009" in p.stdout and "March 2009" in p.stdout
        assert "Su Mo Tu We Th Fr Sa" in p.stdout

    def test_calendar_month_year_args(self, task):
        """calendar <month> <year> starts the grid at that month."""
        p = task(W, "calendar", "7", "2010")
        assert p.returncode == 0
        assert "July 2010" in p.stdout

    def test_calendar_named_month(self, task):
        """calendar with a month name argument."""
        p = task(W, "calendar", "december", "2009")
        assert p.returncode == 0
        assert "December 2009" in p.stdout

    def test_calendar_year(self, task):
        """calendar y: 12 months starting at the current month (Feb 2009 - Jan 2010)."""
        p = task(W, "calendar", "y")
        assert p.returncode == 0
        assert "February 2009" in p.stdout and "January 2010" in p.stdout

    def test_calendar_explicit_year(self, task):
        """calendar 2011: whole year 2011."""
        p = task(W, "calendar", "2011")
        assert p.returncode == 0
        assert "January 2011" in p.stdout and "December 2011" in p.stdout

    def test_calendar_weekstart_monday(self, task):
        """rc.weekstart=monday changes the header order."""
        p = task(W, "rc.weekstart=monday", "calendar")
        assert p.returncode == 0
        assert "Mo Tu We Th Fr Sa Su" in p.stdout

    def test_calendar_offset(self, task):
        """calendar.offset / offset.value shift the first displayed month."""
        p = task(W, "rc.calendar.offset=on", "rc.calendar.offset.value=-1", "calendar")
        assert p.returncode == 0
        assert "January 2009" in p.stdout

    def test_calendar_wide_width(self, task):
        """Wider terminal fits more months per row (rc.defaultwidth=160)."""
        p = task("rc.defaultwidth=160", "calendar")
        assert p.returncode == 0
        assert "February 2009" in p.stdout

    def test_calendar_monthsperline(self, task):
        """rc.monthsperline=1 limits to one month per line."""
        p = task(W, "rc.monthsperline=1", "calendar")
        assert p.returncode == 0
        assert "February 2009" in p.stdout and "March 2009" not in p.stdout

    def test_calendar_bad_arg(self, task):
        """calendar with an unrecognised argument is an error."""
        p = task(W, "calendar", "bogusarg")
        assert p.returncode != 0
        assert "not recognized" in (p.stdout + p.stderr) or p.stderr


class TestCalendarDetails:
    def test_calendar_details_full(self, task):
        """calendar.details=full lists the due tasks below the grid."""
        seed_due(task)
        p = task(W, "rc.calendar.details=full", "calendar")
        assert p.returncode == 0
        assert "cal due one" in p.stdout

    def test_calendar_details_sparse(self, task):
        """calendar.details=sparse (default) lists no tasks."""
        seed_due(task)
        p = task(W, "rc.calendar.details=sparse", "calendar")
        assert p.returncode == 0
        assert "cal due one" not in p.stdout

    def test_calendar_legend_color(self, task):
        """With colour forced, the legend line and coloured due/overdue/today cells appear."""
        seed_due(task)
        p = task(W, "rc._forcecolor=on", "rc.color=on", "rc.calendar.legend=on", "calendar")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout
        assert "Legend:" in p.stdout

    def test_calendar_legend_off(self, task):
        """calendar.legend=off suppresses the legend."""
        seed_due(task)
        p = task(W, "rc._forcecolor=on", "rc.color=on", "rc.calendar.legend=off", "calendar")
        assert p.returncode == 0
        assert "Legend:" not in p.stdout

    def test_calendar_due_arg(self, task):
        """calendar due: start at the month of the earliest due task."""
        seed_due(task)
        p = task(W, "calendar", "due")
        assert p.returncode == 0
        assert "February 2009" in p.stdout


class TestCalendarHolidays:
    def test_calendar_holidays_full(self, sb, task):
        """Holiday include file + calendar.holidays=full prints the holiday list."""
        sb.append_rc("include holidays.en-US.rc\n")
        p = task(W, "rc.calendar.holidays=full", "calendar", "1", "2009")
        assert p.returncode == 0
        assert "Holidays" in p.stdout or "Date" in p.stdout

    def test_calendar_holidays_sparse_color(self, sb, task):
        """calendar.holidays=sparse with colour: holiday cells coloured, no list."""
        sb.append_rc("include holidays.en-US.rc\n")
        p = task(W, "rc._forcecolor=on", "rc.color=on", "rc.calendar.holidays=sparse",
                 "calendar", "12", "2009")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout

    def test_calendar_custom_holiday(self, sb, task):
        """A hand-written holiday.* entry shows in the full holiday listing."""
        sb.append_rc("holiday.ftrun.name=FT Run Day\nholiday.ftrun.date=20090220\n")
        p = task(W, "rc.calendar.holidays=full", "calendar")
        assert p.returncode == 0
        assert "FT Run Day" in p.stdout

    def test_calendar_holidays_de(self, sb, task):
        """German holidays file (UTF-8 names) in full listing for a whole year."""
        sb.append_rc("include holidays.de-DE.rc\n")
        p = task("rc.defaultwidth=120", "rc.calendar.holidays=full", "calendar", "2009")
        assert p.returncode == 0
