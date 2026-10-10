"""calc binary: infix arithmetic, precedence, associativity, unary minus, real/int typing.

C++ reference: src/calc.cpp, src/Eval.cpp (operator table, infixToPostfix), src/Variant.cpp,
test/calc.test.py, test/eval_test.cpp.
"""

import pytest


ARITH = [
    # (expression argv, expected stdout)
    (["1+2"], "3\n"),
    (["2^10"], "1024\n"),
    (["10/3"], "3\n"),               # integer / integer stays integer
    (["7%3"], "1\n"),
    (["1+2*3"], "7\n"),              # * binds tighter than +
    (["(1+2)*3"], "9\n"),            # parentheses
    (["2^3^2"], "512\n"),            # ^ is right associative
    (["12 * 3600 + 34 * 60 + 56"], "45296\n"),
    (["12", "*", "3600", "+", "34", "*", "60", "+", "56"], "45296\n"),  # args are joined
    (["2- -3"], "5\n"),              # unary minus
    (["-5"], "-5\n"),
    (["0*-1"], "0\n"),               # bug 1254
    (["10 - 4 - 3"], "3\n"),         # - is left associative
    (["100/10/5"], "2\n"),
    (["1.5+1"], "2.5\n"),            # real + integer -> real
    (["10.0/4"], "2.5\n"),
    (["7.5%2"], "1.5\n"),            # real modulo uses fmod
    (["2*2.5"], "5\n"),
    (["3 - 5"], "-2\n"),
    (["3-5"], "3-5\n"),             # Lexer: unspaced '3-5' is one word (string), not 3 - 5
]


class TestCalcArithmetic:
    @pytest.mark.parametrize("args,expected", ARITH, ids=[" ".join(a) for a, _ in ARITH])
    def test_calc_arithmetic(self, sb, record, args, expected):
        """calc <infix expression> prints the evaluated result and exits 0."""
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 0, p.stderr
        assert p.stdout == expected
        assert p.stderr == ""


ERRORS = [
    (["12 / 0"], "Cannot divide by zero\n"),
    (["12 % 0"], "Cannot modulo zero\n"),
    (["1.5 / 0"], "Cannot divide by zero\n"),
    (["1.5 % 0.0"], "Cannot modulo zero\n"),
    (["10-4-3"], "Cannot subtract strings\n"),  # Lexer: '10-4-3' lexes as 10 - '4-3'
]


class TestCalcDivideByZero:
    @pytest.mark.parametrize("args,expected", ERRORS, ids=[" ".join(a) for a, _ in ERRORS])
    def test_calc_divide_by_zero(self, sb, record, args, expected):
        """Division / modulo by zero: error text on stderr, nothing on stdout, exit -1 (255)."""
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 255
        assert p.stdout == ""
        assert p.stderr == expected


MALFORMED = [
    ([], "No expression to evaluate.\n"),
    (["1 +"], "The expression could not be evaluated.\n"),
    (["*"], "The expression could not be evaluated.\n"),
    (["(1+2"], "Mismatched parentheses in expression\n"),
    (["1+2)"], "Mismatched parentheses in expression\n"),
]


class TestCalcMalformed:
    @pytest.mark.parametrize("args,expected", MALFORMED,
                             ids=[" ".join(a) or "<empty>" for a, _ in MALFORMED])
    def test_calc_malformed_expression(self, sb, record, args, expected):
        """Empty / malformed expressions: Eval error text on stderr, exit 255."""
        p = sb.calc(args)
        record(p, "calc", args)
        assert p.returncode == 255
        assert p.stdout == ""
        assert p.stderr == expected
