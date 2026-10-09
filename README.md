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

```sh
cargo test                                   # unit tests (#[cfg(test)] modules)
cargo clippy --all-targets -- -D warnings    # lint gate
cargo deny check licenses                    # license gate (deny.toml)
```

The acceptance gate is the original Taskwarrior Python (`test/*.test.py`) and
bash TAP (`tw-*.test.sh`) suites, run against `target/release/task` with
`TASK_USE_PATH=1` and `target/release` on `PATH`. Results are written to
`acceptance-results/`.

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
traced back to its source. Modules whose port has not landed yet contain only
a doc comment naming the C++ file and the milestone that fills them.

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
| `src/shared/<name>.rs` | libshared `src/<Name>.cpp`: `shared`, `format`, `utf8`, `unicode`, `pig` (Pig), `fs` (FS), `timer` (Timer), `configuration` (Configuration), `datetime` (Datetime), `duration` (Duration), `json` (JSON), `color` (Color), `table` (Table), `rx` (RX) |

libshared is translated from GothenburgBitFactory/libshared at commit
`d899eba6d1a863f78f15e5e82d45d857278e9471` (the submodule pinned by the C++
repository); it is not vendored here.

## License

MIT, as the original Taskwarrior.
