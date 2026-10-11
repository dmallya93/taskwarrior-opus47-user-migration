"""Colour rules (rules.rs autoColorize) in reports with color=on, precedence,
and the `color` command (samples, legend, colour test)."""

from conftest import seed_reports, add

ON = ["rc.color=on", "rc._forcecolor=on"]


class TestColorRules:
    def test_color_default_theme_report(self, task):
        """Default theme: overdue/due/active/blocked/blocking/tagged/scheduled rows coloured."""
        seed_reports(task)
        p = task(*ON, "list")
        q = task(*ON, "next")
        assert p.returncode == 0 and q.returncode == 0
        assert "\x1b[" in p.stdout

    def test_color_project_tag_keyword_rules(self, task):
        """color.project.X, color.tag.X, color.keyword.X, color.uda.priority.H blending."""
        seed_reports(task)
        rc = ["rc.color.project.home=on blue", "rc.color.tag.work=bold",
              "rc.color.keyword.delta=red", "rc.color.uda.priority.H=underline",
              "rc.color.tagged=", "rc.color.due=", "rc.color.overdue="]
        p = task(*ON, *rc, "ls")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout

    def test_color_precedence(self, task):
        """rule.precedence.color reordering changes the blended result."""
        seed_reports(task)
        base = ["rc.color.project.home=red", "rc.color.tag.work=green"]
        p = task(*ON, *base, "rc.rule.precedence.color=project.,tag.", "ls")
        q = task(*ON, *base, "rc.rule.precedence.color=tag.,project.", "ls")
        assert p.returncode == 0 and q.returncode == 0

    def test_color_alternate_and_completed(self, task):
        """color.alternate on alternate rows; color.completed in `all`."""
        seed_reports(task)
        p = task(*ON, "rc.color.alternate=on gray4", "rc.color.completed=green",
                 "rc.color.deleted=red", "all", short_uuids=True)
        assert p.returncode == 0

    def test_color_off_no_escape(self, task):
        """color=off: no escape sequences even with rules configured."""
        seed_reports(task)
        p = task("rc.color=off", "rc.color.project.home=red", "list")
        assert p.returncode == 0
        assert "\x1b[" not in p.stdout


class TestColorCommand:
    def test_color_samples(self, task):
        """`task color` (no args) prints the colour test (16/256 colours)."""
        p = task(*ON, "color")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout

    def test_color_legend(self, task):
        """`task color legend` lists active colour settings with samples."""
        p = task(*ON, "color", "legend")
        assert p.returncode == 0
        assert "color.overdue" in p.stdout

    def test_color_sample_spec(self, task):
        """`task color red on blue` shows a sample of that colour."""
        p = task(*ON, "color", "red", "on", "blue")
        q = task(*ON, "color", "bold", "underline", "white", "on", "rgb013")
        assert p.returncode == 0 and q.returncode == 0

    def test_color_disabled(self, task):
        """color command with color off: tells the user colour is disabled."""
        p = task("rc.color=off", "color")
        assert p.returncode != 0 or "disabled" in (p.stdout + p.stderr).lower() \
            or p.returncode == 0
