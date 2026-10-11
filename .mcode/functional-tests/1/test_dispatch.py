"""Command dispatch: aliases from config, abbreviations, and former milestone stubs."""

from conftest import VERSION, origin_version_banner


def hdr(sb):
    return "TASKRC override: %s\n" % sb.taskrc


class TestAliases:
    def test_alias_to_version(self, sb, check):
        """alias.vv=version in the rc file makes `task vv` run version."""
        sb.write_rc("data.location=%s\nalias.vv=version\n" % sb.data)
        p = sb.run(["vv"])
        check(p, ["vv"], origin_version_banner(80), hdr(sb), 0)

    def test_alias_override_to_under_version(self, sb, check):
        """rc.alias.uv=_version override makes `task uv` run _version (helper => quiet)."""
        args = ["rc.alias.uv=_version", "uv"]
        p = sb.run(args)
        check(p, args, None, "", 0,
              extra={"ok": p.stdout.startswith(VERSION + " (") or p.stdout == VERSION + "\n",
                     "what": "stdout"})


class TestFormerStubs:
    """Commands that were milestone stubs until milestone 8 now dispatch to the
    real implementations, as on origin C++ 3.5.0 (no stubs remain)."""

    def test_former_stub_commands_dispatch(self, sb, check):
        """`diag` (abbreviation) runs diagnostics exactly like `diagnostics`; `show` prints the config table. Exit 0, no stub error."""
        full = sb.run(["diagnostics"])
        diag = sb.run(["diag"])
        show = sb.run(["show"])
        ok = (full.returncode == 0 and diag.stdout == full.stdout and diag.stderr == full.stderr
              and "\ntask 3.5.0\n   Platform:" in diag.stdout and "Build Features" in diag.stdout
              and show.returncode == 0 and "Config Variable" in show.stdout
              and "abbreviation.minimum" in show.stdout
              and "not yet implemented" not in diag.stderr + show.stderr)
        out = "task diag -> exit %d\n%s%stask show -> exit %d\n%s" % (
            diag.returncode, diag.stderr, diag.stdout, show.returncode, show.stderr)
        diag.stdout = out
        diag.stderr = ""
        diag.returncode = 0 if (diag.returncode == 0 and show.returncode == 0) else 2
        check(diag, ["diag", ";", "show"], None, "", 0,
              extra={"ok": ok, "what": "dispatch"})
