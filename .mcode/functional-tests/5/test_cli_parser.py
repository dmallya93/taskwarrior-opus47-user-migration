"""CLI2 parser and Context dispatch: command abbreviation, abbreviation.minimum,
alias expansion, rc:/rc. overrides, default command, header/error output,
exit codes, rc.debug.parser dumps."""

from conftest import seed_basic


class TestAbbreviation:
    def test_command_abbreviation(self, task):
        """Unambiguous command prefixes >= abbreviation.minimum dispatch (cou -> count, expo)."""
        seed_basic(task)
        assert task("cou").stdout == "5\n"
        # 'co' is ambiguous (columns/commands/config/context/count) -> not a command; it
        # becomes a filter word for the default command
        p = task("rc.default.command=count", "co")
        assert p.returncode == 0 and p.stdout == "0\n"
        p = task("rc.verbose=nothing", "expo", "rc.json.array=off")
        assert p.returncode == 0 and len(p.stdout.splitlines()) == 5
        assert task("_uniq", "project").stdout == "home\nhome.garden\nwork\n"

    def test_abbreviation_minimum(self, task):
        """rc.abbreviation.minimum controls the shortest accepted prefix."""
        seed_basic(task)
        # 'proj' (4 chars) is below the minimum -> not an attribute, plain filter word
        p = task("rc.abbreviation.minimum=5", "count", "proj:work")
        assert p.returncode == 0 and p.stdout == "0\n"
        assert task("rc.abbreviation.minimum=5", "count", "proje:work").stdout == "1\n"
        # 'coun' below the minimum -> not a command -> default command
        q = task("rc.abbreviation.minimum=5", "rc.default.command=_ids", "rc.verbose=nothing",
                 "coun")
        assert q.returncode == 0 and q.stdout == "\n"
        r = task("rc.abbreviation.minimum=1", "rc.verbose=nothing", "pro:work", "cou")
        assert r.returncode == 0 and r.stdout == "1\n"


class TestAliases:
    def test_alias_expansion(self, task):
        """alias.X=... is expanded in place (incl. filters and multi-word aliases)."""
        seed_basic(task)
        p = task("rc.alias.howmany=count", "howmany")
        assert p.returncode == 0 and p.stdout == "5\n"
        q = task("rc.alias.homework=project:home count", "homework")
        assert q.returncode == 0 and q.stdout == "3\n"
        r = task("rc.alias.n=_ids", "+work", "n")
        assert r.stdout == "1\n"


class TestOverrides:
    def test_rc_overrides(self, task, sb):
        """rc.X=Y and rc.X:Y overrides; rc:<file> alternate rc file; header lines."""
        seed_basic(task)
        p = task("rc.json.array:off", "rc.verbose=nothing", "export")
        assert not p.stdout.startswith("[")
        q = task("rc.json.array=off", "export", "limit:1")
        assert "Configuration override rc.json.array=off" in q.stderr
        import os
        alt = os.path.join(sb.root, "alt.rc")
        with open(alt, "w") as fh:
            fh.write("data.location=%s\nconfirmation=off\nalias.zz=count\n" % sb.data)
        r = task("rc:" + alt, "zz")
        assert r.returncode == 0 and r.stdout == "5\n"
        assert "TASKRC override" in r.stderr

    def test_default_command_and_errors(self, task):
        """Unknown command words become a filter for the default command; helper output."""
        seed_basic(task)
        p = task("rc.default.command=count")
        assert p.returncode == 0 and p.stdout == "5\n"
        q = task("rc.default.command=count", "+work")
        assert q.stdout == "1\n"

    def test_debug_parser(self, task):
        """rc.debug.parser=1 dumps the final parse tree; =2 adds every CLI2 pass (analyze, prepareFilter, ...)."""
        seed_basic(task)
        p = task("rc.debug.parser=1", "project:home", "count")
        assert p.returncode == 0
        assert p.stdout.endswith("3\n")
        assert "Parse Tree (before command-specif" in p.stderr
        assert "_original_args" in p.stderr and "BINARY" in p.stderr
        q = task("rc.debug.parser=2", "+work", "or", "1-2", "count")
        assert q.returncode == 0 and q.stdout.endswith("2\n")
        assert "CLI2::analyze" in q.stderr
        assert "CLI2::prepareFilter" in q.stderr
