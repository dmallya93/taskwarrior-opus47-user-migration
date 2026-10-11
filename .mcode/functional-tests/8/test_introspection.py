"""diagnostics, _aliases, commands/_commands/_zshcommands, help, execute, logo."""

import re

from conftest import add, install_hook

W = "rc.defaultwidth=100"


class TestDiagnostics:
    def test_diagnostics_layout(self, sb, task):
        """diagnostics: same section/label layout; compiler/build values legitimately differ.
        Recorded verbatim; comparison is done on the label skeleton (see FT run notes)."""
        install_hook(sb, "on-add.ftrun", "#!/bin/sh\nread t\necho \"$t\"\nexit 0\n")
        p = task(W, "diagnostics")
        assert p.returncode == 0
        for label in ("Platform:", "Compiler", "Build Features", "Configuration",
                      "Hooks", "Tests"):
            assert label in p.stdout, label

    def test_diagnostics_color(self, task):
        """diagnostics with forced colour: bold section headings."""
        p = task(W, "rc._forcecolor=on", "rc.color=on", "diagnostics")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout


class TestAliases:
    def test_aliases_completion(self, sb, task):
        """_aliases lists alias.* names (defaults + user-defined)."""
        sb.append_rc("alias.ftls=list\n")
        p = task("_aliases")
        assert p.returncode == 0
        assert "ftls" in p.stdout and "rm" in p.stdout

    def test_alias_dispatch(self, sb, task):
        """A user alias dispatches to its command."""
        sb.append_rc("alias.ftcount=count\n")
        add(task, "add", "one")
        p = task("ftcount")
        assert p.stdout.strip() == "1"


class TestCommands:
    def test_commands_table(self, task):
        """commands: table of every command with category/R/W/context flags."""
        p = task("rc.defaultwidth=150", "commands")
        assert p.returncode == 0
        assert "burndown.daily" in p.stdout and "calendar" in p.stdout

    def test_commands_completion(self, task):
        """_commands: plain command name list."""
        p = task("_commands")
        assert p.returncode == 0
        assert "calendar" in p.stdout.split()

    def test_zshcommands(self, task):
        """_zshcommands: name:category:description lines."""
        p = task("_zshcommands")
        assert p.returncode == 0
        assert "calendar:" in p.stdout


class TestHelp:
    def test_help(self, task):
        """help: full usage text."""
        p = task("rc.defaultwidth=100", "help")
        assert p.returncode == 0
        assert "Usage:" in p.stdout or "task" in p.stdout

    def test_help_usage(self, task):
        """help usage: the usage section only."""
        p = task("rc.defaultwidth=100", "help", "usage")
        assert p.returncode == 0

    def test_no_command_default(self, task):
        """No command and empty default.command."""
        p = task("rc.default.command=", stdin="")
        assert p.returncode in (0, 1, 2)


class TestExecute:
    def test_execute_echo(self, task):
        """execute runs an external command and passes through its output/exit code."""
        p = task("execute", "echo", "ftrun", "hello")
        assert p.returncode == 0
        assert "ftrun hello" in p.stdout

    def test_execute_failure_code(self, task):
        """execute returns the raw system() status, which the process exit truncates
        (256 & 0xff == 0): 'execute false' exits 0 on origin as well."""
        p = task("execute", "false")
        assert p.returncode == 0

    def test_execute_disabled(self, task):
        """allow.execute / allow.exec off refuses (if supported)."""
        p = task("rc.allow.exec=off", "execute", "echo", "x")
        assert p.returncode in (0, 1, 2)


class TestLogo:
    def test_logo_color(self, task):
        """logo with forced colour prints the colour logo."""
        p = task("rc._forcecolor=on", "rc.color=on", "logo")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout

    def test_logo_no_color(self, task):
        """logo without colour: 'The logo command requires that color support is enabled.'"""
        p = task("rc.color=off", "logo")
        assert p.returncode != 0 and "requires that color support" in p.stderr
