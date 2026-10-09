"""Context::createDefaultConfig: the sample .taskrc written on first interactive run.

Header: "# [Created by task 3.5.0 <Datetime().toString("m/d/Y H:N:S")>]" - LOCAL time,
so it must follow TZ and libfaketime. Requires a TTY on stdout (pty); rc.confirmation=0
skips the confirmation prompt.
"""

import calendar
import os

import pytest

from conftest import VERSION, faketime_env

THEMES = ["light-16", "light-256", "bubblegum-256", "dark-16", "dark-256", "dark-red-256",
          "dark-green-256", "dark-blue-256", "dark-violets-256", "dark-yellow-green",
          "dark-gray-256", "dark-gray-blue-256", "solarized-dark-256", "solarized-light-256",
          "no-color"]


def epoch(y, mo, d, h=0, mi=0, s=0):
    return calendar.timegm((y, mo, d, h, mi, s, 0, 0, 0))


def expected_rc(stamp, data_location):
    """Byte-for-byte template from origin src/Context.cpp createDefaultConfig."""
    return ("# [Created by task %s %s]\n" % (VERSION, stamp)
            + "data.location=%s\n" % data_location
            + "news.version=%s\n" % VERSION
            + "\n# To use the default location of the XDG directories,\n"
            + "# move this configuration file from ~/.taskrc to ~/.config/task/taskrc and update "
              "location config as follows:\n"
            + "\n#data.location=~/.local/share/task\n"
            + "#hooks.location=~/.config/task/hooks\n"
            + "\n# Color theme (uncomment one to use)\n"
            + "include default.theme\n"
            + "".join("#include %s.theme\n" % t for t in THEMES)
            + "\n")


def first_run_env(sb, rc_path, tz, fake):
    env = sb.env(dict(faketime_env(fake), TZ=tz, TASKDATA=sb.data, TERM="xterm"), taskrc=False)
    env["TASKRC"] = rc_path
    return env


class TestDefaultRcCreation:
    @pytest.mark.parametrize("tz,fake,stamp", [
        ("Pacific/Kiritimati", epoch(2030, 12, 31, 12, 0, 0), "1/1/2031 02:00:00"),
        ("America/Los_Angeles", epoch(2031, 1, 1, 3, 0, 0), "12/31/2030 19:00:00"),
        ("Europe/Berlin", epoch(2030, 10, 27, 0, 59, 59), "10/27/2030 02:59:59"),
        ("UTC", epoch(2030, 3, 5, 7, 8, 9), "3/5/2030 07:08:09"),
    ])
    def test_default_rc_header_local_time(self, sb, record, tz, fake, stamp):
        """First interactive run writes the sample rc; header timestamp is local m/d/Y H:N:S."""
        rc_path = os.path.join(sb.home, "sub", "new.taskrc")
        args = ["rc.confirmation=0", "rc.verbose=nothing", "_version"]
        p = sb.run_pty(args, first_run_env(sb, rc_path, tz, fake))
        content = open(rc_path).read() if os.path.exists(rc_path) else None
        record(p, args, env_delta={"TZ": tz, "FAKETIME": str(fake)},
               extra={"rc_file": None if content is None else content.replace(sb.data, "<DATA>")})
        assert p.returncode == 0, p.stdout
        assert content is not None, "default rc file was not created"
        assert content == expected_rc(stamp, sb.data)

    def test_first_run_tty_version_output(self, sb, record):
        """Whole first interactive `version` run on a TTY (headers, banner, local copyright year)."""
        rc_path = os.path.join(sb.home, ".taskrc")
        fake = epoch(2030, 12, 31, 12, 0, 0)
        args = ["rc.confirmation=0", "version"]
        p = sb.run_pty(args, first_run_env(sb, rc_path, "Pacific/Kiritimati", fake))
        record(p, args, env_delta={"TZ": "Pacific/Kiritimati", "FAKETIME": str(fake)},
               extra={"stdout_normalized": p.stdout.replace(sb.root, "<ROOT>")})
        assert p.returncode == 0, p.stdout
        assert os.path.exists(rc_path)
        assert "Copyright (C) 2006 - 2031 T. Babej, P. Beckingham, F. Hernandez." in p.stdout
        assert "TASKRC override: %s" % rc_path in p.stdout

    def test_missing_rc_without_tty_refuses(self, sb, record):
        """Non-TTY stdout and missing rc => 'Cannot proceed without rc file.' exit 2, no file."""
        rc_path = os.path.join(sb.home, "missing.rc")
        env = sb.env({"TASKRC": rc_path, "TASKDATA": sb.data})
        import subprocess
        from conftest import TASK_BIN
        args = ["rc.confirmation=0", "rc.verbose=nothing", "_version"]
        p = subprocess.run([TASK_BIN] + args, capture_output=True, text=True, env=env,
                           cwd=sb.root, timeout=30)
        record(p, args)
        assert p.returncode == 2
        assert p.stdout == ""
        assert p.stderr == "Cannot proceed without rc file.\n"
        assert not os.path.exists(rc_path)
