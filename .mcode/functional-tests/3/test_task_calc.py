"""`task calc <expression>` (CmdCalc).

C++ reference: src/commands/CmdCalc.cpp (rc.expressions=infix|postfix, rc.debug),
src/Context.cpp::run (errors -> stderr, exit 2), test/calc.test.py::TestBug1254,
test/partial.test.py. DOM references are out of scope (later milestone).
"""

import re

import pytest

from conftest import faketime_env


HAPPY = [
    (["calc", "1+2"], "3\n"),
    (["calc", "2^10"], "1024\n"),
    (["calc", "10/3"], "3\n"),
    (["calc", "7%3"], "1\n"),
    (["calc", "0*-1"], "0\n"),
    (["calc", "0*1"], "0\n"),
    (["calc", "12 * 3600 + 34 * 60 + 56"], "45296\n"),
    (["calc", "1.5+1"], "2.5\n"),
    (["calc", "2009-02-13 + 1d"], "2009-02-14T00:00:00\n"),
    (["calc", "15min"], "PT15M\n"),
    (["calc", "'foo' + 'bar'"], "'foo'bar\n"),
    (["calc", "1 < 2"], "true\n"),
]


class TestTaskCalc:
    @pytest.mark.parametrize("args,expected", HAPPY, ids=[" ".join(a) for a, _ in HAPPY])
    def test_task_calc_infix(self, sb, record, args, expected):
        """task calc <infix> prints the result, exit 0."""
        p = sb.task(args)
        record(p, "task", args)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected
        assert p.stderr == ""

    @pytest.mark.parametrize("args,expected", [
        (["calc", "today = now"], "true\n"),
        (["calc", "today == now"], "false\n"),
        (["calc", "now > yesterday"], "true\n"),
        (["calc", "now"], "2009-02-13T12:00:00\n"),
    ], ids=["today = now", "today == now", "now > yesterday", "now"])
    def test_task_calc_relative_dates(self, sb, record, args, expected):
        """test/partial.test.py: partial vs exact date match, frozen clock."""
        env = faketime_env()
        p = sb.task(args, env)
        record(p, "task", args, env)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected

    @pytest.mark.parametrize("args,expected", [
        (["rc.expressions=postfix", "calc", "1 2 +"], "3\n"),
        (["rc.expressions=postfix", "calc", "12 3600 * 34 60 * 56 + +"], "45296\n"),
        (["rc.expressions=infix", "calc", "1+2*3"], "7\n"),
    ], ids=["postfix 1 2 +", "postfix big", "infix explicit"])
    def test_task_calc_expressions_setting(self, sb, record, args, expected):
        """rc.expressions=postfix switches CmdCalc to RPN evaluation."""
        p = sb.task(args)
        record(p, "task", args)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected

    @pytest.mark.parametrize("args,expected", [
        (["calc", "12 / 0"], "Cannot divide by zero\n"),
        (["calc", "12 % 0"], "Cannot modulo zero\n"),
        (["calc"], "No expression to evaluate.\n"),
        (["calc", "1 +"], "The expression could not be evaluated.\n"),
        (["calc", "'foo' ^ 2"], "Cannot exponentiate strings\n"),
    ], ids=["12 / 0", "12 % 0", "<empty>", "1 +", "string exp"])
    def test_task_calc_errors(self, sb, record, args, expected):
        """Eval/Variant errors propagate through Context::run: message on stderr, exit 2."""
        p = sb.task(args)
        record(p, "task", args)
        assert p.returncode == 2
        assert p.stdout == ""
        assert p.stderr == expected

    def test_task_calc_debug(self, sb, record):
        """rc.debug=1: Eval token dumps and trace are emitted on stderr, result on stdout."""
        args = ["rc.debug=1", "rc.color=off", "calc", "2- -3"]
        p = sb.task(args)
        record(p, "task", args)
        assert p.returncode == 0, p.stderr
        assert p.stdout.endswith("5\n")
        assert "FILTER\x1b[0m Infix        " in p.stderr or "FILTER" in p.stderr
        assert "Eval _neg_ ↓'3' → ↑'-3'" in p.stderr
        assert "Eval ↓'2' - ↓'-3' → ↑'5'" in p.stderr

    @pytest.mark.parametrize("args,expected", [
        (["calc", "'foo' ~ 'f.o'"], "Unsupported operator '~'.\n"),
        (["calc", "'foo' !~ 'x'"], "Unsupported operator '!~'.\n"),
    ], ids=["match", "nomatch"])
    def test_task_calc_regex_needs_task_context(self, sb, record, args, expected):
        """CmdCalc has no current task, so Eval rejects ~ / !~: stderr message, exit 2."""
        p = sb.task(args)
        record(p, "task", args)
        assert p.returncode == 2
        assert p.stdout == ""
        assert p.stderr == expected
