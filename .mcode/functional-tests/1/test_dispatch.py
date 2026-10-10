"""Command dispatch: aliases from config, abbreviations, and milestone stubs."""

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


class TestStubs:
    STUBS = [
        (["list"], "list"),
        (["diag"], "diagnostics"),
        (["show"], "show"),
        ([], "next"),
    ]

    def test_stub_commands_not_implemented(self, sb, check):
        """Unported commands (incl. abbreviation `diag` and default.command=next) fail with exit 2."""
        out, exp = [], []
        all_ok = True
        for args, kw in self.STUBS:
            p = sb.run(args)
            e = hdr(sb) + "Command '%s' is not yet implemented.\n" % kw
            ok = p.returncode == 2 and p.stdout == "" and p.stderr == e
            all_ok &= ok
            out.append("task %s -> exit %d\n%s" % (" ".join(args), p.returncode, p.stderr))
            exp.append("task %s -> exit 2\n%s" % (" ".join(args), e))
        p.stdout = "".join(out)
        p.stderr = ""
        p.returncode = 0 if all_ok else 2
        check(p, ["<multiple>"], "".join(exp), "", 0)
