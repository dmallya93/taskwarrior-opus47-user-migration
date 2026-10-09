"""_show / _config and configuration loading (Context::initialize + libshared Configuration)."""

import os
import re

from conftest import OracleError, origin_config, origin_show, VERSION, origin_version_banner


def oracle_show(sb, rc=None, env=None, overrides=(), taskdata=None):
    env = env or sb.base_env()
    return origin_show(origin_config(rc or sb.taskrc, env, overrides, cwd=sb.root, taskdata=taskdata))


class TestShowRaw:
    def test_show_defaults(self, sb, check):
        """_show = compiled-in Context.cpp defaults + rc file, sorted key=value."""
        p = sb.run(["_show"])
        check(p, ["_show"], oracle_show(sb), "", 0)

    def test_show_rc_parsing_rules(self, sb, check):
        """Comments, whitespace trim, json escapes, ~ and $VAR expansion, last value wins."""
        sb.write_rc(
            "# full comment line\n"
            "data.location=%s\n"
            "   spaced.key   =   spaced value   # trailing comment\n"
            "esc.key=a\\tb\\u00e9\\/c\n"
            "home.key=~/sub/dir\n"
            "env.key=$HOME/x$NOPE_UNSET_VAR/y\n"
            "dup.key=first\n"
            "dup.key=second\n"
            "defaultwidth=123\n"
            "empty.key=\n" % sb.data)
        p = sb.run(["_show"])
        check(p, ["_show"], oracle_show(sb), "", 0)

    def test_show_cli_overrides(self, sb, check):
        """rc.name=value and rc.name:value overrides; last one wins; footnotes hidden for _ cmds."""
        args = ["rc.ovr.eq=1", "rc.ovr.colon:2", "rc.confirmation=0", "rc.dup=a", "rc.dup:b", "_show"]
        p = sb.run(args)
        check(p, args, oracle_show(sb, overrides=args), "", 0)

    def test_config_completion(self, sb, check):
        """_config lists every config name sorted (first _forcecolor, last xterm.title)."""
        p = sb.run(["_config"])
        cfg = origin_config(sb.taskrc, sb.base_env(), cwd=sb.root)
        check(p, ["_config"], "".join(k + "\n" for k in sorted(cfg)), "", 0)


class TestRcFileResolution:
    def test_taskrc_env_and_rc_colon_override(self, sb, check):
        """rc:<file> overrides $TASKRC; header names the overriding file."""
        other = sb.write_rc("data.location=%s\nwhich=other\n" % sb.data,
                            os.path.join(sb.root, "other.rc"))
        args = ["rc:" + other, "version"]
        p = sb.run(args)
        check(p, args, origin_version_banner(80), "TASKRC override: %s\n" % other, 0)

    def test_rc_equals_override_show(self, sb, check):
        """rc=<file> is also accepted; the last rc: argument wins."""
        a = sb.write_rc("data.location=%s\nwhich=a\n" % sb.data, os.path.join(sb.root, "a.rc"))
        b = sb.write_rc("data.location=%s\nwhich=b\n" % sb.data, os.path.join(sb.root, "b.rc"))
        args = ["rc:" + a, "rc=" + b, "_show"]
        p = sb.run(args)
        check(p, args, oracle_show(sb, rc=b), "", 0)

    def test_home_taskrc_default_no_header(self, sb, check):
        """Without TASKRC, ~/.taskrc is used and no override header is printed."""
        rc = sb.write_rc("data.location=%s\nwhere=home\n" % sb.data, os.path.join(sb.home, ".taskrc"))
        env = sb.base_env(taskrc=False)
        p = sb.run(["_show"], env=env)
        check(p, ["_show"], oracle_show(sb, rc=rc, env=env), "", 0)

    def test_xdg_config_home_fallback(self, sb, check):
        """No ~/.taskrc: $XDG_CONFIG_HOME/task/taskrc (trailing slash stripped) is used, no header."""
        xdg = os.path.join(sb.root, "xdg")
        rc = sb.write_rc("data.location=%s\nwhere=xdg\n" % sb.data, os.path.join(xdg, "task", "taskrc"))
        env = sb.base_env(taskrc=False)
        env["XDG_CONFIG_HOME"] = xdg + "/"
        p = sb.run(["_show"], env=env)
        check(p, ["_show"], oracle_show(sb, rc=rc, env=env), "", 0)

    def test_xdg_default_dot_config(self, sb, check):
        """XDG_CONFIG_HOME unset: ~/.config/task/taskrc is used; `version` prints no header."""
        sb.write_rc("data.location=%s\n" % sb.data, os.path.join(sb.home, ".config", "task", "taskrc"))
        env = sb.base_env(taskrc=False)
        p = sb.run(["version"], env=env)
        check(p, ["version"], origin_version_banner(80), "", 0)

    def test_missing_rc_file_noninteractive(self, sb, check):
        """TASKRC points at a missing file and stdout is not a TTY: exit 2."""
        missing = os.path.join(sb.root, "missing.rc")
        env = sb.base_env()
        env["TASKRC"] = missing
        p = sb.run(["version"], env=env)
        check(p, ["version"], "",
              "TASKRC override: %s\nCannot proceed without rc file.\n" % missing, 2)
        assert not os.path.exists(missing)


class TestDataLocation:
    def test_taskdata_env(self, sb, check):
        """$TASKDATA sets data.location and prints a TASKDATA override header."""
        d = os.path.join(sb.root, "d2")
        env = sb.base_env()
        env["TASKDATA"] = d
        p = sb.run(["version"], env=env)
        check(p, ["version"], origin_version_banner(80),
              "TASKRC override: %s\nTASKDATA override: %s\n" % (sb.taskrc, d), 0)

    def test_taskdata_env_show(self, sb, check):
        """$TASKDATA value appears as data.location in _show."""
        d = os.path.join(sb.root, "d2")
        env = sb.base_env()
        env["TASKDATA"] = d
        p = sb.run(["_show"], env=env)
        check(p, ["_show"], oracle_show(sb, env=env, taskdata=d), "", 0)

    def test_rc_data_location_override(self, sb, check):
        """rc.data.location=<dir> => TASKDATA override header + configuration override footnote."""
        d = os.path.join(sb.root, "d3")
        args = ["rc.data.location=" + d, "version"]
        p = sb.run(args)
        check(p, args, origin_version_banner(80),
              "TASKRC override: %s\nTASKDATA override: %s\nConfiguration override rc.data.location=%s\n"
              % (sb.taskrc, d, d), 0)


class TestIncludes:
    def test_include_relative_absolute_and_theme(self, sb, check):
        """include relative to the rc file dir (cwd elsewhere), absolute include, bundled dark-16.theme."""
        conf = os.path.join(sb.root, "conf")
        sb.write_rc("child.key=child\n", os.path.join(conf, "sub", "child.rc"))
        absf = sb.write_rc("abs.key=abs\ncolor.due=bold red\n", os.path.join(sb.root, "abs.rc"))
        rc = sb.write_rc(
            "data.location=%s\ninclude sub/child.rc\ninclude dark-16.theme\ninclude %s\n"
            % (sb.data, absf), os.path.join(conf, "main.rc"))
        env = sb.base_env()
        env["TASKRC"] = rc
        p = sb.run(["_show"], env=env)
        check(p, ["_show"], oracle_show(sb, rc=rc, env=env), "", 0)

    def test_include_holidays(self, sb, check):
        """include holidays.en-US.rc resolves from the bundled rc directory."""
        sb.write_rc("data.location=%s\ninclude holidays.en-US.rc\n" % sb.data)
        p = sb.run(["_show"])
        out = oracle_show(sb)
        assert "holiday." in out
        check(p, ["_show"], out, "", 0)

    def test_include_missing_file(self, sb, check):
        """Unresolvable include => error on stderr, exit 2."""
        sb.write_rc("data.location=%s\ninclude no-such.theme\n" % sb.data)
        try:
            oracle_show(sb)
            msg = None
        except OracleError as e:
            msg = str(e)
        p = sb.run(["version"])
        check(p, ["version"], "", msg + "\n", 2)

    def test_include_nesting_limit(self, sb, check):
        """A self-including rc file hits the 10-level nesting limit, exit 2."""
        sb.write_rc("data.location=%s\ninclude %s\n" % (sb.data, sb.taskrc))
        p = sb.run(["_version"])
        check(p, ["_version"], "", "Configuration files may only be nested to 10 levels.\n", 2)

    def test_malformed_line(self, sb, check):
        """A line with neither '=' nor 'include' is a malformed entry, exit 2."""
        sb.write_rc("data.location=%s\ngarbage line  # with comment\n" % sb.data)
        p = sb.run(["version"])
        check(p, ["version"], "", "Malformed entry 'garbage line' in config file.\n", 2)


class TestMainAndDebug:
    def test_no_command_empty_default(self, sb, check):
        """default.command empty and no args => 'You must specify a command...' exit 2."""
        sb.write_rc("data.location=%s\ndefault.command=\n" % sb.data)
        p = sb.run([])
        check(p, [], "", "TASKRC override: %s\nYou must specify a command or a task to modify.\n"
              % sb.taskrc, 2)

    def test_no_command_override_footnote_order(self, sb, check):
        """Init failure dumps headers, then footnotes, then errors."""
        args = ["rc.default.command="]
        p = sb.run(args)
        check(p, args, "", "TASKRC override: %s\nConfiguration override rc.default.command=\n"
              "You must specify a command or a task to modify.\n" % sb.taskrc, 2)

    def test_rc_debug_timing(self, sb, check):
        """rc.debug=1 emits 'Timer Config::load (<rc>) N sec' and the Perf line on stderr."""
        args = ["rc.debug=1", "_version"]
        p = sb.run(args)
        t = re.search(r"^Timer Config::load \(%s\) \d+\.\d{6} sec$" % re.escape(sb.taskrc), p.stderr, re.M)
        perf = re.search(r"^Perf task %s \S+ \d{8}T\d{6}Z init:\d+ load:\d+ gc:\d+ filter:\d+ "
                         r"commit:\d+ sort:\d+ render:\d+ hooks:\d+ other:-?\d+ total:\d+$"
                         % re.escape(VERSION), p.stderr, re.M)
        check(p, args, None, None, 0, extra={"ok": bool(t and perf), "what": "debug-lines",
                                             "pattern": "Timer Config::load (...) N.NNNNNN sec; Perf task 3.5.0 <commit> <iso> init:.. total:.."})
        assert p.stdout.startswith(VERSION)
