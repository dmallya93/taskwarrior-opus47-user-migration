"""Datetime "now" as observed through the CLI under TZ and libfaketime.

* CmdVersion: "Copyright (C) 2006 - <Datetime().year()>" - LOCAL year (localtime_r).
* Context::run with rc.debug=1: "Perf task 3.5.0 <commit> <Datetime().toISO()> init:.. total:.."
  on stderr - toISO is always UTC with a trailing Z.
"""

import calendar
import re

import pytest

from conftest import faketime_env


def epoch(y, mo, d, h=0, mi=0, s=0):
    return calendar.timegm((y, mo, d, h, mi, s, 0, 0, 0))


E_DEC31_NOON_UTC = epoch(2030, 12, 31, 12, 0, 0)   # Kiritimati(+14): 2031-01-01 02:00
E_JAN1_0300_UTC = epoch(2031, 1, 1, 3, 0, 0)       # Los Angeles(-8): 2030-12-31 19:00

COPYRIGHT_RE = re.compile(r"^Copyright \(C\) 2006 - (\d+) T\. Babej, P\. Beckingham, F\. Hernandez\.$",
                          re.M)
PERF_RE = re.compile(r"^Perf task 3\.5\.0 ([0-9a-f]+|-) (\d{8}T\d{6}Z) init:\d+ load:\d+ gc:-?\d+ "
                     r"filter:\d+ commit:\d+ sort:\d+ render:\d+ hooks:\d+ other:-?\d+ total:\d+$", re.M)


class TestVersionYear:
    @pytest.mark.parametrize("tz,fake,year", [
        ("Pacific/Kiritimati", E_DEC31_NOON_UTC, 2031),
        ("UTC", E_DEC31_NOON_UTC, 2030),
        ("America/Los_Angeles", E_DEC31_NOON_UTC, 2030),
        ("America/Los_Angeles", E_JAN1_0300_UTC, 2030),
        ("UTC", E_JAN1_0300_UTC, 2031),
        ("Asia/Tokyo", E_JAN1_0300_UTC, 2031),
    ])
    def test_version_year_follows_local_tz(self, sb, record, tz, fake, year):
        """Same instant, different TZ => copyright year is the LOCAL year at that instant."""
        env = dict(faketime_env(fake), TZ=tz)
        args = ["rc.verbose=nothing", "version"]
        p = sb.run(args, extra_env=env)
        record(p, args, env_delta={"TZ": tz, "FAKETIME": str(fake)})
        assert p.returncode == 0, p.stderr
        m = COPYRIGHT_RE.search(p.stdout)
        assert m, p.stdout
        assert int(m.group(1)) == year

    @pytest.mark.parametrize("y", [2001, 2099])
    def test_version_year_faketime_far_dates(self, sb, record, y):
        """faketime to mid-year 2001 / 2099 => copyright year reported as-is."""
        fake = epoch(y, 6, 15, 12, 0, 0)
        env = dict(faketime_env(fake), TZ="Europe/Berlin")
        args = ["rc.verbose=nothing", "version"]
        p = sb.run(args, extra_env=env)
        record(p, args, env_delta={"TZ": "Europe/Berlin", "FAKETIME": str(fake)})
        assert p.returncode == 0, p.stderr
        assert "Copyright (C) 2006 - %d T. Babej, P. Beckingham, F. Hernandez.\n" % y in p.stdout


class TestPerfTimestamp:
    @pytest.mark.parametrize("tz", ["UTC", "Pacific/Kiritimati", "America/New_York", "Asia/Kolkata"])
    def test_perf_line_is_utc_iso(self, sb, record, tz):
        """rc.debug=1 Perf line carries Datetime().toISO() = UTC YYYYMMDDTHHMMSSZ regardless of TZ."""
        env = dict(faketime_env(E_DEC31_NOON_UTC), TZ=tz)
        args = ["rc.verbose=nothing", "rc.debug=1", "_version"]
        p = sb.run(args, extra_env=env)
        m = PERF_RE.search(p.stderr)
        record(p, args, env_delta={"TZ": tz, "FAKETIME": str(E_DEC31_NOON_UTC)},
               extra={"perf": m.group(0) if m else None})
        assert p.returncode == 0, p.stderr
        assert m, p.stderr
        assert m.group(2) == "20301231T120000Z"

    def test_perf_line_dst_instant(self, sb, record):
        """An instant inside US DST (2030-07-04 16:05:09 UTC) under New York still prints UTC."""
        fake = epoch(2030, 7, 4, 16, 5, 9)
        env = dict(faketime_env(fake), TZ="America/New_York")
        args = ["rc.verbose=nothing", "rc.debug=1", "version"]
        p = sb.run(args, extra_env=env)
        m = PERF_RE.search(p.stderr)
        record(p, args, env_delta={"TZ": "America/New_York", "FAKETIME": str(fake)},
               extra={"perf": m.group(0) if m else None})
        assert p.returncode == 0, p.stderr
        assert m, p.stderr
        assert m.group(2) == "20300704T160509Z"
        assert "Copyright (C) 2006 - 2030 " in p.stdout

    def test_perf_line_absent_without_debug(self, sb, record):
        """Without rc.debug the Perf line is not emitted."""
        env = dict(faketime_env(E_DEC31_NOON_UTC), TZ="UTC")
        args = ["rc.verbose=nothing", "version"]
        p = sb.run(args, extra_env=env)
        record(p, args, env_delta={"TZ": "UTC", "FAKETIME": str(E_DEC31_NOON_UTC)})
        assert p.returncode == 0
        assert "Perf task" not in p.stderr
        assert p.stderr == ""
