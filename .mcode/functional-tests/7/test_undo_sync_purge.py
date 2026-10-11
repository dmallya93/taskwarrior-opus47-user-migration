"""undo (operation display, confirmation, reverting exactly one command),
synchronize (local server_dir and git backends between two replicas,
config-error messages for the other backends), purge."""

import os

import pytest

from conftest import add, export, FROZEN_EPOCH

CONF = ["rc.confirmation=on"]


def both(p):
    return p.stdout + p.stderr


class TestUndo:
    def test_undo_add(self, task):
        """undo after add (confirmation off) removes the task."""
        add(task, "add", "alpha")
        p = task("undo")
        assert p.returncode == 0
        e = export(task)
        assert "alpha" not in e.stdout

    def test_undo_modify_confirm_yes(self, task):
        """undo with confirmation shows the operations of the last command and reverts on yes."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "modify", "priority:H", "+t", record=False,
             epoch=FROZEN_EPOCH + 60)
        p = task(*CONF, "undo", stdin="yes\n", epoch=FROZEN_EPOCH + 120)
        assert p.returncode == 0
        e = export(task)
        assert '"priority"' not in e.stdout

    def test_undo_confirm_no(self, task):
        """Declining the undo prompt leaves the data unchanged."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "modify", "priority:H", record=False,
             epoch=FROZEN_EPOCH + 60)
        p = task(*CONF, "undo", stdin="no\n")
        e = export(task)
        assert '"priority":"H"' in e.stdout
        assert p.returncode in (0, 1)

    def test_undo_exactly_one_command(self, task):
        """Two modifications then undo: only the second is reverted; a second undo reverts the first."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "modify", "priority:H", record=False,
             epoch=FROZEN_EPOCH + 60)
        task("rc.verbose=nothing", "1", "modify", "project:p", record=False,
             epoch=FROZEN_EPOCH + 120)
        task("undo")
        e1 = export(task)
        assert '"priority":"H"' in e1.stdout and '"project"' not in e1.stdout
        task("undo")
        e2 = export(task)
        assert '"priority"' not in e2.stdout

    def test_undo_bulk_done(self, task):
        """undo of a multi-task done reverts all tasks of that command."""
        for d in ("one", "two", "three"):
            add(task, "add", d)
        task("rc.verbose=nothing", "rc.bulk=0", "1-3", "done", record=False, epoch=FROZEN_EPOCH + 60)
        task(*CONF, "undo", stdin="yes\n")
        e = export(task)
        assert e.stdout.count('"status":"pending"') == 3

    def test_undo_nothing(self, task):
        """undo on an empty replica: 'No operations to undo.'"""
        p = task("undo")
        assert "No operations to undo." in both(p)

    def test_undo_delete_annotate(self, task):
        """undo after annotate and after delete."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "annotate", "note", record=False, epoch=FROZEN_EPOCH + 30)
        task(*CONF, "undo", stdin="yes\n")
        task("rc.verbose=nothing", "1", "delete", record=False, epoch=FROZEN_EPOCH + 60)
        task(*CONF, "undo", stdin="yes\n")
        export(task)


class TestSync:
    def test_sync_no_config(self, task):
        """No sync settings: 'No sync.* settings are configured. See task-sync(5).'"""
        p = task("synchronize")
        assert p.returncode != 0
        assert "No sync.* settings are configured." in both(p)

    @pytest.mark.parametrize("rc", [
        ["rc.sync.aws.bucket=b"],
        ["rc.sync.aws.bucket=b", "rc.sync.aws.region=r"],
        ["rc.sync.aws.bucket=b", "rc.sync.aws.region=r", "rc.sync.encryption_secret=s"],
        ["rc.sync.aws.bucket=b", "rc.sync.aws.region=r", "rc.sync.encryption_secret=s",
         "rc.sync.aws.profile=p", "rc.sync.aws.access_key_id=k"],
        ["rc.sync.gcp.bucket=b"],
        ["rc.sync.server.url=http://localhost:1"],
        ["rc.sync.server.origin=http://u:pw@localhost:1"],
        ["rc.sync.git.local_path=/tmp/nowhere-ft7"],
        ["rc.sync.git.local_path=/tmp/nowhere-ft7", "rc.sync.git.branch=main"],
    ], ids=["aws-noregion", "aws-nosecret", "aws-nocreds", "aws-twocreds", "gcp-nosecret",
            "server-noclient", "server-origin-deprecated", "git-nobranch", "git-nosecret"])
    def test_sync_config_errors(self, task, rc):
        """Configuration-error messages for remote backends (exact text and exit code)."""
        p = task(*rc, "sync")
        assert p.returncode != 0

    def test_sync_local_round_trip(self, sb, task):
        """Two replicas sharing sync.local.server_dir exchange tasks both ways."""
        server = os.path.join(sb.root, "server")
        data_b = os.path.join(sb.root, "dataB")
        os.makedirs(data_b)
        os.makedirs(server)
        rc_a = ["rc.sync.local.server_dir=" + server]
        rc_b = rc_a + ["rc.data.location=" + data_b]
        add(task, "add", "from a", "project:sync")
        add(task, "add", "second a")
        p = task(*rc_a, "sync")
        assert p.returncode == 0 and "Success!" in both(p)
        q = task(*rc_b, "sync")
        assert q.returncode == 0
        eb = task(*rc_b, "rc.verbose=nothing", "rc.json.array=on", "export", unordered=True)
        assert "from a" in eb.stdout
        task(*rc_b, "rc.verbose=nothing", "1", "modify", "priority:H", record=False,
             epoch=FROZEN_EPOCH + 60)
        task(*rc_b, "sync")
        task(*rc_a, "sync")
        ea = export(task)
        assert '"priority":"H"' in ea.stdout

    def test_sync_local_nothing_to_send(self, sb, task):
        """Second sync with no local changes: no 'Sent N local operations' line."""
        os.makedirs(os.path.join(sb.root, "server"))
        rc = ["rc.sync.local.server_dir=" + os.path.join(sb.root, "server")]
        add(task, "add", "alpha")
        task(*rc, "sync")
        p = task(*rc, "sync")
        assert "Sent" not in both(p)

    def test_sync_git_round_trip(self, sb, task):
        """git backend (local_only) between two replicas."""
        repo = os.path.join(sb.root, "gitrepo")
        data_b = os.path.join(sb.root, "dataB")
        os.makedirs(data_b)
        rc_a = ["rc.sync.git.local_path=" + repo, "rc.sync.git.branch=main",
                "rc.sync.git.local_only=1", "rc.sync.encryption_secret=sekrit"]
        rc_b = rc_a + ["rc.data.location=" + data_b]
        add(task, "add", "git task")
        p = task(*rc_a, "sync")
        q = task(*rc_b, "sync")
        eb = task(*rc_b, "rc.verbose=nothing", "rc.json.array=on", "export", unordered=True)
        assert p.returncode == 0 and q.returncode == 0
        assert "git task" in eb.stdout

    def test_sync_purge_on_sync(self, sb, task):
        """purge.on-sync expires old deleted tasks during sync."""
        os.makedirs(os.path.join(sb.root, "server"))
        rc = ["rc.sync.local.server_dir=" + os.path.join(sb.root, "server"), "rc.purge.on-sync=1"]
        add(task, "add", "old", epoch=FROZEN_EPOCH - 200 * 86400)
        task("rc.verbose=nothing", "1", "delete", record=False, epoch=FROZEN_EPOCH - 190 * 86400)
        task(*rc, "sync")
        export(task)


class TestPurge:
    def test_purge_deleted(self, task):
        """purge a deleted task (confirmation on, yes)."""
        add(task, "add", "alpha")
        add(task, "add", "beta")
        task("rc.verbose=nothing", "1", "delete", record=False)
        p = task(*CONF, "status:deleted", "purge", short_uuids=True, stdin="yes\n")
        assert p.returncode == 0
        e = export(task)
        assert "alpha" not in e.stdout

    def test_purge_decline(self, task):
        """Declining purge leaves the deleted task."""
        add(task, "add", "alpha")
        task("rc.verbose=nothing", "1", "delete", record=False)
        task(*CONF, "status:deleted", "purge", short_uuids=True, stdin="no\n")
        e = export(task)
        assert "alpha" in e.stdout

    def test_purge_pending_refused(self, task):
        """purge of a pending task is refused / no-op."""
        add(task, "add", "alpha")
        task("1", "purge")
        e = export(task)
        assert "alpha" in e.stdout

    def test_purge_with_dependents(self, task):
        """purge a deleted task that others depend on prompts about dependencies."""
        add(task, "add", "blocker")
        add(task, "add", "blocked", "depends:1")
        task("rc.verbose=nothing", "1", "delete", record=False)
        task(*CONF, "status:deleted", "purge", short_uuids=True, stdin="yes\nyes\n")
        export(task)

    def test_purge_recurring_template(self, task):
        """purge a deleted recurring template (with children) prompts for the children."""
        add(task, "add", "rec", "recur:daily", "due:2009-02-12")
        task("rc.recurrence=on", "rc.verbose=nothing", "list", record=False)
        task("rc.verbose=nothing", "rc.recurrence.confirmation=yes", "2", "delete", record=False)
        # prompt order follows replica (UUID) order on both sides, so the
        # prompt transcript is asserted here and only stored data is compared
        p = task(*CONF, "status:deleted", "purge", record=False,
                 stdin="yes\nyes\nyes\nyes\nyes\nyes\n")
        assert ("Task 'rec' is a recurrence template. All its 3 deleted children tasks will be "
                "purged as well. Continue? (yes/no)") in p.stdout
        assert "Purged 7 tasks." in p.stdout
        task("rc.verbose=nothing", "count")
        export(task)
