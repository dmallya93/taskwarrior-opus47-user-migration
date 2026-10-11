"""news: release highlights, 'up to date', news.version bookkeeping, the nag
footnote, interactive (stdin) path."""

import re

from conftest import add


def rc_text(sb):
    with open(sb.taskrc) as fh:
        return fh.read()


def set_news_version(sb, v):
    t = rc_text(sb)
    t = re.sub(r"news\.version=.*\n", "news.version=%s\n" % v if v else "", t)
    with open(sb.taskrc, "w") as fh:
        fh.write(t)


class TestNews:
    def test_news_up_to_date(self, task):
        """news.version=3.5.0: 'You are up to date!'"""
        p = task("news", stdin="\n")
        assert p.returncode == 0
        assert "You are up to date!" in p.stdout

    def test_news_from_2_6(self, sb, task):
        """news.version unset: highlights from 2.6.0 through 3.5.0, then news.version updated."""
        set_news_version(sb, None)
        p = task("rc.defaultwidth=80", "news", stdin="\n\n\n\n\n\n\n\n\n\n")
        assert p.returncode == 0
        assert "Release Highlights" in p.stdout
        assert "news.version=3.5.0" in rc_text(sb)

    def test_news_from_3_0(self, sb, task):
        """news.version=3.0.0: only the newer items are shown."""
        set_news_version(sb, "3.0.0")
        p = task("rc.defaultwidth=80", "news", stdin="\n\n\n\n\n\n")
        assert p.returncode == 0
        assert "3.0.0 through 3.5.0" in p.stdout or "Release Highlights" in p.stdout

    def test_news_color(self, sb, task):
        """news with forced colour (bold banner)."""
        set_news_version(sb, "3.3.0")
        p = task("rc._forcecolor=on", "rc.color=on", "rc.defaultwidth=80", "news",
                 stdin="\n\n\n\n")
        assert p.returncode == 0

    def test_news_no_stdin(self, sb, task):
        """news with empty stdin (non-interactive, EOF at the enter prompt)."""
        set_news_version(sb, "3.4.0")
        p = task("rc.defaultwidth=80", "news", stdin="")
        assert p.returncode == 0

    def test_news_nag_footnote(self, sb, task):
        """news.version older than current: the 'Recently upgraded' nag appears on reports."""
        set_news_version(sb, "3.0.0")
        add(task, "add", "something")
        p = task("rc.defaultwidth=80", "list")
        assert p.returncode == 0
        assert "news" in (p.stdout + p.stderr)

    def test_news_nag_suppressed(self, sb, task):
        """verbose without 'news' suppresses the nag."""
        set_news_version(sb, "3.0.0")
        add(task, "add", "something")
        p = task("rc.defaultwidth=80", "rc.verbose=header,footnote", "list")
        assert p.returncode == 0

    def test_news_then_no_nag(self, sb, task):
        """After running news the nag is gone (news.version bookkeeping)."""
        set_news_version(sb, "3.0.0")
        add(task, "add", "something")
        task("rc.defaultwidth=80", "news", stdin="\n\n\n\n\n\n")
        p = task("rc.defaultwidth=80", "list")
        assert "news.version=3.5.0" in rc_text(sb)
