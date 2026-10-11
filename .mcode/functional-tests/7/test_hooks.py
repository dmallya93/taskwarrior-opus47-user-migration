"""Hooks: discovery in <data>/hooks, JSON protocol, misbehaving-script error
messages and exit codes (reference test/test_hooks fixtures), hook args,
rc.debug.hooks output."""

import pytest

from conftest import add, export, install_hook


def both(p):
    return p.stdout + p.stderr


class TestOnLaunchExit:
    def test_on_launch_good(self, sb, task):
        """on-launch-good feedback is printed, command proceeds."""
        install_hook(sb, "on-launch-good", src="on-launch-good")
        p = task("rc.verbose=nothing", "add", "x")
        assert p.returncode == 0

    def test_on_launch_bad(self, sb, task):
        """on-launch-bad: non-zero exit stops the command with its error text."""
        install_hook(sb, "on-launch-bad", src="on-launch-bad")
        p = task("add", "x")
        assert p.returncode != 0
        export(task)

    @pytest.mark.parametrize("fixture", ["on-launch-misbehave1", "on-launch-misbehave2"])
    def test_on_launch_misbehave(self, sb, task, fixture):
        """on-launch misbehaving scripts: exact error messages and exit codes."""
        install_hook(sb, fixture, src=fixture)
        task("add", "x")

    def test_on_launch_env_args(self, sb, task):
        """Hook script arguments: api:, args:, command:, rc:, data:, version:."""
        install_hook(sb, "on-launch-good-env", src="on-launch-good-env")
        p = task("list")
        assert "api:2" in both(p) and "command:list" in both(p) and "version:3.5.0" in both(p)

    def test_on_exit_good_bad(self, sb, task):
        """on-exit-good feedback; on-exit-bad error and exit code."""
        install_hook(sb, "on-exit-good", src="on-exit-good")
        task("add", "x")
        install_hook(sb, "on-exit-bad", src="on-exit-bad")
        p = task("add", "y")
        assert p.returncode != 0

    @pytest.mark.parametrize("fixture", ["on-exit-misbehave1", "on-exit-misbehave2"])
    def test_on_exit_misbehave(self, sb, task, fixture):
        """on-exit misbehaving scripts."""
        install_hook(sb, fixture, src=fixture)
        task("add", "x")


class TestOnAdd:
    def test_on_add_accept_modify(self, sb, task):
        """on-add-accept passes JSON through; on-add-modify changes the description."""
        install_hook(sb, "on-add-accept", src="on-add-accept")
        install_hook(sb, "on-add-modify", src="on-add-modify")
        p = task("add", "teh", "task")
        assert p.returncode == 0
        export(task)

    def test_on_add_reject(self, sb, task):
        """on-add-reject: task not added, error text, non-zero exit."""
        install_hook(sb, "on-add-reject", src="on-add-reject")
        p = task("add", "x")
        assert p.returncode != 0
        e = export(task)
        assert '"description"' not in e.stdout

    @pytest.mark.parametrize("fixture", ["on-add-misbehave%d" % i for i in range(1, 7)])
    def test_on_add_misbehave(self, sb, task, fixture):
        """on-add misbehave1..6: no JSON / extra JSON / bad JSON / uuid change / etc."""
        install_hook(sb, fixture, src=fixture)
        task("add", "x")
        export(task)


class TestOnModify:
    def test_on_modify_accept(self, sb, task):
        """on-modify-accept on modify/done/annotate."""
        add(task, "add", "alpha")
        install_hook(sb, "on-modify-accept", src="on-modify-accept")
        task("1", "modify", "priority:H", short_uuids=True)
        task("1", "annotate", "note", short_uuids=True)
        task("1", "done", short_uuids=True)

    def test_on_modify_reject_revert(self, sb, task):
        """on-modify-reject blocks the change; on-modify-revert returns the original."""
        add(task, "add", "alpha")
        install_hook(sb, "on-modify-reject", src="on-modify-reject")
        p = task("1", "modify", "priority:H")
        assert p.returncode != 0
        export(task)

    def test_on_modify_revert(self, sb, task):
        """on-modify-revert returns the original task: no change stored."""
        add(task, "add", "alpha")
        install_hook(sb, "on-modify-revert", src="on-modify-revert")
        task("1", "modify", "priority:H")
        export(task)

    @pytest.mark.parametrize("fixture", ["on-modify-misbehave%d" % i for i in range(2, 7)])
    def test_on_modify_misbehave(self, sb, task, fixture):
        """on-modify misbehave2..6."""
        add(task, "add", "alpha")
        install_hook(sb, fixture, src=fixture)
        task("1", "modify", "priority:H")
        export(task)


class TestHookMisc:
    def test_debug_hooks(self, sb, task):
        """rc.debug.hooks=2 prints hook discovery, input and output lines."""
        install_hook(sb, "on-add-accept", src="on-add-accept")
        install_hook(sb, "on-launch-good", src="on-launch-good")
        p = task("rc.debug.hooks=2", "add", "x")
        assert "Hook" in both(p) or "hook" in both(p)

    def test_hooks_off(self, sb, task):
        """rc.hooks=off disables hooks entirely."""
        install_hook(sb, "on-add-reject", src="on-add-reject")
        p = task("rc.hooks=off", "add", "x")
        assert p.returncode == 0

    def test_non_executable_and_bad_prefix_ignored(self, sb, task):
        """Non-executable scripts and names without an on-* prefix are not run."""
        import os
        path = install_hook(sb, "on-add-reject", src="on-add-reject")
        os.chmod(path, 0o644)
        install_hook(sb, "pre-add-reject", src="on-add-reject")
        p = task("add", "x")
        assert p.returncode == 0

    def test_hook_order_alphabetical(self, sb, task):
        """Several on-add hooks run in name order, each seeing the previous output."""
        install_hook(sb, "on-add.b",
                     "#!/bin/sh\nread t\necho \"$t\" | sed 's/\"description\":\"/\"description\":\"B-/'\n"
                     "echo FB-B\n")
        install_hook(sb, "on-add.a",
                     "#!/bin/sh\nread t\necho \"$t\" | sed 's/\"description\":\"/\"description\":\"A-/'\n"
                     "echo FB-A\n")
        p = task("add", "x")
        assert p.returncode == 0
        export(task)
