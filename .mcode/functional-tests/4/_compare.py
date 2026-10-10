"""Compare origin vs target JSONL records produced by the milestone-4 tests.

Usage: python _compare.py /tmp/ft4_origin.jsonl /tmp/ft4_target.jsonl [out.json]

Compares the normalized `observed` payload of each case (UUIDs, operation
timestamps and sandbox paths are already normalized by the tests); falls back to
exit/stdout/stderr when a case has no payload.
"""

import json
import sys
from collections import OrderedDict


def load(path):
    out = OrderedDict()
    for line in open(path):
        r = json.loads(line)
        out.setdefault(r["test"], OrderedDict())[r["node"]] = r
    return out


def key(r):
    if r.get("observed") is not None:
        return json.dumps(r["observed"], sort_keys=True, ensure_ascii=False)
    return json.dumps([r["exit"], r["stdout"], r["stderr"]], ensure_ascii=False)


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
                diffs.append("%s: origin=%s target=%s" % (node, a[:400], b[:400]))
        result[test] = {"match": not diffs, "diffs": diffs,
                        "cases": len(cases_t), "origin_cases": len(cases_o)}
    out = json.dumps(result, indent=1, ensure_ascii=False)
    if len(sys.argv) > 3:
        open(sys.argv[3], "w").write(out)
    for k, v in result.items():
        print("%-44s %3d %s %s" % (k, v["cases"], "MATCH" if v["match"] else "DIFF",
                                   "; ".join(v["diffs"])[:900]))


if __name__ == "__main__":
    main()
