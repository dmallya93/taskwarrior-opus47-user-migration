"""version / _version / --version / --help (main.cpp + CmdVersion.cpp)."""

import os
import re
import subprocess

from conftest import TARGET_REPO, VERSION, origin_version_banner


def hdr(sb, path=None):
    return "TASKRC override: %s\n" % (path or sb.taskrc)


class TestVersionOption:
    def test_version_option_exact(self, sb, check):
        """`task --version` prints exactly VERSION, no config/IO (rc file need not exist)."""
        env = sb.base_env()
        env["TASKRC"] = os.path.join(sb.root, "does-not-exist.rc")
        p = sb.run(["--version"], env=env)
        check(p, ["--version"], VERSION + "\n", "", 0)

    def test_help_option_rewritten_to_help(self, sb, check):
        """`task --help` is rewritten to `help`; help is a milestone stub (target_only)."""
        p = sb.run(["--help"])
        check(p, ["--help"], "", hdr(sb) + "Command 'help' is not yet implemented.\n", 2)


class TestVersionCommand:
    def test_version_banner(self, sb, check):
        """`task version` banner, copyright year, MIT disclaimer and link wrapped to 80."""
        p = sb.run(["version"])
        check(p, ["version"], origin_version_banner(80), hdr(sb), 0)

    def test_version_defaultwidth_40(self, sb, check):
        """rc.defaultwidth=40 wraps the disclaimer/link tables to 40 columns; override footnote."""
        args = ["rc.defaultwidth=40", "version"]
        p = sb.run(args)
        check(p, args, origin_version_banner(40),
              hdr(sb) + "Configuration override rc.defaultwidth=40\n", 0)

    def test_version_defaultwidth_0_unlimited(self, sb, check):
        """defaultwidth=0 means infinite width: each paragraph on one line."""
        sb.write_rc("data.location=%s\ndefaultwidth=0\n" % sb.data)
        p = sb.run(["version"])
        check(p, ["version"], origin_version_banner(0), hdr(sb), 0)

    def test_version_bold_with_forcecolor(self, sb, check):
        """With _forcecolor=1 the package name and version are bold (ESC[1m..ESC[0m)."""
        sb.write_rc("data.location=%s\n_forcecolor=1\n" % sb.data)
        p = sb.run(["version"])
        check(p, ["version"], origin_version_banner(80, bold=True), None, 0)

    def test_version_verbose_nothing(self, sb, check):
        """rc.verbose=nothing suppresses the TASKRC header and override footnote."""
        args = ["rc.verbose=nothing", "version"]
        p = sb.run(args)
        check(p, args, origin_version_banner(80), "", 0)

    def test_version_abbreviation(self, sb, check):
        """`task ve` (>= abbreviation.minimum=2) dispatches to version."""
        p = sb.run(["ve"])
        check(p, ["ve"], origin_version_banner(80), hdr(sb), 0)


class TestUnderVersion:
    def test_under_version(self, sb, check):
        """`task _version` prints 'VERSION (<git short hash>)'; helper command => no headers."""
        h = subprocess.run(["git", "-C", TARGET_REPO, "rev-parse", "--short", "HEAD"],
                           capture_output=True, text=True).stdout.strip()
        p = sb.run(["_version"])
        expected = "%s (%s)\n" % (VERSION, h) if h else VERSION + "\n"
        ok = re.fullmatch(r"3\.5\.0( \([0-9a-f]+\))?\n", p.stdout) is not None
        check(p, ["_version"], expected, "", 0, extra={"ok": ok, "what": "format"})
