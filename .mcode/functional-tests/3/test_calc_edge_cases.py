"""calc binary: Variant/Eval edge cases (real formatting, integer wrap, casts, type errors).

Expected values are the C++ 3.5.0 oracle results (origin calc built from the reference
repo at the baseline ref), including its quirks: libshared format(double) 6 significant
digits, pow() results narrowed through int, Lexer word-splitting of unspaced '-',
quoted string literals keeping their quotes, and the Variant error texts.
"""

import pytest

EDGE = [
    ('1/3.0', 0, '0.333333\n', ''),
    ('2^-1', 255, '', 'The expression could not be evaluated.\n'),
    ('1e3 + 1', 0, '1001\n', ''),
    ('0.1+0.2', 0, '0.3\n', ''),
    ('2^63', 0, '-2147483648\n', ''),
    ('9223372036854775807 + 1', 0, '-9223372036854775808\n', ''),
    ('3 == 3.0', 0, 'true\n', ''),
    ("'3' == 3", 0, 'true\n', ''),
    ("'abc' = 'ABC'", 0, 'false\n', ''),
    ('1y / 12', 0, 'P30DT10H\n', ''),
    ('-1d', 0, '-P1D\n', ''),
    ('1d - 2d', 0, '-P1D\n', ''),
    ('1d * 1.5', 0, 'P1DT12H\n', ''),
    ('1d / 0', 255, '', 'Cannot divide by zero\n'),
    ('2009-02-13T12:34:56 + 1', 0, '2009-02-13T12:34:57\n', ''),
    ('1234567890 + 1d', 0, '2009-02-14T23:31:30\n', ''),
    ('2009-02-13 + 2009-02-13', 255, '', 'Cannot add two date values\n'),
    ('2009-02-13 * 2', 255, '', 'Cannot multiply date values\n'),
    ('1h > 59min', 0, 'true\n', ''),
    ('1h == 60min', 0, 'true\n', ''),
    ('3600 == 1h', 0, 'true\n', ''),
    ('!0', 0, 'true\n', ''),
    ("!''", 0, 'true\n', ''),
    ('1.5 ^ 2', 0, '2.25\n', ''),
    ('-2 ^ 2', 0, '-4\n', ''),
    ('- - 2', 255, '', 'The expression could not be evaluated.\n'),
    ('1 + +3', 0, '4\n', ''),
    ('2 * (3 + 4) ^ 2', 0, '98\n', ''),
    ('()', 255, '', 'No expression to evaluate.\n'),
    ('1 2', 255, '', 'The value is not an expression.\n'),
    ('5 % 3 * 2', 0, '4\n', ''),
    ('-5 % 3', 0, '-2\n', ''),
    ('-7 / 2', 0, '-3\n', ''),
    ('7.0 % -2', 0, '1\n', ''),
    ('100000000000 * 100000000000', 255, '', 'Cannot multiply date values\n'),
    ('1.7976931348623157e308 * 10', 0, 'inf\n', ''),
    ('2009-02-30', 255, '', 'Cannot subtract strings\n'),
    ('PT1H30M', 0, 'PT1H30M\n', ''),
    ("'foo' + 2009-02-13", 0, "'foo'2009-02-13T00:00:00\n", ''),
    ("'x' - 1", 255, '', 'Cannot subtract strings\n'),
    ("3 * 'ab'", 0, 'ababab\n', ''),
    ("2 / 'x'", 255, '', 'Cannot divide integer by string\n'),
    ('1d ^ 2', 255, '', 'Cannot exponentiate durations\n'),
    ('2009-02-13 % 1d', 255, '', 'Cannot modulo date values\n'),
    ('1h / 1d', 0, '0.0416667\n', ''),
    ('123456789.123456789', 0, '1.23457e+08\n', ''),
]


class TestCalcEdgeCases:
    @pytest.mark.parametrize("expr,code,out,err", EDGE, ids=[e for e, *_ in EDGE])
    def test_calc_edge_cases(self, sb, record, expr, code, out, err):
        """Edge-case expression yields exactly the C++ stdout/stderr/exit code."""
        args = [expr]
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == code, p.stderr
        assert p.stdout == out
        assert p.stderr == err
