#!/usr/bin/env bash
# Acceptance-test harness: runs the reference Taskwarrior Python (test/*.test.py)
# and bash TAP (test/*.test.sh) suites against the Rust release binaries.
#
# Usage: scripts/run-acceptance.sh [--no-build] [--strict] [-j N] [-t SECS] [TEST...]
#
#   TEST...      test file names or shell globs (e.g. version.test.py 'tw-*.test.sh').
#                Default: every *.test.py and *.test.sh in the reference test/ dir.
#   --no-build   skip `cargo build --release` (binaries must already exist).
#   --strict     exit 1 if any CTest-registered test file fails/times out, or a
#                WILL_FAIL file unexpectedly passes (CTest semantics). Files not
#                registered in the reference test/CMakeLists.txt (pythonTests /
#                shell_SRCS), e.g. template.test.py, still run and are reported
#                as UNREGISTERED but never count against the gate.
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
#   summary.txt       one line per file, discovery/registration counts, the
#                     WILL_FAIL note, in-file unittest expected-failure/skip
#                     markers, and totals (all files and CTest-registered only)
#   summary.json      machine-readable equivalent
#
# Expected failures: the reference test/CMakeLists.txt declares no WILL_FAIL
# test properties, so the expected-failure list is the in-file Python markers
# (@unittest.expectedFailure, @unittest.skip/skipIf/skipUnless). They are
# scanned and listed in the summary; their TAP outcomes ("# TODO" = xfail,
# "# skip" = skip) are counted per file. WILL_FAIL parsing is still applied
# if the CMakeLists ever declares any.
#
# Exit code: 0 when the harness ran (test failures are only tracked), 2 on
# harness errors (missing reference dir / python3 / binaries, build failure,
# summary failure), 1 with --strict when any registered test file fails.

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
        -h|--help) sed -n '2,/^$/p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
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
for b in task make_tc_task calc; do
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

# CTest registration: the pythonTests and shell_SRCS lists in the reference
# test/CMakeLists.txt are the files `ctest` would run. Written as JSON
# {"pythonTests": [...], "shell_SRCS": [...]}; a missing list is null.
REGISTERED_FILE="$OUT_DIR/.registered.json"
"$PYTHON" -I - "$REF_TEST/CMakeLists.txt" >"$REGISTERED_FILE" <<'PY' || die "cannot parse CTest registration lists"
import json, re, sys
text = open(sys.argv[1], encoding="utf-8").read()
text = re.sub(r"#[^\n]*", "", text)
out = {}
for var in ("pythonTests", "shell_SRCS"):
    m = re.search(r"\bset\s*\(\s*" + var + r"\b([^)]*)\)", text, re.I | re.S)
    out[var] = m.group(1).split() if m else None
json.dump(out, sys.stdout)
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

"$PYTHON" -I - "$RES_DIR" "$WILL_FAIL_FILE" "$REGISTERED_FILE" "$OUT_DIR" "$STRICT" "$TIMEOUT_SECS" "$REF_DIR" "$REF_TEST" "${TESTS[@]}" <<'PY'
import ast, fnmatch, json, os, re, sys, traceback

def main():
    (res_dir, wf_file, reg_file, out_dir, strict, tmo, ref_dir, ref_test) = sys.argv[1:9]
    tests = sys.argv[9:]
    will_fail = set(l.strip() for l in open(wf_file) if l.strip())
    lists = json.load(open(reg_file))

    # --- Discovery and CTest registration --------------------------------
    on_disk = sorted(f for f in os.listdir(ref_test) if os.path.isfile(os.path.join(ref_test, f)))
    disc_py = [f for f in on_disk if fnmatch.fnmatchcase(f, "*.test.py")]
    disc_sh = [f for f in on_disk if fnmatch.fnmatchcase(f, "*.test.sh")]
    disc_tw = [f for f in disc_sh if f.startswith("tw-")]
    disc_sh_other = [f for f in disc_sh if not f.startswith("tw-")]
    notes = []
    registered = set()
    for var, kind, found in (("pythonTests", "*.test.py", disc_py), ("shell_SRCS", "*.test.sh", disc_sh)):
        if lists.get(var) is None:
            # Cannot tell what CTest would run: gate every discovered file.
            notes.append("test/CMakeLists.txt has no set(%s ...) list; every %s file is treated as registered" % (var, kind))
            registered.update(found)
        else:
            registered.update(lists[var])
    reg_py = lists.get("pythonTests") or []
    reg_sh = lists.get("shell_SRCS") or []
    disc_all = disc_py + disc_sh
    unregistered = [f for f in disc_all if f not in registered]
    registered_missing = sorted(f for f in registered if f not in disc_all)
    discovery = dict(
        test_py=len(disc_py), tw_test_sh=len(disc_tw), other_test_sh=len(disc_sh_other),
        other_test_sh_files=disc_sh_other, total=len(disc_all),
        registered_python_tests=len(reg_py) if lists.get("pythonTests") is not None else None,
        registered_shell_srcs=len(reg_sh) if lists.get("shell_SRCS") is not None else None,
        registered_total=len([f for f in disc_all if f in registered]),
        unregistered=unregistered, registered_but_missing=registered_missing,
    )

    if will_fail:
        wf_note = ("test/CMakeLists.txt declares WILL_FAIL for %d test(s): %s"
                   % (len(will_fail), ", ".join(sorted(will_fail))))
    else:
        wf_note = ("Spec inconsistency: the spec tracks tests \"marked WILL_FAIL in test/CMakeLists.txt\", "
                   "but the reference test/CMakeLists.txt declares no WILL_FAIL test properties, so that "
                   "list is empty. The in-file Python unittest markers (@unittest.expectedFailure, "
                   "@unittest.skip/skipIf/skipUnless, skipTest/SkipTest) listed below are used as the "
                   "expected-failure list instead; their TAP outcomes are counted as xfail (# TODO) and "
                   "skip (# skip).")

    # --- In-file unittest markers -----------------------------------------
    markers = scan_markers(ref_test, [t for t in tests if t.endswith(".py")], registered)

    # --- Per-file results -------------------------------------------------
    rows = []
    for t in tests:
        reg = t in registered
        mk = [m for m in markers if m["file"] == t]
        base = dict(registered=reg, classification="registered" if reg else "unregistered",
                    markers_expected_failure=sum(1 for m in mk if m["marker"] == "expectedFailure"),
                    markers_skip=sum(1 for m in mk if m["marker"] != "expectedFailure"))
        p = os.path.join(res_dir, t + ".res")
        if not os.path.exists(p):
            rows.append(dict(file=t, status="error", exit_code=None, passed=0, failed=0,
                             skipped=0, expected_failures=0, seconds=0, will_fail=t in will_fail, **base))
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
                         skipped=sk, expected_failures=xf, seconds=sec, will_fail=wf, **base))

    totals = make_totals(rows)
    reg_rows = [r for r in rows if r["registered"]]
    totals_reg = make_totals(reg_rows)
    bad = [r for r in reg_rows if r["status"] in ("fail", "timeout", "xpass", "error")]
    unreg_bad = [r for r in rows if not r["registered"] and r["status"] in ("fail", "timeout", "xpass", "error")]

    # --- summary.txt ------------------------------------------------------
    L = []
    L.append("reference: %s" % ref_dir)
    L.append("discovered test files in test/: %d  (*.test.py=%d, tw-*.test.sh=%d, other *.test.sh=%d%s)" % (
        discovery["total"], discovery["test_py"], discovery["tw_test_sh"], discovery["other_test_sh"],
        (": " + ", ".join(disc_sh_other)) if disc_sh_other else ""))
    L.append("CTest-registered in test/CMakeLists.txt: %d  (pythonTests=%s, shell_SRCS=%s)" % (
        discovery["registered_total"], discovery["registered_python_tests"], discovery["registered_shell_srcs"]))
    L.append("unregistered (run and reported, not gated): %d%s" % (
        len(unregistered), (": " + ", ".join(unregistered)) if unregistered else ""))
    if registered_missing:
        L.append("registered but missing on disk: " + ", ".join(registered_missing))
    L.append("selected this run: %d  (registered=%d, unregistered=%d)" % (
        len(rows), len(reg_rows), len(rows) - len(reg_rows)))
    L.append("WILL_FAIL: " + wf_note)
    for n in notes:
        L.append("note: " + n)
    L.append("")
    for r in rows:
        tags = []
        if r["will_fail"]:
            tags.append("[WILL_FAIL]")
        if not r["registered"]:
            tags.append("[UNREGISTERED: not gated]")
        L.append("%-8s %-42s pass=%-4d fail=%-4d skip=%-4d xfail=%-3d rc=%-4s %4ds%s" % (
            r["status"].upper(), r["file"], r["passed"], r["failed"], r["skipped"],
            r["expected_failures"], r["exit_code"], r["seconds"], ("  " + " ".join(tags)) if tags else ""))
    L.append("")
    mfiles = sorted(set(m["file"] for m in markers))
    L.append("in-file expected-failure/skip markers: %d in %d file(s)  (expectedFailure=%d, skip-type=%d)" % (
        len(markers), len(mfiles), sum(1 for m in markers if m["marker"] == "expectedFailure"),
        sum(1 for m in markers if m["marker"] != "expectedFailure")))
    for m in markers:
        detail = []
        if m["condition"] is not None:
            detail.append("if " + m["condition"])
        if m["reason"]:
            detail.append("reason: " + m["reason"])
        L.append("  %s:%d  %s  @%s%s%s" % (
            m["file"], m["line"], m["test"], m["marker"],
            ("  " + "; ".join(detail)) if detail else "",
            "" if m["registered"] else "  [UNREGISTERED]"))
    L.append("")
    L.append("per-file expected failures and skips (observed TAP counts vs in-file markers):")
    L.append("  %-42s %6s %6s %10s %8s" % ("file", "xfail", "skip", "xf-markers", "sk-markers"))
    for r in rows:
        if r["expected_failures"] or r["skipped"] or r["markers_expected_failure"] or r["markers_skip"]:
            L.append("  %-42s %6d %6d %10d %8d%s" % (
                r["file"], r["expected_failures"], r["skipped"], r["markers_expected_failure"],
                r["markers_skip"], "" if r["registered"] else "  [UNREGISTERED]"))
    L.append("")
    tail = [
        "files (all):        " + fmt_files(totals),
        "cases (all):        " + fmt_cases(totals),
        "files (registered): " + fmt_files(totals_reg),
        "cases (registered): " + fmt_cases(totals_reg),
        "gate (registered files, --strict): %s%s" % (
            "PASS" if not bad else "FAIL (%d file(s): %s)" % (len(bad), ", ".join(r["file"] for r in bad)),
            ("  [unregistered failures not gated: %s]" % ", ".join(r["file"] for r in unreg_bad)) if unreg_bad else ""),
    ]
    L.extend(tail)
    with open(os.path.join(out_dir, "summary.txt"), "w") as fh:
        fh.write("\n".join(L) + "\n")
    with open(os.path.join(out_dir, "summary.json"), "w") as fh:
        json.dump(dict(reference_dir=ref_dir, timeout_seconds=int(tmo),
                       discovery=discovery, will_fail=sorted(will_fail), will_fail_note=wf_note,
                       notes=notes, expected_failure_markers=markers,
                       totals=totals, totals_registered=totals_reg,
                       gate=dict(scope="registered", passed=not bad, failing=[r["file"] for r in bad],
                                 unregistered_failing=[r["file"] for r in unreg_bad]),
                       files=rows), fh, indent=2)
        fh.write("\n")
    print()
    print("\n".join(tail))
    print("run-acceptance: summary written to " + os.path.join(out_dir, "summary.txt") + " and summary.json")
    if strict == "1" and bad:
        print("run-acceptance: --strict: %d registered file(s) failed" % len(bad), file=sys.stderr)
        return 1
    return 0

def make_totals(rows):
    count = lambda s: sum(1 for r in rows if r["status"] == s)
    tot = {k: sum(r[k] for r in rows) for k in ("passed", "failed", "skipped", "expected_failures")}
    nfiles = len(rows)
    cases_run = tot["passed"] + tot["failed"]
    return dict(
        files=nfiles,
        files_pass=count("pass"), files_fail=count("fail"), files_timeout=count("timeout"),
        files_skip=count("skip"), files_xfail=count("xfail"), files_xpass=count("xpass"),
        files_error=count("error"),
        file_pass_rate=round(100.0 * (count("pass") + count("xfail")) / nfiles, 2) if nfiles else 0.0,
        cases_passed=tot["passed"], cases_failed=tot["failed"], cases_skipped=tot["skipped"],
        cases_expected_failures=tot["expected_failures"],
        case_pass_rate=round(100.0 * tot["passed"] / cases_run, 2) if cases_run else 0.0,
    )

def fmt_files(t):
    return ("%(files)d  pass=%(files_pass)d fail=%(files_fail)d timeout=%(files_timeout)d "
            "skip=%(files_skip)d xfail=%(files_xfail)d xpass=%(files_xpass)d error=%(files_error)d  "
            "file pass rate=%(file_pass_rate).2f%%" % t)

def fmt_cases(t):
    return ("pass=%(cases_passed)d fail=%(cases_failed)d skip=%(cases_skipped)d "
            "xfail=%(cases_expected_failures)d  case pass rate=%(case_pass_rate).2f%%" % t)

SKIP_DECOS = {"expectedFailure", "skip", "skipIf", "skipUnless"}

def scan_markers(ref_test, files, registered):
    """List @unittest.expectedFailure / skip / skipIf / skipUnless decorators
    (on test methods or classes) and skipTest()/SkipTest raised in bodies."""
    out = []
    for f in sorted(files):
        path = os.path.join(ref_test, f)
        try:
            src = open(path, encoding="utf-8", errors="surrogateescape").read()
            tree = ast.parse(src, filename=f)
        except (OSError, SyntaxError) as e:
            out.append(dict(file=f, line=0, test="?", marker="unparsed", condition=None,
                            reason="cannot parse: %s" % e, description=None, registered=f in registered))
            continue
        lines = src.splitlines()
        seg = lambda n: " ".join((ast.get_source_segment(src, n) or "").split())
        def comment(lineno):
            m = re.search(r"#\s*(.*)$", lines[lineno - 1]) if 0 < lineno <= len(lines) else None
            return m.group(1).strip() if m else None
        def text(n):
            if isinstance(n, ast.Constant) and isinstance(n.value, str):
                return n.value
            return seg(n)
        def visit(node, cls):
            for child in ast.iter_child_nodes(node):
                if isinstance(child, ast.ClassDef):
                    handle(child, child.name, cls_level=True)
                    visit(child, child.name)
                elif isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    handle(child, cls, cls_level=False)
                    for sub in ast.walk(child):
                        rt = None
                        if isinstance(sub, ast.Call) and name_of(sub.func) == "skipTest":
                            rt = ("skipTest", sub.args[0] if sub.args else None)
                        elif isinstance(sub, ast.Raise) and sub.exc is not None:
                            exc = sub.exc.func if isinstance(sub.exc, ast.Call) else sub.exc
                            if name_of(exc) == "SkipTest":
                                rt = ("SkipTest", sub.exc.args[0] if isinstance(sub.exc, ast.Call) and sub.exc.args else None)
                        if rt:
                            out.append(dict(file=f, line=sub.lineno, test=qual(cls, child.name), marker=rt[0],
                                            condition="runtime", reason=text(rt[1]) if rt[1] is not None else comment(sub.lineno),
                                            description=None, registered=f in registered))
        def handle(defn, cls, cls_level):
            for d in defn.decorator_list:
                fn = d.func if isinstance(d, ast.Call) else d
                kind = name_of(fn)
                if kind not in SKIP_DECOS:
                    continue
                cond = reason = None
                args = list(d.args) if isinstance(d, ast.Call) else []
                kw = {k.arg: k.value for k in d.keywords} if isinstance(d, ast.Call) else {}
                if kind in ("skipIf", "skipUnless"):
                    if args:
                        cond = seg(args[0]) if kind == "skipIf" else "not (%s)" % seg(args[0])
                    r = args[1] if len(args) > 1 else kw.get("reason")
                    reason = text(r) if r is not None else None
                elif kind == "skip":
                    r = args[0] if args else kw.get("reason")
                    reason = text(r) if r is not None else None
                if not reason:
                    reason = comment(d.lineno)
                doc = ast.get_docstring(defn)
                test = (defn.name + " (all tests in class)") if cls_level else qual(cls, defn.name)
                out.append(dict(file=f, line=d.lineno, test=test, marker=kind, condition=cond,
                                reason=reason, description=doc.strip().splitlines()[0] if doc else None,
                                registered=f in registered))
        visit(tree, None)
    return out

def name_of(n):
    if isinstance(n, ast.Attribute):
        return n.attr
    if isinstance(n, ast.Name):
        return n.id
    return None

def qual(cls, name):
    return "%s.%s" % (cls, name) if cls else name

try:
    sys.exit(main())
except SystemExit:
    raise
except BaseException:
    traceback.print_exc()
    print("run-acceptance: error: summary generation failed", file=sys.stderr)
    sys.exit(2)
PY
rc=$?
rm -rf "$RES_DIR" "$WILL_FAIL_FILE" "$REGISTERED_FILE"
exit $rc
