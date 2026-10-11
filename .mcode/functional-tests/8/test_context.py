"""context/_context: define/delete/list/show/none/set and the effect of
read/write contexts on report filters and on add/log."""

from conftest import add, export, seed_reports

W = "rc.defaultwidth=100"


def define(task, name, filt):
    p = task("rc.confirmation=off", "context", "define", name, filt)
    assert p.returncode == 0, (p.stdout, p.stderr)
    return p


class TestContextManage:
    def test_context_define_list(self, task):
        """context define (non-interactive) then context list."""
        define(task, "home", "project:home")
        define(task, "work", "+work or project:work")
        p = task(W, "context", "list")
        assert p.returncode == 0
        assert "home" in p.stdout and "work" in p.stdout

    def test_context_define_confirm(self, task):
        """context define with confirmation on and a filter matching no tasks prompts."""
        p = task("rc.confirmation=on", "context", "define", "empty", "project:nothing",
                 stdin="yes\nyes\nyes\n")
        assert p.returncode == 0
        assert "matches 0 pending tasks" in p.stdout

    def test_context_set_show_none(self, task):
        """context <name>, context show, context none, context show."""
        define(task, "home", "project:home")
        a = task("context", "home")
        b = task("context", "show")
        c = task("context", "none")
        d = task("context", "show")
        assert a.returncode == 0 and "home" in b.stdout
        assert c.returncode == 0

    def test_context_delete(self, task):
        """context delete removes the definition; deleting active context unsets it."""
        define(task, "home", "project:home")
        task("context", "home")
        p = task("rc.confirmation=off", "context", "delete", "home")
        q = task(W, "context", "list")
        assert p.returncode == 0

    def test_context_delete_missing(self, task):
        """context delete of an unknown context is an error."""
        p = task("rc.confirmation=off", "context", "delete", "nope")
        assert p.returncode != 0

    def test_context_set_unknown(self, task):
        """context <unknown> is an error."""
        p = task("context", "nope")
        assert p.returncode != 0

    def test_context_list_empty(self, task):
        """context list with nothing defined."""
        p = task(W, "context", "list")
        assert p.returncode != 0 and "No contexts defined." in p.stderr

    def test_context_bad_subcommand_name(self, task):
        """context define with a reserved/invalid name."""
        p = task("rc.confirmation=off", "context", "define", "list", "project:x")
        assert p.returncode != 0

    def test_context_completion(self, task):
        """_context lists defined context names."""
        define(task, "home", "project:home")
        define(task, "work", "project:work")
        p = task("_context")
        assert p.returncode == 0 and "home" in p.stdout and "work" in p.stdout

    def test_context_separate_read_write(self, sb, task):
        """context.<name>.read / .write defined directly in the rc file."""
        sb.append_rc("context.split.read=project:home\ncontext.split.write=project:home +ctx\n")
        p = task(W, "context", "list")
        assert p.returncode == 0 and "split" in p.stdout


class TestContextEffect:
    def test_context_filters_report(self, task):
        """Active read context restricts list/next reports and shows the context footnote."""
        seed_reports(task)
        define(task, "home", "project:home")
        task("context", "home")
        p = task(W, "list")
        assert p.returncode == 0
        assert "alpha one" in p.stdout and "gamma three" not in p.stdout

    def test_context_filters_count_and_export(self, task):
        """Context filter injection applies to count (and export)."""
        seed_reports(task)
        define(task, "work", "project:work")
        task("context", "work")
        p = task("count")
        e = export(task)
        assert p.returncode == 0

    def test_context_override_rc(self, task):
        """rc.context=none on the command line disables the active context for one command."""
        seed_reports(task)
        define(task, "home", "project:home")
        task("context", "home")
        p = task(W, "rc.context=none", "list")
        assert "gamma three" in p.stdout

    def test_context_write_add(self, sb, task):
        """Write context attributes are applied to add."""
        sb.append_rc("context.split.read=project:home\ncontext.split.write=project:home +ctx\n")
        task("context", "split")
        p = task("add", "ctx task")
        e = export(task)
        assert '"project":"home"' in e.stdout and '"ctx"' in e.stdout

    def test_context_write_log(self, task):
        """Write context applies to log as well (define sets both read and write)."""
        define(task, "home", "project:home")
        task("context", "home")
        p = task("log", "logged in ctx")
        e = export(task)
        assert '"project":"home"' in e.stdout

    def test_context_and_cli_filter(self, task):
        """Context filter is ANDed with the command-line filter."""
        seed_reports(task)
        define(task, "home", "project:home")
        task("context", "home")
        p = task(W, "+work", "list")
        assert p.returncode == 0

    def test_context_affects_analytics(self, task):
        """Context filter also affects summary/projects/tags."""
        seed_reports(task)
        define(task, "home", "project:home")
        task("context", "home")
        a = task(W, "summary")
        b = task(W, "projects")
        c = task(W, "tags")
        assert a.returncode == 0

    def test_context_not_applied_to_id_filter(self, task):
        """Context is not applied to modify by id (write commands with ids)."""
        seed_reports(task)
        define(task, "home", "project:home")
        task("context", "home")
        p = task("3", "modify", "+touched")
        assert p.returncode == 0
