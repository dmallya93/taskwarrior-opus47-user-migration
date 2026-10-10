"""calc binary: Variant comparison/logical/string/date/duration operators and casts.

C++ reference: src/Variant.cpp (operator matrices, cast rules), test/variant_*_test.cpp,
test/partial.test.py. Dates are rendered in local time (TZ=UTC here) as ISO without zone.
"""

import pytest

from conftest import faketime_env


LOGIC = [
    (["1 < 2"], "true\n"),
    (["2 <= 1"], "false\n"),
    (["3 > 2.5"], "true\n"),
    (["2 >= 2"], "true\n"),
    (["1 == 1"], "true\n"),
    (["1 == 2"], "false\n"),
    (["1 !== 2"], "true\n"),
    (["1 != 1"], "false\n"),
    (["1 or 0"], "true\n"),
    (["1 < 2 and 2 < 3"], "true\n"),
    (["1 < 2 || 2 < 1"], "false\n"),   # C++ oracle result (Eval precedence quirk), kept verbatim
    (["true xor true"], "false\n"),
    (["!true"], "false\n"),
    (["! false"], "true\n"),
    (["1 < 2 && 2 < 3"], "true\n"),
    (["'abc' < 'abd'"], "true\n"),
    (["'foo' == 'foo'"], "true\n"),
    (["'foo' == 'FOO'"], "false\n"),
    (["'foobar' = 'foo'"], "true\n"),    # partial (left-anchored) match
    (["'foobar' == 'foo'"], "false\n"),
    (["'a' = 'aa'"], "false\n"),
]


class TestCalcLogic:
    @pytest.mark.parametrize("args,expected", LOGIC, ids=[" ".join(a) for a, _ in LOGIC])
    def test_calc_comparison_logic(self, sb, record, args, expected):
        """Comparison / logical operators return Boolean 'true'/'false', exit 0."""
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected
        assert p.stderr == ""


STRINGS = [
    # Quoted literals keep their quotes in the Variant string value (C++ behaviour).
    (["'foo' + 'bar'"], 0, "'foo'bar\n", ""),
    (["'foo' + 1"], 0, "'foo'1\n", ""),
    (["'foo' * 2"], 0, "'foo''foo'\n", ""),
    # 'true'/'false' are not Lexer literals: '&&' with identifier operands cannot be evaluated.
    (["true && false"], 255, "", "The expression could not be evaluated.\n"),
    (["1 && 0"], 255, "", "The expression could not be evaluated.\n"),
    (["1 || 0"], 255, "", "The expression could not be evaluated.\n"),
    (["'foo' ^ 2"], 255, "", "Cannot exponentiate strings\n"),
    (["true + true"], 255, "", "Cannot add two Boolean values\n"),
    (["true * 2"], 0, "2\n", ""),
    (["2 ^ 0.5"], 255, "", "Cannot exponentiate to a non-integer power\n"),
]


class TestCalcStringsAndTypeErrors:
    @pytest.mark.parametrize("args,code,out,err", STRINGS, ids=[" ".join(a) for a, *_ in STRINGS])
    def test_calc_string_ops_and_type_errors(self, sb, record, args, code, out, err):
        """String concatenation/repetition, and Variant type errors (exit 255 + message)."""
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == code, p.stderr
        assert p.stdout == out
        assert p.stderr == err


DATES = [
    (["2009-02-13 + 1d"], "2009-02-14T00:00:00\n"),
    (["2009-02-13", "+", "1d"], "2009-02-14T00:00:00\n"),
    (["2009-02-14 - 2009-02-13"], "P1D\n"),
    (["2009-02-13 - 1w"], "2009-02-06T00:00:00\n"),
    (["2009-02-13 > 2009-02-12"], "true\n"),
    (["15min"], "PT15M\n"),
    (["1d + 1h"], "P1DT1H\n"),
    (["2 * 1h"], "PT2H\n"),
    (["1d / 2"], "PT12H\n"),
    (["1w > 1d"], "true\n"),
]


class TestCalcDates:
    @pytest.mark.parametrize("args,expected", DATES, ids=[" ".join(a) for a, _ in DATES])
    def test_calc_date_duration_math(self, sb, record, args, expected):
        """Date +/- duration, date - date, duration arithmetic and comparisons."""
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected
        assert p.stderr == ""


RELATIVE = [
    (["now > yesterday"], "true\n"),
    (["now < tomorrow"], "true\n"),
    (["today = now"], "true\n"),     # partial date match: same day
    (["today == now"], "false\n"),   # exact: midnight != noon
    (["now"], "2009-02-13T12:00:00\n"),
    (["today"], "2009-02-13T00:00:00\n"),
    (["tomorrow - today"], "P1D\n"),
    (["now + 1d"], "2009-02-14T12:00:00\n"),
]


class TestCalcRelativeDates:
    @pytest.mark.parametrize("args,expected", RELATIVE, ids=[" ".join(a) for a, _ in RELATIVE])
    def test_calc_relative_dates(self, sb, record, args, expected):
        """Named dates (now/today/yesterday/tomorrow) with time frozen at 2009-02-13T12:00:00Z."""
        env = faketime_env()
        p = sb.calc(args, env)
        record(p, "calc", args, env)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected
        assert p.stderr == ""


REGEX = [
    ["'foo' ~ 'f.o'"],
    ["'foo' !~ 'x'"],
]


class TestCalcRegexWithoutTask:
    @pytest.mark.parametrize("args", REGEX, ids=[" ".join(a) for a in REGEX])
    def test_calc_regex_needs_task_context(self, sb, record, args):
        """~ / !~ need a current task in Eval; standalone calc reports Unsupported operator."""
        p = sb.calc(args)
        record(p, "calc", args)
        op = "~" if "!~" not in args[0] else "!~"
        assert p.returncode == 255
        assert p.stdout == ""
        assert p.stderr == "Unsupported operator '%s'.\n" % op
