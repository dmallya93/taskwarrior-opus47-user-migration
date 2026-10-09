#!/usr/bin/env bash
# Acceptance-test harness: runs the reference Taskwarrior Python (test/*.test.py)
# and bash TAP (test/*.test.sh) suites against the Rust release binaries.
#
# Usage: scripts/run-acceptance.sh [--no-build] [--strict] [-j N] [-t SECS] [TEST...]
#
#   TEST...      test file names or shell globs (e.g. version.test.py 'tw-*.test.sh').
#                Default: every *.test.py and *.test.sh in the reference test/ dir.
#   --no-build   skip `cargo build --release` (binaries must already exist).
#   --strict     exit nonzero if any non-WILL_FAIL test file fails/times out, or
#                a WILL_FAIL file unexpectedly passes (CTest semantics).
#   -j N         run N test files in parallel (default 1; env TW_ACCEPTANCE_JOBS).
#   -t SECS      per-file timeout in seconds (default 300; env TW_ACCEPTANCE_TIMEOUT).
#
# Environment:
#   TW_REFERENCE_DIR      reference (C++) repo root; default ../taskwarrior-opus47-dmallya93
#                         relative to this repo's root.
#   TW_ACCEPTANCE_TIMEOUT per-file timeout (seconds).
#   TW_ACCEPTANCE_JOBS    parallelism.
#   TW_ACCEPTANCE_OUT     results dir; default <repo>/acceptance-results.
#
# Outputs (in acceptance-results/):
#   logs/<file>.log   full stdout+stderr of each test file
#   summary.txt       one line per file plus totals
#   summary.json      machine-readable equivalent
#
# Exit code: 0 when the harness ran (test failures are only tracked), nonzero on
# harness errors (missing reference dir / python3 / binaries, build failure), or
# with --strict when any non-WILL_FAIL test fails.

set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

die() { echo "run-acceptance: error: $*" >&2; exit 2; }

BUILD=1
STRICT=0
JOBS="${TW_ACCEPTANCE_JOBS:-1}"
TIMEOUT_SECS="${TW_ACCEPTANCE_TIMEOUT:-300}"
PATTERNS=()

while [ $# -gt 0 ]; do
    case "$1" in
        --no-build) BUILD=0 ;;
        --strict) STRICT=1 ;;
        -j) shift; JOBS="${1:-}" ;;
        -j*) JOBS="${1#-j}" ;;
        -t|--timeout) shift; TIMEOUT_SECS="${1:-}" ;;
        -h|--help) sed -n '2,29p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        --) shift; PATTERNS+=("$@"); break ;;
        -*) die "unknown option: $1" ;;
        *) PATTERNS+=("$1") ;;
    esac
    shift
done

[[ "$JOBS" =~ ^[1-9][0-9]*$ ]] || die "-j expects a positive integer, got '$JOBS'"
[[ "$TIMEOUT_SECS" =~ ^[1-9][0-9]*$ ]] || die "timeout expects a positive integer, got '$TIMEOUT_SECS'"

# --- Prerequisites ---------------------------------------------------------

REF_DIR="${TW_REFERENCE_DIR:-$REPO_ROOT/../taskwarrior-opus47-dmallya93}"
[ -d "$REF_DIR/test" ] || die "reference test dir not found: $REF_DIR/test (set TW_REFERENCE_DIR)"
[ -f "$REF_DIR/test/basetest/utils.py" ] || die "reference basetest missing in $REF_DIR/test"
REF_DIR="$(cd "$REF_DIR" && pwd)"
REF_TEST="$REF_DIR/test"

PYTHON="$(command -v python3 || true)"
[ -n "$PYTHON" ] || die "python3 not found on PATH"
command -v timeout >/dev/null 2>&1 || die "coreutils 'timeout' not found on PATH"

export PATH="$HOME/.cargo/bin:$PATH"

if [ "$BUILD" -eq 1 ]; then
    echo "run-acceptance: cargo build --release"
    (cd "$REPO_ROOT" && cargo build --release) || die "cargo build --release failed"
fi

RELEASE_DIR="$REPO_ROOT/target/release"
for b in task make_tc_task; do
    [ -x "$RELEASE_DIR/$b" ] || die "missing binary $RELEASE_DIR/$b (build first or drop --no-build)"
done

# --- Staging ---------------------------------------------------------------
# Mirror the CMake build tree: CMAKE_BINARY_DIR=<work> with <work>/src/task,
# <work>/test/make_tc_task and a configured copy of the reference test/ dir.
# CMAKE_SOURCE_DIR points at the (read-only) reference repo root, so
# SOURCE_DIR/CMakeLists.txt, doc/rc themes, scripts/bash and test_hooks resolve.

OUT_DIR="${TW_ACCEPTANCE_OUT:-$REPO_ROOT/acceptance-results}"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
WORK="$OUT_DIR/work"
LOG_DIR="$OUT_DIR/logs"
RES_DIR="$OUT_DIR/.results"

rm -rf "$WORK" "$RES_DIR"
mkdir -p "$WORK/src" "$LOG_DIR" "$RES_DIR"
cp -R "$REF_TEST" "$WORK/test" || die "failed to stage $REF_TEST"
# Never run against stale build artefacts that may exist in the reference tree.
rm -rf "$WORK/test/__pycache__" "$WORK/test/basetest/__pycache__" "$WORK/test/simpletap/__pycache__"

for b in task calc lex; do
    [ -x "$RELEASE_DIR/$b" ] && ln -sf "$RELEASE_DIR/$b" "$WORK/src/$b"
done
ln -sf "$RELEASE_DIR/make_tc_task" "$WORK/test/make_tc_task"

# configure_file() substitution. In the reference test tree only
# basetest/utils.py is configured without COPYONLY; scan every staged file
# anyway so any future ${CMAKE_*_DIR} reference is substituted the same way.
"$PYTHON" -I - "$WORK" "$REF_DIR" "$WORK/test" "$REF_TEST" <<'PY' || die "configure_file substitution failed"
import os, sys
binary_dir, source_dir, cur_bin, cur_src = sys.argv[1:5]
subs = {
    "${CMAKE_BINARY_DIR}": binary_dir,
    "${CMAKE_SOURCE_DIR}": source_dir,
    "${CMAKE_CURRENT_BINARY_DIR}": cur_bin,
    "${CMAKE_CURRENT_SOURCE_DIR}": cur_src,
}
targets = []
for root, _, files in os.walk(cur_bin):
    for f in files:
        if f.endswith(".py"):
            targets.append(os.path.join(root, f))
changed = []
for path in targets:
    with open(path, encoding="utf-8", errors="surrogateescape") as fh:
        text = fh.read()
    new = text
    for k, v in subs.items():
        new = new.replace(k, v)
    if new != text:
        with open(path, "w", encoding="utf-8", errors="surrogateescape") as fh:
            fh.write(new)
        changed.append(os.path.relpath(path, cur_bin))
if "basetest/utils.py" not in changed:
    sys.exit("basetest/utils.py had no ${CMAKE_*} placeholders to substitute")
print("run-acceptance: configured " + ", ".join(sorted(changed)))
PY

# WILL_FAIL list from the reference test/CMakeLists.txt, e.g.
#   set_tests_properties(foo.test.py bar.test.sh PROPERTIES WILL_FAIL TRUE)
WILL_FAIL_FILE="$OUT_DIR/.will_fail"
"$PYTHON" -I - "$REF_TEST/CMakeLists.txt" >"$WILL_FAIL_FILE" <<'PY' || die "cannot parse WILL_FAIL list"
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
text = re.sub(r"#[^\n]*", "", text)
for m in re.finditer(r"set_tests_properties\s*\(([^)]*)\)", text, re.I | re.S):
    words = m.group(1).split()
    up = [w.upper() for w in words]
    if "PROPERTIES" not in up:
        continue
    i = up.index("PROPERTIES")
    props = words[i + 1:]
    for k, v in zip(props[0::2], props[1::2]):
        if k.upper() == "WILL_FAIL" and v.upper() in ("TRUE", "ON", "1", "YES", "Y"):
            for name in words[:i]:
                print(name)
PY

# --- Test selection ---------------------------------------------------------

cd "$WORK/test" || die "cannot enter $WORK/test"
shopt -s nullglob
declare -a TESTS=()
if [ ${#PATTERNS[@]} -eq 0 ]; then
    PATTERNS=("*.test.py" "*.test.sh")
fi
for p in "${PATTERNS[@]}"; do
    p="$(basename "$p")"
    matched=0
    for f in $p; do
        case "$f" in *.test.py|*.test.sh) ;; *) continue ;; esac
        [ -f "$f" ] || continue
        TESTS+=("$f"); matched=1
    done
    [ "$matched" -eq 1 ] || echo "run-acceptance: warning: no test matches '$p'" >&2
done
shopt -u nullglob
# De-duplicate, stable sort.
mapfile -t TESTS < <(printf '%s\n' "${TESTS[@]}" | awk 'NF && !seen[$0]++' | LC_ALL=C sort)
[ ${#TESTS[@]} -gt 0 ] || die "no tests selected"

# --- Test environment (spec: TASK_USE_PATH=1, target/release first on PATH) ---

export TASK_USE_PATH=1
export PATH="$RELEASE_DIR:$PATH"
unset TASKRC TASKDATA TASKW_SKIP
export PYTHONDONTWRITEBYTECODE=1

# Run one file: writes logs/<file>.log and .results/<file>.res
run_one() {
    local f="$1" log="$LOG_DIR/$1.log" res="$RES_DIR/$1.res"
    local start end rc
    start=$(date +%s)
    if [[ "$f" == *.py ]]; then
        timeout -k 10 "$TIMEOUT_SECS" "$PYTHON" "$f" </dev/null >"$log" 2>&1
    else
        timeout -k 10 "$TIMEOUT_SECS" bash "$f" </dev/null >"$log" 2>&1
    fi
    rc=$?
    end=$(date +%s)
    # TAP counts (strip any ANSI colour first). Same classification as the
    # upstream run_all: "ok ... # skip" = skipped, "not ok ... # TODO" =
    # expected failure, other "not ok" = failure, other "ok" = pass.
    local tap passed skipped failed xfail
    tap="$(sed 's/\x1b\[[0-9;]*m//g' "$log")"
    passed=$(grep -E '^ok( |$)' <<<"$tap" | grep -viE '#[[:space:]]*skip' | wc -l)
    skipped=$(grep -E '^ok( |$)' <<<"$tap" | grep -ciE '#[[:space:]]*skip')
    xfail=$(grep -E '^not ok( |$)' <<<"$tap" | grep -cE '#[[:space:]]*TODO')
    failed=$(grep -E '^not ok( |$)' <<<"$tap" | grep -vcE '#[[:space:]]*TODO')
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$f" "$rc" "$passed" "$failed" "$skipped" "$xfail" "$((end - start))" >"$res"
}

echo "run-acceptance: reference=$REF_DIR"
echo "run-acceptance: running ${#TESTS[@]} test file(s), jobs=$JOBS, timeout=${TIMEOUT_SECS}s"

running=0
for f in "${TESTS[@]}"; do
    if [ "$JOBS" -le 1 ]; then
        printf '  %-45s ' "$f"
        run_one "$f"
        cut -f2- "$RES_DIR/$f.res" | awk -F'\t' '{printf "rc=%s pass=%s fail=%s skip=%s (%ss)\n",$1,$2,$3,$4,$6}'
    else
        run_one "$f" &
        running=$((running + 1))
        if [ "$running" -ge "$JOBS" ]; then
            wait -n 2>/dev/null || true
            running=$((running - 1))
        fi
    fi
done
wait

# --- Summary -----------------------------------------------------------------

"$PYTHON" -I - "$RES_DIR" "$WILL_FAIL_FILE" "$OUT_DIR" "$STRICT" "$TIMEOUT_SECS" "$REF_DIR" "${TESTS[@]}" <<'PY'
import json, os, sys
res_dir, wf_file, out_dir, strict, tmo, ref_dir = sys.argv[1:7]
tests = sys.argv[7:]
will_fail = set(l.strip() for l in open(wf_file) if l.strip())
rows = []
for t in tests:
    p = os.path.join(res_dir, t + ".res")
    if not os.path.exists(p):
        rows.append(dict(file=t, status="error", exit_code=None, passed=0, failed=0,
                         skipped=0, expected_failures=0, seconds=0, will_fail=t in will_fail))
        continue
    f, rc, ps, fl, sk, xf, sec = open(p).read().rstrip("\n").split("\t")
    rc, ps, fl, sk, xf, sec = map(int, (rc, ps, fl, sk, xf, sec))
    if rc in (124, 137):
        raw = "timeout"
    elif rc == 0 and fl == 0 and ps + xf == 0:
        raw = "skip"
    elif rc == 0 and fl == 0:
        raw = "pass"
    else:
        raw = "fail"
    wf = t in will_fail
    status = raw
    if wf:
        status = "xpass" if raw == "pass" else ("xfail" if raw in ("fail", "timeout") else raw)
    rows.append(dict(file=t, status=status, exit_code=rc, passed=ps, failed=fl,
                     skipped=sk, expected_failures=xf, seconds=sec, will_fail=wf))

count = lambda s: sum(1 for r in rows if r["status"] == s)
tot = {k: sum(r[k] for r in rows) for k in ("passed", "failed", "skipped", "expected_failures")}
nfiles = len(rows)
cases_run = tot["passed"] + tot["failed"]
totals = dict(
    files=nfiles,
    files_pass=count("pass"), files_fail=count("fail"), files_timeout=count("timeout"),
    files_skip=count("skip"), files_xfail=count("xfail"), files_xpass=count("xpass"),
    files_error=count("error"),
    file_pass_rate=round(100.0 * (count("pass") + count("xfail")) / nfiles, 2) if nfiles else 0.0,
    cases_passed=tot["passed"], cases_failed=tot["failed"], cases_skipped=tot["skipped"],
    cases_expected_failures=tot["expected_failures"],
    case_pass_rate=round(100.0 * tot["passed"] / cases_run, 2) if cases_run else 0.0,
)
lines = ["%-8s %-42s pass=%-4d fail=%-4d skip=%-4d xfail=%-3d rc=%-4s %4ds%s" % (
    r["status"].upper(), r["file"], r["passed"], r["failed"], r["skipped"],
    r["expected_failures"], r["exit_code"], r["seconds"], "  [WILL_FAIL]" if r["will_fail"] else "")
    for r in rows]
lines.append("")
lines.append("files: %(files)d  pass=%(files_pass)d fail=%(files_fail)d timeout=%(files_timeout)d "
             "skip=%(files_skip)d xfail=%(files_xfail)d xpass=%(files_xpass)d error=%(files_error)d  "
             "file pass rate=%(file_pass_rate).2f%%" % totals)
lines.append("cases: pass=%(cases_passed)d fail=%(cases_failed)d skip=%(cases_skipped)d "
             "xfail=%(cases_expected_failures)d  case pass rate=%(case_pass_rate).2f%%" % totals)
with open(os.path.join(out_dir, "summary.txt"), "w") as fh:
    fh.write("\n".join(lines) + "\n")
with open(os.path.join(out_dir, "summary.json"), "w") as fh:
    json.dump(dict(reference_dir=ref_dir, timeout_seconds=int(tmo),
                   will_fail=sorted(will_fail), totals=totals, files=rows), fh, indent=2)
    fh.write("\n")
print()
print("\n".join(lines[-2:]))
print("run-acceptance: summary written to " + os.path.join(out_dir, "summary.txt") + " and summary.json")
bad = [r for r in rows if r["status"] in ("fail", "timeout", "xpass", "error")]
if strict == "1" and bad:
    print("run-acceptance: --strict: %d file(s) failed" % len(bad), file=sys.stderr)
    sys.exit(1)
PY
rc=$?
rm -rf "$RES_DIR" "$WILL_FAIL_FILE"
exit $rc
