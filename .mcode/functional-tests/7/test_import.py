"""import (JSON array, line-delimited, stdin, UUID update vs add, errors) and
import-v2 (legacy pending.data/completed.data via TF2)."""

import os

from conftest import add, export, install_hook

U1 = "11111111-1111-4111-8111-111111111111"
U2 = "22222222-2222-4222-8222-222222222222"
U3 = "33333333-3333-4333-8333-333333333333"


def both(p):
    return p.stdout + p.stderr


def write(sb, name, text):
    path = os.path.join(sb.root, name)
    with open(path, "w") as fh:
        fh.write(text)
    return path


ARRAY = ('[{"uuid":"%s","description":"imp one","status":"pending","entry":"20090101T000000Z",'
         '"project":"imp","tags":["a","b"],"priority":"H"},'
         '{"uuid":"%s","description":"imp two","status":"completed","entry":"20090101T000000Z",'
         '"end":"20090105T000000Z","annotations":[{"entry":"20090102T000000Z","description":"ann"}]}]'
         % (U1, U2))


class TestImport:
    def test_import_array_file(self, sb, task):
        """Import a JSON array file: per-task 'add' lines and 'Imported 2 tasks.'"""
        f = write(sb, "a.json", ARRAY)
        p = task("import", f)
        assert p.returncode == 0
        assert "Imported 2 tasks." in both(p)
        e = export(task)
        assert U1 in e.stdout or "imp one" in e.stdout

    def test_import_line_delimited_stdin(self, task):
        """Line-delimited JSON objects on stdin (no file argument)."""
        text = ('{"uuid":"%s","description":"ld one","status":"pending"}\n'
                '{"uuid":"%s","description":"ld two","status":"pending","due":"20090301T000000Z"}\n'
                % (U1, U2))
        p = task("import", stdin=text)
        assert "Imported 2 tasks." in both(p)
        export(task)

    def test_import_dash_stdin(self, task):
        """'import -' reads stdin."""
        p = task("import", "-", stdin=ARRAY)
        assert "Imported 2 tasks." in both(p)

    def test_import_update_existing_uuid(self, sb, task):
        """Re-importing a UUID with changes reports 'mod', identical re-import reports 'skip'."""
        f = write(sb, "a.json", ARRAY)
        task("rc.verbose=nothing", "import", f, record=False)
        changed = ARRAY.replace('"priority":"H"', '"priority":"L"')
        g = write(sb, "b.json", changed)
        p = task("import", g)
        assert p.returncode == 0
        assert "Imported 2 tasks." in both(p)
        export(task)

    def test_import_invalid_json(self, sb, task):
        """Malformed JSON is an error with non-zero exit."""
        f = write(sb, "bad.json", '[{"description": "x",')
        p = task("import", f)
        assert p.returncode != 0

    def test_import_missing_file(self, task):
        """Nonexistent import file: 'File '...' not found.'"""
        p = task("import", "/nonexistent/file.json")
        assert p.returncode != 0
        assert "not found" in both(p)

    def test_import_no_uuid_and_depends(self, task):
        """Objects without uuid get one; depends as array of uuids is accepted."""
        text = ('[{"uuid":"%s","description":"base"},'
                '{"description":"child","depends":["%s"]}]' % (U1, U1))
        p = task("import", stdin=text)
        assert "Imported 2 tasks." in both(p)
        export(task)

    def test_import_empty_object(self, task):
        """An empty JSON object: 'Cannot import an empty task.' (non-object array
        elements are undefined behaviour in C++ - it segfaults - so not compared)."""
        p = task("import", stdin='{}')
        assert p.returncode != 0
        assert "Cannot import an empty task." in both(p)

    def test_import_runs_hooks(self, sb, task):
        """import runs on-add for new tasks and on-modify for updates."""
        install_hook(sb, "on-add-feedback", "#!/bin/sh\nread t\necho \"$t\"\necho 'ADD HOOK'\n")
        install_hook(sb, "on-modify-feedback",
                     "#!/bin/sh\nread a\nread b\necho \"$b\"\necho 'MODIFY HOOK'\n")
        p = task("import", stdin='{"uuid":"%s","description":"h one"}\n' % U3)
        q = task("import", stdin='{"uuid":"%s","description":"h one changed"}\n' % U3)
        assert "ADD HOOK" in both(p)
        assert "MODIFY HOOK" in both(q)


PENDING = """\
[description:"bing" due:"1734480000" entry:"1734397061" modified:"1734397061" status:"pending" uuid:"ad7f7585-bff3-4b57-a116-abfc9f71ee4a"]
[description:"baz" entry:"1734397063" modified:"1734397063" status:"pending" tags:"x,y" project:"p" uuid:"591ccfee-dd8d-44e9-908a-40618257cf54" annotation_1734397070:"an \\"quoted\\" note"]\
"""
COMPLETED = """\
[description:"foo" end:"1734397073" entry:"1734397054" modified:"1734397074" status:"deleted" uuid:"6849568f-55d7-4152-8db0-00356e39f0bb"]
[description:"bar" end:"1734397065" entry:"1734397056" modified:"1734397065" status:"completed" uuid:"51921813-7abb-412d-8ada-7c1417d01209"]\
"""


class TestImportV2:
    def test_import_v2(self, sb, task):
        """import-v2 reads pending.data/completed.data: 'Imported 4 tasks.'"""
        write(sb, "data/pending.data", PENDING)
        write(sb, "data/completed.data", COMPLETED)
        p = task("import-v2")
        assert "Imported 4 tasks" in both(p)
        export(task)
        task("rc.verbose=nothing", "list")

    def test_import_v2_no_files(self, task):
        """import-v2 with no legacy files."""
        p = task("import-v2")
        task("rc.verbose=nothing", "rc.json.array=on", "export")
        assert p.returncode in (0, 1, 2)

    def test_import_v2_existing_skipped(self, sb, task):
        """import-v2 twice: second run does not duplicate tasks."""
        write(sb, "data/pending.data", PENDING)
        write(sb, "data/completed.data", COMPLETED)
        task("rc.verbose=nothing", "import-v2", record=False)
        p = task("import-v2")
        e = export(task)
        assert e.stdout.count('"description":"bing"') == 1
        assert p.returncode in (0, 1)
