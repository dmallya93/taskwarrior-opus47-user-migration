"""Context::staticInitialization weekstart validation (Datetime::dayOfWeek + closeEnough).

C++: auto weekStart = Datetime::dayOfWeek(config.get("weekstart"));
     if (weekStart != 0 && weekStart != 1) throw "The 'weekstart' configuration ...";
dayOfWeek matches day names case-insensitively, accepting prefixes of >= 3 chars.
"""

import pytest

from conftest import VERSION, WEEKSTART_ERROR


def hdr(sb):
    return "TASKRC override: %s\n" % sb.taskrc


class TestWeekstartInvalid:
    @pytest.mark.parametrize("value", ["Tuesday", "Saturday", "friday", "mo", "", "Tues", "xyz"])
    def test_weekstart_invalid_override(self, sb, record, value):
        """rc.weekstart=<non Sunday/Monday> => error message on stderr, exit 2, no stdout."""
        args = ["rc.weekstart=%s" % value, "version"]
        p = sb.run(args)
        record(p, args)
        assert p.returncode == 2
        assert p.stdout == ""
        assert p.stderr == hdr(sb) + "Configuration override rc.weekstart=%s\n" % value + WEEKSTART_ERROR

    def test_weekstart_invalid_in_rcfile(self, sb, record):
        """weekstart=Wednesday in the .taskrc is rejected the same way (exit 2)."""
        sb.write_rc("data.location=%s\nweekstart=Wednesday\n" % sb.data)
        p = sb.run(["version"])
        record(p, ["version"], extra={"rc": "weekstart=Wednesday"})
        assert p.returncode == 2
        assert p.stdout == ""
        assert p.stderr == hdr(sb) + WEEKSTART_ERROR

    @pytest.mark.parametrize("cmd", ["_version", "_show", "--version"])
    def test_weekstart_invalid_blocks_every_command(self, sb, record, cmd):
        """Validation happens during Context init, before any command (incl. helpers) runs."""
        args = ["rc.verbose=nothing", "rc.weekstart=Thursday", cmd]
        p = sb.run(args)
        record(p, args)
        assert p.returncode == 2
        assert p.stdout == ""
        assert p.stderr == WEEKSTART_ERROR


class TestWeekstartValid:
    @pytest.mark.parametrize("value", ["Sunday", "Monday", "sunday", "monday", "MON", "SUNDAY",
                                       "sun", "Mond", "Sund"])
    def test_weekstart_valid_values(self, sb, record, value):
        """Full, case-insensitive and >=3-char prefix forms of Sunday/Monday are accepted."""
        args = ["rc.verbose=nothing", "rc.weekstart=%s" % value, "_version"]
        p = sb.run(args)
        record(p, args)
        assert p.returncode == 0, p.stderr
        assert p.stdout.startswith(VERSION)
        assert p.stderr == ""

    def test_weekstart_monday_in_rcfile(self, sb, record):
        """weekstart=Monday in the rc file is accepted and shown verbatim by _show."""
        sb.write_rc("data.location=%s\nweekstart=Monday\n" % sb.data)
        p = sb.run(["_show"])
        record(p, ["_show"], extra={"rc": "weekstart=Monday"})
        assert p.returncode == 0, p.stderr
        assert "weekstart=Monday\n" in p.stdout
