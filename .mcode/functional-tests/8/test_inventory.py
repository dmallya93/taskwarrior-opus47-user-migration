"""projects/_projects, tags/_tags, udas/_udas (incl. orphan UDAs)."""

from conftest import add, seed_reports, seed_history, make_tc_task

W = "rc.defaultwidth=100"


class TestProjects:
    def test_projects(self, task):
        """projects: hierarchical project list with task counts."""
        seed_reports(task)
        p = task(W, "projects")
        assert p.returncode == 0
        assert "home" in p.stdout and "garden" in p.stdout

    def test_projects_completion(self, task):
        """_projects: plain project list."""
        seed_reports(task)
        p = task("_projects")
        assert p.returncode == 0
        assert "home.garden" in p.stdout

    def test_projects_list_all(self, task):
        """projects with list.all.projects=on includes completed-only projects."""
        seed_history(task)
        add(task, "log", "closed one", "project:closed")
        p = task(W, "rc.list.all.projects=on", "projects")
        q = task(W, "projects")
        assert p.returncode == 0 and "closed" in p.stdout

    def test_projects_empty(self, task):
        """projects with no tasks: 'No projects.'"""
        p = task(W, "projects")
        assert "No projects." in (p.stdout + p.stderr)


class TestTags:
    def test_tags(self, task):
        """tags: tag counts table, including virtual-free tags."""
        seed_reports(task)
        p = task(W, "tags")
        assert p.returncode == 0
        assert "work" in p.stdout

    def test_tags_completion(self, task):
        """_tags: tag list including the virtual tags."""
        seed_reports(task)
        p = task("_tags")
        assert p.returncode == 0
        assert "work" in p.stdout and "PENDING" in p.stdout

    def test_tags_color(self, task):
        """tags with colour."""
        seed_reports(task)
        add(task, "add", "nextone", "+next")
        p = task(W, "rc._forcecolor=on", "rc.color=on", "tags")
        assert p.returncode == 0

    def test_tags_empty(self, task):
        """tags with no tags: 'No tags.'"""
        add(task, "add", "plain")
        p = task(W, "tags")
        assert "No tags." in (p.stdout + p.stderr)


class TestUdas:
    def test_udas(self, sb, task):
        """udas: defined UDAs table with type/label/allowed values/usage count."""
        sb.append_rc("uda.estimate.type=numeric\nuda.estimate.label=Est\n"
                     "uda.size.type=string\nuda.size.values=S,M,L\nuda.size.label=Size\n")
        add(task, "add", "one", "estimate:3", "size:M")
        add(task, "add", "two", "size:L")
        p = task(W, "udas")
        assert p.returncode == 0
        assert "estimate" in p.stdout and "S,M,L" in p.stdout

    def test_udas_orphans(self, sb, task):
        """udas lists orphan UDAs (attributes on tasks with no uda.* definition)."""
        make_tc_task(sb, status="pending", description="orphaned", orphanattr="zzz",
                     entry="1234500000", modified="1234500000")
        p = task(W, "udas")
        assert p.returncode == 0
        assert "orphanattr" in p.stdout

    def test_udas_completion(self, sb, task):
        """_udas: plain UDA name list."""
        sb.append_rc("uda.estimate.type=numeric\nuda.size.type=string\n")
        p = task("_udas")
        assert p.returncode == 0
        assert "estimate" in p.stdout and "size" in p.stdout

    def test_udas_none(self, task):
        """udas with only the built-in priority UDA: '1 UDA defined'."""
        p = task(W, "udas")
        assert "1 UDA defined" in p.stdout
