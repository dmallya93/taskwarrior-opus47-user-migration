"""calc binary: --postfix / --infix / --debug / --help / --version option handling.

C++ reference: src/calc.cpp (exit(1) for help/version, status -1 on error),
src/Eval.cpp (debug token dump, 'Eval ...' trace lines), test/calc.test.py.
"""

import re

import pytest

from conftest import faketime_env

VERSION = "3.5.0"

HELP = ("\n"
        "Usage: calc [options] '<expression>'\n"
        "\n"
        "Options:\n"
        "  -h|--help         Display this usage\n"
        "  -d|--debug        Debug mode\n"
        "  -i|--infix        Infix expression (default)\n"
        "  -p|--postfix      Postfix expression\n"
        "\n")


class TestCalcHelpVersion:
    @pytest.mark.parametrize("flag", ["--help", "-h"])
    def test_calc_help(self, sb, record, flag):
        """-h/--help prints usage (argv[0] in the Usage line) and exits 1."""
        args = [flag, "1+2"]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 1
        assert p.stdout == HELP
        assert p.stderr == ""

    @pytest.mark.parametrize("flag", ["--version", "-v"])
    def test_calc_version(self, sb, record, flag):
        """-v/--version prints 'calc 3.5.0 built for <os>' banner and exits 1."""
        args = [flag]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 1
        assert re.match(r"^\ncalc %s built for \S+\n" % re.escape(VERSION), p.stdout)
        assert "Copyright (C) 2006 - 2021 T. Babej, P. Beckingham, F. Hernandez.\n" in p.stdout
        assert p.stdout.endswith("which may be found in the Taskwarrior source kit.\n\n")


POSTFIX = [
    (["--postfix", "12 3600 * 34 60 * 56 + +"], "45296\n"),
    (["-p", "1 2 +"], "3\n"),
    (["-p", "10 3 -"], "7\n"),
    (["-p", "2 3 4 * +"], "14\n"),
    (["-p", "true false ||"], "true\n"),
    (["-p", "2009-02-13 1d +"], "2009-02-14T00:00:00\n"),
    (["-p", "-i", "1+2"], "3\n"),       # last of -p/-i wins
    (["-i", "-p", "3 4 *"], "12\n"),
]


class TestCalcPostfix:
    @pytest.mark.parametrize("args,expected", POSTFIX, ids=[" ".join(a) for a, _ in POSTFIX])
    def test_calc_postfix(self, sb, record, args, expected):
        """--postfix evaluates RPN expressions; -i/-p toggle (last wins)."""
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected
        assert p.stderr == ""

    def test_calc_postfix_insufficient_operands(self, sb, record):
        """Postfix with a dangling operator => 'The expression could not be evaluated.' exit 255."""
        args = ["-p", "1 +"]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 255
        assert p.stdout == ""
        assert p.stderr == "The expression could not be evaluated.\n"


FILTER = "\x1b[1;37;42mFILTER\x1b[0m"


class TestCalcDebug:
    def test_calc_debug_regular_math(self, sb, record):
        """calc --debug '12 * 3600 + 34 * 60 + 56' (test/calc.test.py::test_regular_math)."""
        args = ["--debug", "12 * 3600 + 34 * 60 + 56"]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0
        for n in ("12", "3600", "34", "60", "56"):
            assert "Eval literal number ↑'%s'" % n in p.stdout
        assert re.search(r"^45296$", p.stdout, re.M)
        assert "Error" not in p.stdout + p.stderr
        lines = p.stdout.splitlines()
        assert lines[0].startswith(FILTER + " Infix        ")
        assert lines[1].startswith(FILTER + " Infix parsed ")
        assert lines[2].startswith(FILTER + " Postfix      ")
        assert "Eval ↓'12' * ↓'3600' → ↑'43200'" in p.stdout

    def test_calc_debug_postfix(self, sb, record):
        """calc --debug --postfix: only the Postfix token dump, then Eval trace."""
        args = ["--debug", "--postfix", "12 3600 * 34 60 * 56 + +"]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0
        lines = p.stdout.splitlines()
        assert lines[0].startswith(FILTER + " Postfix      ")
        assert "Infix" not in p.stdout
        assert lines[-1] == "45296"

    def test_calc_debug_negative(self, sb, record):
        """calc --debug '2- -3' shows the _neg_ unary operator trace."""
        args = ["-d", "2- -3"]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0
        assert "Eval _neg_ ↓'3' → ↑'-3'" in p.stdout
        assert re.search(r"^5$", p.stdout, re.M)

    def test_calc_debug_duration(self, sb, record):
        """'15min' is a single duration literal (test/calc.test.py::test_duration)."""
        args = ["--debug", "15min"]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0
        assert "Eval literal duration ↑'PT15M'" in p.stdout
        assert re.search(r"^PT15M$", p.stdout, re.M)
        assert "Unexpected stack size" not in p.stdout + p.stderr

    @pytest.mark.parametrize("expr", ["now > yesterday", "'foo' + 'bar'", "!true",
                                      "2009-02-13 + 1d", "1.5 * 2"])
    def test_calc_debug_trace_exact(self, sb, record, expr):
        """Full --debug stdout for mixed-type expressions (compared byte-for-byte to origin)."""
        args = ["--debug", expr]
        env = faketime_env()
        p = sb.calc(args, env)
        record(p, "calc", args, env)
        assert p.returncode == 0, p.stderr
        assert p.stdout.splitlines()[0].startswith(FILTER + " Infix        ")

    def test_calc_debug_error(self, sb, record):
        """--debug with a failing expression: token dumps are lost (thrown before printing), error on stderr."""
        args = ["--debug", "12 / 0"]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 255
        assert p.stdout == ""
        assert p.stderr == "Cannot divide by zero\n"
