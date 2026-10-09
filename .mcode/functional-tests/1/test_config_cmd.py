"""`task config` rc-file editing (CmdConfig.cpp)."""

import os

from conftest import json_encode, origin_config, origin_show


def hdr(sb):
    return "TASKRC override: %s\n" % sb.taskrc


def modified(sb):
    return "Config file %s modified.\n" % sb.taskrc


class TestConfigSet:
    def test_config_set_new_no_confirmation(self, sb, check):
        """config name multi word value appends name=value (confirmation=off, no prompt)."""
        sb.write_rc("data.location=%s\nconfirmation=off\n" % sb.data)
        before = sb.read_rc()
        args = ["config", "newkey", "hello", "world"]
        p = sb.run(args)
        exp_file = before + "newkey=" + json_encode("hello world") + "\n"
        check(p, args, modified(sb), hdr(sb), 0,
              extra={"ok": sb.read_rc() == exp_file, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_change_existing_confirmed_preserves_comment(self, sb, check):
        """Changing an existing entry prompts, json-encodes '/' and keeps the trailing comment."""
        sb.write_rc("data.location=%s\nname=old  # keep me\n" % sb.data)
        args = ["config", "name", "one/two/three"]
        p = sb.run(args, stdin="y\n")
        q = "Are you sure you want to change the value of 'name' from 'old' to 'one/two/three'? (yes/no) "
        exp_file = "data.location=%s\nname=one\\/two\\/three  # keep me\n" % sb.data
        check(p, args, q + modified(sb), hdr(sb), 0, stdin="y\n",
              extra={"ok": sb.read_rc() == exp_file, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_change_declined(self, sb, check):
        """Answering 'no' leaves the file untouched and prints 'No changes made.'"""
        content = "data.location=%s\nname=old\n" % sb.data
        sb.write_rc(content)
        args = ["config", "name", "new"]
        p = sb.run(args, stdin="no\n")
        q = "Are you sure you want to change the value of 'name' from 'old' to 'new'? (yes/no) "
        check(p, args, q + "No changes made.\n", hdr(sb), 0, stdin="no\n",
              extra={"ok": sb.read_rc() == content, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_add_eof_means_no(self, sb, check):
        """EOF on stdin at the confirmation prompt counts as 'no'."""
        content = "data.location=%s\n" % sb.data
        sb.write_rc(content)
        args = ["config", "k2", "v"]
        p = sb.run(args, stdin="")
        q = "Are you sure you want to add 'k2' with a value of 'v'? (yes/no) "
        check(p, args, q + "No changes made.\n", hdr(sb), 0, stdin="",
              extra={"ok": sb.read_rc() == content, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_set_blank_value(self, sb, check):
        """config name '' sets the value to blank."""
        sb.write_rc("data.location=%s\nconfirmation=off\nname=old\n" % sb.data)
        args = ["config", "name", ""]
        p = sb.run(args)
        exp_file = "data.location=%s\nconfirmation=off\nname=\n" % sb.data
        check(p, args, modified(sb), hdr(sb), 0,
              extra={"ok": sb.read_rc() == exp_file, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_json_encodes_quotes_backslash(self, sb, check):
        """Value containing quotes and backslash is json-encoded in the rc file."""
        sb.write_rc("data.location=%s\nconfirmation=off\n" % sb.data)
        val = 'say "hi" \\ back'
        args = ["config", "quote", val]
        p = sb.run(args)
        exp_file = "data.location=%s\nconfirmation=off\nquote=%s\n" % (sb.data, json_encode(val))
        check(p, args, modified(sb), hdr(sb), 0,
              extra={"ok": sb.read_rc() == exp_file, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_double_dash(self, sb, check):
        """`config -- name -x` (basetest Task.config form) with 'y' confirmation."""
        sb.write_rc("data.location=%s\n" % sb.data)
        args = ["config", "--", "dash.key", "-x"]
        p = sb.run(args, stdin="y\n")
        q = "Are you sure you want to add 'dash.key' with a value of '-x'? (yes/no) "
        exp_file = "data.location=%s\ndash.key=-x\n" % sb.data
        check(p, args, q + modified(sb), hdr(sb), 0, stdin="y\n",
              extra={"ok": sb.read_rc() == exp_file, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_rc_confirmation_override(self, sb, check):
        """rc.confirmation=off on the command line suppresses the prompt; override footnote."""
        sb.write_rc("data.location=%s\n" % sb.data)
        args = ["rc.confirmation=off", "config", "k", "v"]
        p = sb.run(args)
        check(p, args, modified(sb), hdr(sb) + "Configuration override rc.confirmation=off\n", 0,
              extra={"ok": sb.read_rc().endswith("\nk=v\n"), "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_leading_space_quirk(self, sb, check):
        """C++ quirk: '  name=old' is matched after trim but rewritten from the untrimmed prefix."""
        sb.write_rc("data.location=%s\nconfirmation=off\n  name=old\n" % sb.data)
        args = ["config", "name", "new"]
        p = sb.run(args)
        # line.substr(0, pos + name.length() + 1) with pos computed on the trimmed line
        exp_file = "data.location=%s\nconfirmation=off\n  namnew\n" % sb.data
        check(p, args, modified(sb), hdr(sb), 0,
              extra={"ok": sb.read_rc() == exp_file, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_roundtrip_show(self, sb, check):
        """Value written by config (json-encoded) is decoded back by the parser in _show."""
        sb.write_rc("data.location=%s\n" % sb.data)
        sb.run(["config", "name", "one/two/three"], stdin="y\n")
        p = sb.run(["_show"])
        exp = origin_show(origin_config(sb.taskrc, sb.base_env(), cwd=sb.root))
        assert "name=one/two/three\n" in exp
        check(p, ["_show"], exp, "", 0)


class TestConfigUnset:
    def test_config_unset_existing(self, sb, check):
        """config name (no value) removes the entry."""
        sb.write_rc("data.location=%s\nconfirmation=off\nfoo=bar  # c\n" % sb.data)
        args = ["config", "foo"]
        p = sb.run(args)
        exp_file = "data.location=%s\nconfirmation=off\n" % sb.data
        check(p, args, modified(sb), hdr(sb), 0,
              extra={"ok": sb.read_rc() == exp_file, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_unset_declined_exit_1(self, sb, check):
        """Declining removal: 'No changes made.' and exit status 1 (unsetConfigVariable rc)."""
        content = "data.location=%s\nfoo=bar\n" % sb.data
        sb.write_rc(content)
        args = ["config", "foo"]
        p = sb.run(args, stdin="no\n")
        q = "Are you sure you want to remove 'foo'? (yes/no) "
        check(p, args, q + "No changes made.\n", hdr(sb), 1, stdin="no\n",
              extra={"ok": sb.read_rc() == content, "what": "rcfile", "rcfile": sb.read_rc()})

    def test_config_unset_missing(self, sb, check):
        """config <unknown> => \"No entry named '<n>' found.\" exit 2."""
        args = ["config", "nope"]
        p = sb.run(args)
        check(p, args, "", hdr(sb) + "No entry named 'nope' found.\n", 2)

    def test_config_no_args(self, sb, check):
        """config with no name => 'Specify the name of a config variable to modify.' exit 2."""
        p = sb.run(["config"])
        check(p, ["config"], "", hdr(sb) + "Specify the name of a config variable to modify.\n", 2)
