# taskwarrior-opus47-user-migration

A hand translation of [Taskwarrior](https://taskwarrior.org/) 3.5.0 from C++ to
Rust. The goal is a `task` binary that is a drop-in replacement for the C++
one: same CLI syntax, same report bytes (whitespace, colour codes, wrapping),
same JSON import/export, error text, exit codes and hook protocol, and the same
on-disk `taskchampion.sqlite3` format in `TASKDATA`.

Storage, undo, the working set and sync come from the
[`taskchampion`](https://crates.io/crates/taskchampion) crate, the same engine
the C++ version links through a cxx bridge. Everything else, including the
libshared utility library (Datetime, Duration, Color, Table, JSON,
Configuration, format/utf8 helpers), is translated by hand.

The C++ reference (`taskwarrior-opus47-dmallya93`) is read-only and is never
compiled or executed.

## Building

The toolchain is pinned in `rust-toolchain.toml` (Rust 1.94.1, the MSRV
declared by the source for taskchampion 3.1); rustup installs it automatically.

```sh
cargo build --release        # -> target/release/task, target/release/make_tc_task
```

The first build needs network access to crates.io. `Cargo.lock` is committed.
The optional `tls-native-roots` feature uses the system CA roots for sync
instead of the bundled ones.

## Testing

### CI gates

Every gate must be green before a change lands:

| Gate | Command |
|---|---|
| Release build | `cargo build --release` |
| Unit and integration tests | `cargo test` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Full acceptance suite | `scripts/run-acceptance.sh --strict` |
| License policy | `cargo deny check licenses` (policy in `deny.toml`; permissive licenses only) |

```sh
cargo build --release        # -> target/release/task, target/release/make_tc_task
cargo test
cargo clippy --all-targets -- -D warnings
TW_REFERENCE_DIR=../taskwarrior-opus47-dmallya93 scripts/run-acceptance.sh --no-build --strict -j 8 -t 120
cargo deny check licenses
```

### Acceptance harness

The acceptance gate is the original Taskwarrior Python (`test/*.test.py`) and
bash TAP (`test/*.test.sh`) suites, run against `target/release/task` with
`TASK_USE_PATH=1` and `target/release` on `PATH`. Results are written to
`acceptance-results/`.

```sh
scripts/run-acceptance.sh                         # build, then run every discovered file (148)
scripts/run-acceptance.sh --no-build -j 8         # skip build, 8 files in parallel
scripts/run-acceptance.sh version.test.py 'tw-*.test.sh'   # subset (names/globs)
scripts/run-acceptance.sh --strict                # final gate: exit 1 on any registered failure
```

The reference repo defaults to `../taskwarrior-opus47-dmallya93` (override with
`TW_REFERENCE_DIR`), and the per-file timeout is 300s (`-t SECS` or
`TW_ACCEPTANCE_TIMEOUT`). The reference `test/` directory is copied to
`acceptance-results/work/test`, and `basetest/utils.py` gets the same
`${CMAKE_BINARY_DIR}`/`${CMAKE_SOURCE_DIR}` substitution that CMake's
`configure_file` does. `CMAKE_BINARY_DIR` is set to `acceptance-results/work`,
which holds the `src/task` and `test/make_tc_task` symlinks. `CMAKE_SOURCE_DIR` is
set to the reference repo root. Each file's output goes to
`acceptance-results/logs/<file>.log`. `summary.txt` and `summary.json` record:

- the files discovered in the reference `test/` directory (`*.test.py`,
  `tw-*.test.sh`, other `*.test.sh`) and how many of them are registered with
  CTest in `test/CMakeLists.txt` (the `pythonTests` and `shell_SRCS` lists);
- one row per file (pass, fail, timeout, skip, plus xfail/xpass for files on a
  `WILL_FAIL` list), with TAP case counts: pass, fail, skip (`ok ... # skip`)
  and xfail (`not ok ... # TODO`, i.e. `@unittest.expectedFailure`);
- every in-file `@unittest.expectedFailure` / `skip` / `skipIf` / `skipUnless`
  marker (and any `skipTest()`/`SkipTest`), with file, line, test, condition
  and reason, plus per-file observed xfail/skip counts next to the marker counts;
- totals for all files and for the CTest-registered files, and the gate verdict.

Files that exist in `test/` but are not registered with CTest still run and are
reported, tagged `[UNREGISTERED: not gated]`. `--strict` mirrors `ctest`: it
gates only the registered files, and it exits 1 if any of them fails or times
out, or if a `WILL_FAIL` file passes. Without `--strict` the script exits 0
once the harness has run. In both modes it exits 2 on a harness error (missing
reference dir, `python3` or binaries, a failed build, or a failed summary).

### Acceptance results

Final run (`scripts/run-acceptance.sh --no-build -j 8 -t 120`, `--strict`
exits 0):

| Scope | Files | File result | Cases |
|---|---|---|---|
| CTest-registered (the gate) | 147 (125 `pythonTests` + 22 `shell_SRCS`) | 146 pass, 1 skip, 0 fail | 1180 pass, 0 fail, 12 skip, 6 xfail (100.00% of cases run) |
| All discovered | 148 (126 `*.test.py` + 22 `tw-*.test.sh`, no other `*.test.sh`) | 146 pass, 1 skip, 1 fail | 1184 pass, 2 fail, 13 skip, 7 xfail (99.83%) |

Notes:

- **`template.test.py`** is the only failing file, and it is not listed in the
  reference `test/CMakeLists.txt`. It is the upstream template for new tests
  and contains deliberately broken cases: `test_fail_other` asserts
  `"I like to code" == "I like\nto code\n"`, and `test_version` calls `.decode`
  on a `str`. It fails against any binary, the C++ one included. It accounts
  for the 1 failing file, the 2 failing cases, and 1 of the skips and 1 of the
  xfails in the "all discovered" row.
- **WILL_FAIL inconsistency.** The spec tracks tests "marked WILL_FAIL in
  test/CMakeLists.txt", but the reference CMakeLists declares no `WILL_FAIL`
  properties, so that list is empty. The harness says this in the summary and
  uses the in-file Python markers as the expected-failure list instead. It
  still parses `set_tests_properties(... WILL_FAIL ...)` in case the reference
  ever adds any.
- **In-file markers.** There are 27 markers in 10 files: 7
  `@unittest.expectedFailure` and 20 skip-type markers. Among the registered
  files, all 6 `expectedFailure` cases report `# TODO` (xfail): 2 in
  `dependencies.test.py`, 3 in `filter.test.py` and 1 in `quotes.test.py`.
  None passes unexpectedly. The 12 registered skips are 9
  `datetime-negative.test.py` cases (`@unittest.skip`, upstream #2996), one
  `dependencies.test.py` case ("Skipping due to undeterminism"), plus the two
  below. The `skipIf` markers in `calc.test.py` (calc binary missing),
  `color.rules.test.py` (Cygwin/FreeBSD) and `search.test.py` (Cygwin) do not
  trigger on Linux, so those tests run and pass.
- **`taskrc.test.py`** is reported as a skipped file because its only test,
  `test_default_taskrc`, is unconditionally `@unittest.skip("taskrc generation
  requires a tty - see #3751")` upstream.
- **`diag.test.py`**: `test_diagnostics` is skipped by
  `skipIf(getattr(platform, "dist", None) == None or ...)`. `platform.dist` was
  removed in Python 3.8, so the condition is always true on a modern Python,
  and the reference skips the test as well.

## Layout

```
Cargo.toml             package `task`; library crate `taskwarrior`
rust-toolchain.toml    pinned toolchain
deny.toml              cargo-deny license policy
doc/rc/                bundled *.theme and holidays.*.rc files (copied from the C++ doc/rc)
src/lib.rs             library root: all logic lives in the `taskwarrior` crate
src/main.rs            `task` binary
src/bin/make_tc_task.rs  test fixture helper writing tasks straight into the replica
src/shared/            libshared port
src/columns/           one module per Col*.cpp
src/commands/          one module per Cmd*.cpp
```

## Module mapping

Each Rust module is the translation of one C++ file, so every piece can be
traced back to its source, and every C++ command class is ported: no
registered command is a stub.

| Rust | C++ |
|---|---|
| `src/main.rs` | `src/main.cpp` |
| `src/bin/make_tc_task.rs` | `test/make_tc_task.cpp` |
| `src/error.rs` | `throw std::string` / `throw int` (spec DD7) |
| `src/version.rs` | `src/Version.cpp`, `cmake.h.in` package macros |
| `src/context.rs` | `src/Context.cpp` |
| `src/cli2.rs` | `src/CLI2.cpp` |
| `src/lexer.rs` | `src/Lexer.cpp` |
| `src/eval.rs` | `src/Eval.cpp` |
| `src/variant.rs` | `src/Variant.cpp` |
| `src/dom.rs` | `src/DOM.cpp` |
| `src/filter.rs` | `src/Filter.cpp` |
| `src/task.rs` | `src/Task.cpp` |
| `src/tdb2.rs` | `src/TDB2.cpp` |
| `src/operation.rs` | `src/Operation.cpp` |
| `src/hooks.rs` | `src/Hooks.cpp` |
| `src/recur.rs` | `src/recur.cpp` |
| `src/rules.rs` | `src/rules.cpp` |
| `src/sort.rs` | `src/sort.cpp` |
| `src/dependency.rs` | `src/dependency.cpp` |
| `src/feedback.rs` | `src/feedback.cpp` |
| `src/nag.rs` | `src/nag.cpp` |
| `src/legacy.rs` | `src/legacy.cpp` |
| `src/util.rs` | `src/util.cpp` |
| `src/view_task.rs` | `src/ViewTask.cpp` |
| `src/tf2.rs` | `src/TF2.cpp` |
| `src/columns/column.rs` | `src/columns/Column.cpp` (`Column` trait) |
| `src/columns/col_<name>.rs` | `src/columns/Col<Name>.cpp` (e.g. `col_type_date.rs` = `ColTypeDate.cpp`, `col_uuid.rs` = `ColUUID.cpp`) |
| `src/commands/command.rs` | `src/commands/Command.cpp` (`Command` trait, registry) |
| `src/commands/cmd_<name>.rs` | `src/commands/Cmd<Name>.cpp` (e.g. `cmd_ids.rs` = `CmdIDs.cpp`, `cmd_import_v2.rs` = `CmdImportV2.cpp`) |
| `src/shared/<name>.rs` | libshared `src/<Name>.cpp`: `shared`, `format`, `utf8`, `unicode`, `pig` (Pig), `fs` (FS), `timer` (Timer), `configuration` (Configuration), `datetime` (Datetime; upstream tests in `datetime_tests.rs`), `duration` (Duration), `json` (JSON, plus `SAX.cpp`), `color` (Color), `table` (Table), `rx` (RX) |
| `src/shared/localtime.rs` | the libc time boundary (`localtime_r`/`mktime`/`timegm`/`time`) used by `datetime`, so `TZ` and libfaketime behave as in C++ |
| `src/lexer_tests.rs` | `test/lexer_test.cpp` (included by `src/lexer.rs` as a test module) |

libshared is translated from GothenburgBitFactory/libshared at commit
`d899eba6d1a863f78f15e5e82d45d857278e9471` (the submodule pinned by the C++
repository); it is not vendored here.

## License

MIT, as the original Taskwarrior.
