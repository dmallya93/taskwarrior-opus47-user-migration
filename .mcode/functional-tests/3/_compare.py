"""Compare origin vs target JSONL records produced by the milestone-3 tests.

Usage: python _compare.py /tmp/ft3_origin.jsonl /tmp/ft3_target.jsonl [out.json]

Normalized before comparison: pytest sandbox paths; for test_task_calc_debug only the
Eval / FILTER trace lines of stderr are compared (the rest of the rc.debug=1 dump -- Perf
timers, CLI2 parse tree, hook dirs -- is dynamic or belongs to other milestones).
"""

import json
import re
import sys
from collections import OrderedDict

SANDBOX = re.compile(r"/tmp/pytest-of-[^/]+/pytest-\d+/[^/\s'\"]+")


def norm(s, test=None, field=None):
    if s is None:
        return None
    if not isinstance(s, str):
        s = json.dumps(s, sort_keys=True)
    s = SANDBOX.sub("<SANDBOX>", s)
    if test == "test_task_calc_debug" and field == "stderr":
        s = "\n".join(l for l in s.splitlines() if l.startswith("Eval ") or "FILTER" in l)
    return s


def load(path):
    out = OrderedDict()
    for line in open(path):
        r = json.loads(line)
        out.setdefault(r["test"], OrderedDict())[r["node"]] = r
    return out


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
            for f in ("exit", "stdout", "stderr"):
                a, b = norm(ro[f], test, f), norm(rt[f], test, f)
                if a != b:
                    diffs.append("%s: %s differs (origin=%r target=%r)" % (node, f, a, b))
        result[test] = {"match": not diffs, "diffs": diffs,
                        "cases": len(cases_t), "origin_cases": len(cases_o)}
    out = json.dumps(result, indent=1)
    if len(sys.argv) > 3:
        open(sys.argv[3], "w").write(out)
    for k, v in result.items():
        print("%-42s %3d %s %s" % (k, v["cases"], "MATCH" if v["match"] else "DIFF",
                                   "; ".join(v["diffs"])[:600]))


if __name__ == "__main__":
    main()
