"""Compare origin vs target JSONL records produced by the milestone-7 tests.

Usage: python _compare.py /tmp/ft7_origin.jsonl /tmp/ft7_target.jsonl [out.json]

Each recorded command (test, node, seq) is compared on args, exit code, stdout
and stderr (already normalized for UUIDs / sandbox paths / build hash).
rc.debug timing lines ('Timer ...', 'Perf ...', '... sec') are dropped since
they are wall-clock dependent.
"""

import json
import re
import sys
from collections import OrderedDict

DYN = re.compile(r"(Timer |Perf task|\d+\.\d+ sec|\d+ sec\b)")


def clean(s):
    if s is None:
        return s
    return "\n".join(l for l in s.split("\n") if not DYN.search(l))


def load(path):
    out = OrderedDict()
    for line in open(path):
        r = json.loads(line)
        out.setdefault(r["test"], OrderedDict())["%s#%d" % (r["node"], r["seq"])] = r
    return out


def key(r):
    return [r["args"], r["exit"], clean(r["stdout"]), clean(r["stderr"])]


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
            a, b = key(ro), key(rt)
            if a != b:
                diffs.append("%s: args=%s origin=%s target=%s" % (
                    node, " ".join(rt["args"]), json.dumps(a[1:], ensure_ascii=False)[:600],
                    json.dumps(b[1:], ensure_ascii=False)[:600]))
        result[test] = {"match": not diffs, "diffs": diffs,
                        "cases": len(cases_t), "origin_cases": len(cases_o)}
    out = json.dumps(result, indent=1, ensure_ascii=False)
    if len(sys.argv) > 3:
        open(sys.argv[3], "w").write(out)
    for k, v in result.items():
        print("%-44s %3d %s" % (k, v["cases"], "MATCH" if v["match"] else "DIFF"))
        for d in v["diffs"]:
            print("    " + d[:1500])


if __name__ == "__main__":
    main()
