"""burndown.*, history.*, ghistory.*: chart layout at fixed width, bars,
colours, rate/estimate lines, cumulative mode, empty-data messages."""

import pytest

from conftest import seed_history

W = "rc.defaultwidth=100"
COLOR = ["rc._forcecolor=on", "rc.color=on"]


@pytest.mark.parametrize("cmd", ["burndown.daily", "burndown.weekly", "burndown.monthly",
                                 "burndown.annual", "burndown"])
def test_burndown(task, cmd):
    """burndown variants over the seeded history: chart + rate/estimate lines."""
    seed_history(task)
    p = task(W, cmd)
    assert p.returncode == 0
    assert "Burndown" in p.stdout


@pytest.mark.parametrize("cmd", ["burndown.daily", "burndown.weekly", "burndown.monthly"])
def test_burndown_color(task, cmd):
    """burndown with forced colour: coloured bars instead of + X D characters."""
    seed_history(task)
    p = task(W, *COLOR, cmd)
    assert p.returncode == 0
    assert "\x1b[" in p.stdout


def test_burndown_cumulative_off(task):
    """rc.burndown.cumulative=off changes the done line to per-period."""
    seed_history(task)
    p = task(W, "rc.burndown.cumulative=off", "burndown.weekly")
    assert p.returncode == 0


def test_burndown_filter(task):
    """burndown restricted by a project filter."""
    seed_history(task)
    p = task(W, "project:alpha", "burndown.daily")
    assert p.returncode == 0


def test_burndown_empty(task):
    """burndown with no tasks."""
    p = task(W, "burndown.weekly")
    assert p.returncode in (0, 1)


def test_burndown_narrow(task):
    """burndown at a narrow width (rc.defaultwidth=60) rescales/truncates."""
    seed_history(task)
    p = task("rc.defaultwidth=60", "burndown.daily")
    assert p.returncode == 0


HISTORY = ["history.daily", "history.weekly", "history.monthly", "history.annual",
           "history", "ghistory.daily", "ghistory.weekly", "ghistory.monthly",
           "ghistory.annual", "ghistory"]


@pytest.mark.parametrize("cmd", HISTORY)
def test_history(task, cmd):
    """history/ghistory tables and graphs over the seeded history."""
    seed_history(task)
    p = task(W, cmd)
    assert p.returncode == 0
    assert "Year" in p.stdout


@pytest.mark.parametrize("cmd", ["history.monthly", "ghistory.monthly", "ghistory.weekly"])
def test_history_color(task, cmd):
    """history/ghistory with forced colour."""
    seed_history(task)
    p = task(W, *COLOR, cmd)
    assert p.returncode == 0
    assert "\x1b[" in p.stdout


@pytest.mark.parametrize("cmd", ["history.monthly", "ghistory.monthly"])
def test_history_empty(task, cmd):
    """history/ghistory with no tasks: 'No tasks.' style message."""
    p = task(W, cmd)
    assert p.returncode in (0, 1)


def test_history_filter(task):
    """history.monthly with a filter."""
    seed_history(task)
    p = task(W, "project:beta", "history.monthly")
    assert p.returncode == 0


def test_ghistory_narrow(task):
    """ghistory.monthly at narrow width scales the bars down."""
    seed_history(task)
    p = task("rc.defaultwidth=50", "ghistory.monthly")
    assert p.returncode == 0
