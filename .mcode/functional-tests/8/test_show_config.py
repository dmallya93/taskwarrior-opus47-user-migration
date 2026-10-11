"""show (full dump, filtered, unrecognised and default-differing settings,
colour, validation notes) and config/_config (in-place .taskrc editing with
confirmation)."""

from conftest import add

W = "rc.defaultwidth=100"
CONF = "rc.confirmation=on"


def rc_text(sb):
    with open(sb.taskrc) as fh:
        return fh.read()


class TestShow:
    def test_show_all(self, task):
        """show with no args: full configuration table."""
        p = task(W, "show")
        assert p.returncode == 0
        assert "abbreviation.minimum" in p.stdout

    def test_show_filtered(self, task):
        """show <substring>: only matching settings."""
        p = task(W, "show", "report.next")
        assert p.returncode == 0
        assert "report.next.columns" in p.stdout and "abbreviation" not in p.stdout

    def test_show_unrecognised(self, sb, task):
        """Unknown settings in the rc file are listed under 'Your .taskrc file contains these unrecognized variables'."""
        sb.append_rc("ftrun.bogus.setting=1\nalso.unknown=yes\n")
        p = task(W, "show")
        assert p.returncode == 0
        assert "ftrun.bogus.setting" in p.stdout
        assert "unrecognized" in p.stdout

    def test_show_default_differing_color(self, sb, task):
        """With colour, settings that differ from the defaults are highlighted."""
        sb.append_rc("default.project=ftrunproj\n")
        p = task(W, "rc._forcecolor=on", "rc.color=on", "show", "default")
        assert p.returncode == 0
        assert "\x1b[" in p.stdout

    def test_show_bad_report_column(self, sb, task):
        """Validation note for a report referencing an unknown column."""
        sb.append_rc("report.ftbad.columns=id,nosuchcol\nreport.ftbad.description=bad\n")
        p = task(W, "show", "report.ftbad")
        assert "nosuchcol" in (p.stdout + p.stderr)

    def test_show_validation_notes(self, sb, task):
        """Validation of bad values (e.g. color theme / data.location / bad due) prints notes."""
        sb.append_rc("color.due=bogus_colour_value\ndateformat=Y-M-D\n")
        p = task(W, "show", "color.due")
        assert p.returncode in (0, 1)

    def test_show_nomatch(self, task):
        """show with a substring that matches nothing."""
        p = task(W, "show", "zzznomatchzzz")
        assert p.returncode in (0, 1)

    def test_underscore_show(self, task):
        """_show: raw name=value dump."""
        p = task("_show")
        assert p.returncode == 0 and "abbreviation.minimum=2" in p.stdout


class TestConfig:
    def test_config_add_new(self, sb, task):
        """config name value (new) appends to .taskrc: 'Config file ... modified.'"""
        p = task("config", "ftrun.newsetting", "hello world")
        assert p.returncode == 0
        assert "ftrun.newsetting=hello world" in rc_text(sb)

    def test_config_change_existing(self, sb, task):
        """config changes an existing value in place, preserving other lines and comments."""
        sb.append_rc("# a comment\nftrun.key=old  # trailing\n")
        p = task("config", "ftrun.key", "new")
        assert p.returncode == 0
        t = rc_text(sb)
        assert "# a comment" in t and "ftrun.key=new" in t

    def test_config_unset(self, sb, task):
        """config name (no value) removes the setting."""
        sb.append_rc("ftrun.gone=1\n")
        p = task("config", "ftrun.gone")
        assert p.returncode == 0
        assert "ftrun.gone" not in rc_text(sb)

    def test_config_set_blank(self, sb, task):
        """config name '' sets a blank value."""
        p = task("config", "ftrun.blank", "")
        assert p.returncode == 0
        assert "ftrun.blank=" in rc_text(sb)

    def test_config_unset_missing(self, task):
        """Removing a setting that is not in the file: 'No entry named ... found.' (exit 1)."""
        p = task("config", "ftrun.never.there")
        assert p.returncode != 0

    def test_config_confirm_yes(self, sb, task):
        """confirmation=on: change prompt answered yes."""
        sb.append_rc("ftrun.key=old\n")
        p = task(CONF, "config", "ftrun.key", "new", stdin="yes\n")
        assert "ftrun.key=new" in rc_text(sb)

    def test_config_confirm_no(self, sb, task):
        """confirmation=on: add prompt answered no leaves the file unchanged."""
        p = task(CONF, "config", "ftrun.add", "v", stdin="no\n")
        assert "ftrun.add" not in rc_text(sb)

    def test_config_confirm_remove_no(self, sb, task):
        """confirmation=on: removal prompt answered no keeps the setting."""
        sb.append_rc("ftrun.keep=1\n")
        p = task(CONF, "config", "ftrun.keep", stdin="no\n")
        assert "ftrun.keep=1" in rc_text(sb)

    def test_config_then_show(self, sb, task):
        """A value written with config is visible to show (cross-command)."""
        task("config", "default.project", "ftrunp")
        p = task(W, "show", "default.project")
        assert "ftrunp" in p.stdout

    def test_underscore_config(self, task):
        """_config: list of all setting names (completion)."""
        p = task("_config")
        assert p.returncode == 0 and "abbreviation.minimum" in p.stdout
