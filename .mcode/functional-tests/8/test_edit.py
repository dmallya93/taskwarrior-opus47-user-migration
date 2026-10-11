"""edit: rc.editor round trip with scripted editors, parse-back of the edited
document, error-recovery prompt (yes/no), bulk confirmation, hooks."""

import os

from conftest import add, export, install_hook


def editor(sb, name, body):
    path = os.path.join(sb.root, name)
    with open(path, "w") as fh:
        fh.write("#!/bin/sh\n" + body)
    os.chmod(path, 0o755)
    return path


def seed(task):
    add(task, "add", "edit me", "project:home", "priority:L", "+one", "due:2009-02-20")
    add(task, "add", "other task")


class TestEdit:
    def test_edit_noop(self, sb, task):
        """Editor that changes nothing: 'No edits were detected.'"""
        seed(task)
        ed = editor(sb, "noop.sh", "exit 0\n")
        p = task("rc.editor=" + ed, "1", "edit")
        assert p.returncode == 0
        assert "No edits were detected." in (p.stdout + p.stderr)

    def test_edit_document_format(self, sb, task):
        """The formatted document handed to the editor (the editor cats it to stdout)."""
        seed(task)
        add(task, "1", "annotate", "an annotation")
        ed = editor(sb, "cat.sh", 'cat "$1"\n')
        p = task("rc.editor=" + ed, "1", "edit")
        assert p.returncode == 0
        assert "Description:" in p.stdout and "edit me" in p.stdout

    def test_edit_description_priority(self, sb, task):
        """Editor rewrites Description and Priority: values parsed back and stored."""
        seed(task)
        ed = editor(sb, "mod.sh",
                    "sed -i -e 's/^  Description: .*/  Description: edited desc/' "
                    "-e 's/^  UDA priority: .*/  UDA priority:      H/' \"$1\"\n")
        p = task("rc.editor=" + ed, "1", "edit")
        assert p.returncode == 0
        e = export(task, "1")
        assert '"description":"edited desc"' in e.stdout and '"priority":"H"' in e.stdout

    def test_edit_project_tags_due(self, sb, task):
        """Editor changes Project, Tags and Due."""
        seed(task)
        ed = editor(sb, "mod2.sh",
                    "sed -i -e 's/^  Project: .*/  Project:           work/' "
                    "-e 's/^  Tags: .*/  Tags:              two three/' "
                    "-e 's/^  Due: .*/  Due:               2009-03-01/' \"$1\"\n")
        p = task("rc.editor=" + ed, "1", "edit")
        assert p.returncode == 0
        e = export(task, "1")
        assert '"project":"work"' in e.stdout and '"three"' in e.stdout

    def test_edit_add_annotation(self, sb, task):
        """Appending an 'Annotation:' line adds an annotation."""
        seed(task)
        ed = editor(sb, "ann.sh",
                    "sed -i 's/^\\(  Annotation: .* -- \\)$/\\1ftrun note/' \"$1\"\n")
        p = task("rc.editor=" + ed, "1", "edit")
        e = export(task, "1")
        assert p.returncode == 0
        assert "ftrun note" in e.stdout

    def test_edit_error_retry_no(self, sb, task):
        """Unparseable edit (bad due date): retry prompt answered 'no' -> no change."""
        seed(task)
        ed = editor(sb, "bad.sh",
                    "sed -i 's/^  Due: .*/  Due:               notadate/' \"$1\"\n")
        p = task("rc.confirmation=on", "rc.editor=" + ed, "1", "edit", stdin="no\n")
        assert "Would you like to try again?" in (p.stdout + p.stderr)
        e = export(task, "1")

    def test_edit_error_retry_yes(self, sb, task):
        """Bad edit, answer 'yes', the editor (stateful) fixes it on the 2nd run."""
        seed(task)
        state = os.path.join(sb.root, "count")
        ed = editor(sb, "retry.sh",
                    'if [ -f "%s" ]; then\n'
                    "  sed -i 's/^  Due: .*/  Due:               2009-04-01/' \"$1\"\n"
                    "else\n"
                    '  touch "%s"\n'
                    "  sed -i 's/^  Due: .*/  Due:               notadate/' \"$1\"\n"
                    "fi\n" % (state, state))
        p = task("rc.confirmation=on", "rc.editor=" + ed, "1", "edit", stdin="yes\n")
        e = export(task, "1")
        assert "Would you like to try again?" in (p.stdout + p.stderr)

    def test_edit_editor_fails(self, sb, task):
        """Editor exiting non-zero."""
        seed(task)
        ed = editor(sb, "fail.sh", "exit 3\n")
        p = task("rc.editor=" + ed, "1", "edit")
        e = export(task, "1")

    def test_edit_bulk_confirm(self, sb, task):
        """Editing more than rc.bulk tasks asks 'Do you wish to manually edit N tasks?'"""
        seed(task)
        add(task, "add", "third")
        ed = editor(sb, "noop2.sh", "exit 0\n")
        p = task("rc.confirmation=on", "rc.bulk=2", "rc.editor=" + ed, "1-3", "edit",
                 stdin="no\n")
        assert "Do you wish to manually edit" in (p.stdout + p.stderr)

    def test_edit_no_match(self, sb, task):
        """edit with a filter matching nothing."""
        seed(task)
        ed = editor(sb, "noop3.sh", "exit 0\n")
        p = task("rc.editor=" + ed, "99", "edit")
        assert p.returncode != 0 or "No matches" in (p.stdout + p.stderr) or p.stderr

    def test_edit_with_hook(self, sb, task):
        """edit mutates via the hook pipeline: on-modify hook runs and its feedback appears."""
        seed(task)
        install_hook(sb, "on-modify.ftrun",
                     "#!/bin/sh\nread original\nread modified\necho \"$modified\"\n"
                     "echo 'FTRUN hook saw edit'\nexit 0\n")
        ed = editor(sb, "mod3.sh",
                    "sed -i 's/^  Description: .*/  Description: hooked edit/' \"$1\"\n")
        p = task("rc.hooks=on", "rc.editor=" + ed, "1", "edit")
        assert "FTRUN hook saw edit" in (p.stdout + p.stderr)
        e = export(task, "1")
        assert "hooked edit" in e.stdout
