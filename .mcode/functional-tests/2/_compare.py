"""Compare origin vs target JSONL records produced by the milestone-2 tests.

Usage: python _compare.py /tmp/ft2_origin.jsonl /tmp/ft2_target.jsonl [out.json]

Dynamic fields normalized before comparison: pytest sandbox paths, the git commit hash in
the Perf line / _version output, and the Perf timer values.
"""

import json
import re
import sys
from collections import OrderedDict

SANDBOX = re.compile(r"/tmp/pytest-of-[^/]+/pytest-\d+/[^/\s'\"]+")
PERF = re.compile(r"Perf task (\S+) (?:[0-9a-f]+|-) (\d{8}T\d{6}Z)(?: [a-z]+:-?\d+)+")
VERSION_HASH = re.compile(r"^3\.5\.0 \([0-9a-f]+\)(?=\r?$)", re.M)

# Per-test fields to compare. The Perf tests target the Perf line (recorded in extra) only:
# the rest of the rc.debug=1 stderr dump (CLI2 parse tree, hook dir) belongs to later milestones.
FIELDS = {
    "test_perf_line_is_utc_iso": ("exit", "stdout", "extra"),
    "test_perf_line_dst_instant": ("exit", "stdout", "extra"),
}
DEFAULT_FIELDS = ("exit", "stdout", "stderr", "extra")


def norm(s):
    if s is None:
        return None
    if not isinstance(s, str):
        s = json.dumps(s, sort_keys=True)
    s = SANDBOX.sub("<SANDBOX>", s)
    s = PERF.sub(r"Perf task \1 <COMMIT> \2 <TIMERS>", s)
    s = VERSION_HASH.sub("3.5.0 (<COMMIT>)", s)
    return s


def load(path):
    out = OrderedDict()
    for line in open(path):
        r = json.loads(line)
        out.setdefault(r["test"], OrderedDict())[r["node"]] = r
    return out


def summarize(r):
    return {"args": r["args"], "env": r["env"], "exit": r["exit"], "stdout": norm(r["stdout"]),
            "stderr": norm(r["stderr"]), "extra": norm(r["extra"])}


def main():
    o, t = load(sys.argv[1]), load(sys.argv[2])
    result = OrderedDict()
    for test in t:
        cases_o, cases_t = o.get(test, {}), t[test]
        diffs = []
        for node, rt in cases_t.items():
            ro = cases_o.get(node)
            if ro is None:
                diffs.append("%s: no origin record" % node)
                continue
            for f in FIELDS.get(test, DEFAULT_FIELDS):
                if norm(ro[f]) != norm(rt[f]):
                    diffs.append("%s: %s differs (origin=%r target=%r)"
                                 % (node, f, norm(ro[f])[:300] if isinstance(norm(ro[f]), str) else ro[f],
                                    norm(rt[f])[:300] if isinstance(norm(rt[f]), str) else rt[f]))
        result[test] = {
            "match": not diffs,
            "diffs": diffs,
            "origin": [summarize(r) for r in cases_o.values()],
            "target": [summarize(r) for r in cases_t.values()],
        }
    out = json.dumps(result, indent=1)
    if len(sys.argv) > 3:
        open(sys.argv[3], "w").write(out)
    for k, v in result.items():
        print("%-45s %s %s" % (k, "MATCH" if v["match"] else "DIFF", "; ".join(v["diffs"])[:400]))


if __name__ == "__main__":
    main()
