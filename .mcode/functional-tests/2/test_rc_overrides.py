"""rc.<name>=<value> / rc.<name>:<value> command-line overrides.

C++ CLI2::applyOverrides: every argv entry before `--` that starts with "rc." and has a ':' or
'=' after position 3 sets config[raw[3:sep]] = raw[sep+1:] (no dequoting, no trimming); every
"rc."-prefixed entry gets a "Configuration override <raw>" footnote (verbose=override).
CLI2::lexArguments (Lexer::wasQuoted / Lexer::token / isPair / isIdentifier) decides whether
the argument is an RC/CONFIG pair (removed from the command line) or a plain word; plain
words are rejected by commands that take no arguments:
"The '<cmd>' command does not allow '<arg>'." (exit 2).
"""

import re

import pytest

from conftest import VERSION


def hdr(sb):
    return "TASKRC override: %s\n" % sb.taskrc


def show_value(stdout, name):
    for line in stdout.splitlines():
        if line.startswith(name + "="):
            return line[len(name) + 1:]
    return None


def expected_override(raw):
    m = re.search(r"[:=]", raw[3:])
    sep = 3 + m.start()
    return raw[3:sep], raw[sep + 1:]


OVERRIDE_FORMS = [
    "rc.zz=bar",
    "rc.zz:bar",
    "rc.zz::bar",
    "rc.zz:=bar",
    "rc.zz=a b",
    "rc.zz='x y'",
    'rc.zz="quoted"',
    "rc.zz=(a)",
    "rc.zz=",
    "rc.zz.yy=1",
    "rc.zz=ünï",
    "rc.zz=a\tb",
    "rc.zz=2030-01-01T00:00:00Z",
    "rc.zz=P1DT2H",
    "rc.zz={\"a\":[1,2]}",
    "rc.dateformat=Y-M-D",
    "rc.weekstart:Monday",
]


class TestOverrideApplied:
    @pytest.mark.parametrize("raw", OVERRIDE_FORMS)
    def test_override_forms_applied(self, sb, record, raw):
        """Each override form is applied verbatim (name before first ':'/'=', rest is value)."""
        args = [raw, "_show"]
        p = sb.run(args)
        name, value = expected_override(raw)
        record(p, args, extra={"show_value": show_value(p.stdout, name)})
        assert p.returncode == 0, p.stderr
        assert show_value(p.stdout, name) == value
        assert p.stderr == ""  # _show is a helper command: no header/footnotes

    def test_override_last_wins(self, sb, record):
        """Repeated overrides: the last one wins."""
        args = ["rc.zz=bar", "rc.zz:baz", "_show"]
        p = sb.run(args)
        record(p, args, extra={"show_value": show_value(p.stdout, "zz")})
        assert p.returncode == 0
        assert show_value(p.stdout, "zz") == "baz"
        assert p.stderr == ""

    def test_override_empty_name(self, sb, record):
        """rc.=x sets the empty-named key (shown as '=x')."""
        args = ["rc.=x", "_show"]
        p = sb.run(args)
        record(p, args, extra={"show_value": show_value(p.stdout, "")})
        assert p.returncode == 0, p.stderr
        assert show_value(p.stdout, "") == "x"

    def test_override_beats_rcfile(self, sb, record):
        """Command-line override replaces the rc file value."""
        sb.write_rc("data.location=%s\nzz=fromfile\n" % sb.data)
        args = ["rc.zz=fromcli", "_show"]
        p = sb.run(args)
        record(p, args, extra={"show_value": show_value(p.stdout, "zz"), "rc": "zz=fromfile"})
        assert p.returncode == 0
        assert show_value(p.stdout, "zz") == "fromcli"


class TestOverrideSideEffects:
    def test_override_footnotes_on_version(self, sb, record):
        """Overrides (incl. quoted/odd values) are stripped before dispatch and footnoted."""
        args = ["rc.zz=1", "rc.yy:a b", "rc.xx='quoted value'", "version"]
        p = sb.run(args)
        record(p, args)
        assert p.returncode == 0, p.stderr
        assert "task %s built for" % VERSION in p.stdout
        assert p.stderr == (hdr(sb) + "Configuration override rc.zz=1\n"
                            "Configuration override rc.yy:a b\n"
                            "Configuration override rc.xx='quoted value'\n")

    def test_override_verbose_nothing_suppresses_footnotes(self, sb, record):
        args = ["rc.verbose=nothing", "rc.zz=1", "version"]
        p = sb.run(args)
        record(p, args)
        assert p.returncode == 0
        assert p.stderr == ""

    def test_override_abbreviation_minimum(self, sb, record):
        """rc.abbreviation.minimum=1 lets `v` abbreviate `version`."""
        args = ["rc.verbose=nothing", "rc.abbreviation.minimum=1", "v"]
        p = sb.run(args)
        record(p, args)
        assert p.returncode == 0, p.stderr
        assert "task %s built for" % VERSION in p.stdout

    def test_override_default_command(self, sb, record):
        """Only rc overrides on the command line => default.command (set by override) runs."""
        args = ["rc.verbose=nothing", "rc.default.command=version", "rc.zz=a b"]
        p = sb.run(args)
        record(p, args)
        assert p.returncode == 0, p.stderr
        assert "task %s built for" % VERSION in p.stdout

    def test_rc_colon_file_with_overrides(self, sb, record):
        """rc:<file> selects the rc file and coexists with rc.<name>= overrides."""
        alt = sb.write_rc("data.location=%s\nzz=alt\nyy=alt\n" % sb.data,
                          path=sb.taskrc + ".alt")
        args = ["rc:" + alt, "rc.zz=cli", "_show"]
        p = sb.run(args)
        record(p, args, extra={"zz": show_value(p.stdout, "zz"), "yy": show_value(p.stdout, "yy")})
        assert p.returncode == 0, p.stderr
        assert show_value(p.stdout, "zz") == "cli"
        assert show_value(p.stdout, "yy") == "alt"


class TestNonPairArguments:
    @pytest.mark.parametrize("args,bad", [
        (["rc.zz='x'y", "version"], "rc.zz='x'y"),
        (["RC.zz=bar", "_show"], "RC.zz=bar"),
        (["rc.zz", "_show"], "rc.zz"),
        (["_show", "--", "rc.zz=bar"], "rc.zz=bar"),
        (["version", "foo"], "foo"),
    ])
    def test_non_pair_word_rejected(self, sb, record, args, bad):
        """Arguments the Lexer does not classify as an rc pair stay as words and are rejected."""
        p = sb.run(args)
        record(p, args)
        cmd = [a for a in args if a in ("version", "_show")][0]
        assert p.returncode == 2, (p.stdout, p.stderr)
        assert p.stdout == ""
        assert p.stderr.endswith("The '%s' command does not allow '%s'.\n" % (cmd, bad))
