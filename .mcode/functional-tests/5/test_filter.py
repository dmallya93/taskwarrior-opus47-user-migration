"""Filter / CLI2 filter grammar: ID & UUID sequences, attribute modifiers,
tags, /pattern/, boolean operators, implicit `and`, status filtering."""

import pytest

from conftest import seed_basic


def ids(task, *filt):
    p = task("rc.verbose=nothing", *filt, "_ids")
    assert p.returncode == 0, p.stderr
    return [int(x) for x in p.stdout.split()]


def count(task, *filt):
    p = task("rc.verbose=nothing", *filt, "count")
    assert p.returncode == 0, p.stderr
    return int(p.stdout.strip())


class TestSequences:
    def test_id_sequences(self, task):
        """ID lists/ranges: `1-3,5`, `2 4`, `1,3`, `3-4`; non-existent ids select nothing."""
        for i in range(6):
            task("rc.verbose=nothing", "add", "t%d" % i, record=False)
        assert ids(task, "1-3,5") == [1, 2, 3, 5]
        assert ids(task, "2", "4") == [2, 4]
        assert ids(task, "1,3") == [1, 3]
        assert ids(task, "3-4") == [3, 4]
        assert ids(task, "6-9") == [6]
        assert ids(task, "42") == []
        assert count(task, "1-6") == 6

    def test_uuid_sequences(self, task):
        """Full and partial (>=8 hex chars) UUIDs select tasks; mixed with ids."""
        for i in range(3):
            task("rc.verbose=nothing", "add", "t%d" % i, record=False)
        u2 = task("_get", "2.uuid", record=False).stdout.strip()
        u3 = task("_get", "3.uuid", record=False).stdout.strip()
        assert ids(task, u2) == [2]
        assert ids(task, u2[:8]) == [2]
        assert ids(task, u2[:13]) == [2]
        # (a comma-joined `id,uuid` token is lexed as an expression in C++ whose result
        #  depends on the random UUID text, so it is not asserted here)
        assert ids(task, "1", u3) == [1, 3]
        assert ids(task, u2, u3) == [2, 3]


class TestModifiers:
    def test_attribute_modifiers(self, task):
        """before/after/below/above, is/isnt/equals, has/hasnt, startswith/endswith,
        none/any, not, word/noword on seed data."""
        seed_basic(task)
        assert ids(task, "due.before:2009-02-15") == [3]
        assert ids(task, "due.after:2009-02-15") == [1]
        assert ids(task, "due.below:2009-02-15") == [3]
        assert ids(task, "due.above:2009-02-15") == [1]
        assert ids(task, "due.none:") == [2, 4]
        assert ids(task, "due.any:") == [1, 3]
        assert ids(task, "project.is:home") == [1]
        assert ids(task, "project.equals:home") == [1]
        assert ids(task, "project.isnt:home") == [2, 3, 4]
        assert ids(task, "project.not:home") == [3, 4]
        assert ids(task, "project.startswith:ho") == [1, 2]
        assert ids(task, "project.endswith:den") == [2]
        assert ids(task, "description.has:ta") == [2, 4]
        assert ids(task, "description.contains:ta") == [2, 4]
        assert ids(task, "description.hasnt:e") == []
        assert ids(task, "description.word:two") == [2]
        assert ids(task, "description.noword:two") == [1, 3, 4]
        assert ids(task, "priority:H") == [1]
        assert ids(task, "pri.none:") == [2, 3, 4]

    def test_attribute_name_abbreviation_in_filter(self, task):
        """Attribute names may be abbreviated (proj:, desc.has:, pri:)."""
        seed_basic(task)
        assert ids(task, "proj:work") == [3]
        assert ids(task, "desc.has:gamma") == [3]
        assert ids(task, "pro.not:home") == [3, 4]

    def test_bad_modifier(self, task):
        """An unknown attribute modifier is an error."""
        seed_basic(task)
        p = task("rc.verbose=nothing", "project.bogus:home", "count")
        assert p.returncode == 2
        assert "Error: unrecognized attribute modifier 'bogus'." in p.stderr
        task("rc.verbose=nothing", "due.before:notadate", "count")


class TestTagsPatternsOperators:
    def test_tags(self, task):
        """+tag includes, -tag excludes, virtual tags (+PENDING, +COMPLETED, +DUE ...)."""
        seed_basic(task)
        assert ids(task, "+work") == [1]
        assert ids(task, "-work") == [2, 3, 4]
        assert ids(task, "+home") == [2]
        assert ids(task, "-home", "-work") == [3, 4]
        assert count(task, "+PENDING") == 4
        assert count(task, "+COMPLETED") == 1
        assert ids(task, "+OVERDUE") == [3]
        assert ids(task, "+TAGGED") == [1, 2]
        assert ids(task, "+PROJECT") == [1, 2, 3]

    def test_pattern(self, task):
        """/pattern/ matches description (regex off by default -> substring)."""
        seed_basic(task)
        assert ids(task, "/eta/") == [2]
        assert ids(task, "/a/") == [1, 2, 3, 4]
        assert ids(task, "/nomatch/") == []
        assert ids(task, "rc.regex=on", "/^g.*e$/") == [3]

    def test_boolean_operators(self, task):
        """Implicit and, explicit and/or/xor, parentheses, status filters."""
        seed_basic(task)
        assert ids(task, "project:home", "+work") == [1]
        assert ids(task, "project:home", "and", "+home") == [2]
        assert ids(task, "+work", "or", "+home") == [1, 2]
        assert ids(task, "(", "+work", "or", "project:work", ")", "and", "due.any:") == [1, 3]
        assert ids(task, "(+work or +home)") == [1, 2]
        assert ids(task, "project:home", "xor", "+home") == [1]
        assert count(task, "status:completed") == 1
        assert count(task, "status:pending") == 4
        assert count(task) == 5
        assert count(task, "description:alpha") == 1
        assert count(task, "description:'alpha one'") == 1

    def test_algebraic_expression(self, task):
        """Expressions with DOM refs and operators: due < 2009-02-15, urgency > 0."""
        seed_basic(task)
        assert ids(task, "due", "<", "2009-02-15") == [3]
        assert ids(task, "due<2009-02-15") == [3]
        assert ids(task, "id", ">", "2") == [3, 4]
        assert ids(task, "project", "==", "home") == [1]
        assert ids(task, "description", "~", "two") == [2]
