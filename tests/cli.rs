//! End-to-end tests of the `task` binary: the `version` family and
//! configuration loading (`TASKRC`, `rc:`, `rc.x=y` / `rc.x:y` overrides,
//! `include`), `main.cpp`'s exit statuses, and `Context` dispatch through the
//! full CLI2 parser (`rc.debug.parser` dumps, default command, assumed
//! `information`, abbreviations, `--`, argument checks, message colours),
//! and the milestone 5 commands `add`, `log`, `count`, `_get`, `export`, the
//! `ids`/`uuids` family, `_unique` and `_zshattributes`, and the milestone 6
//! report stack (custom reports, `sort.cpp`, `setHeaderUnderline` and the
//! column factory), and the milestone 7 hook scripts (`Hooks.cpp`) and
//! `synchronize` (local, git and configuration-error paths), and the
//! milestone 8 commands (including `_aliases`, `commands`, `_commands`,
//! `_zshcommands`, `help` / `--help`, `execute` and `logo`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// An isolated environment: a temporary `HOME`, data directory and rc file.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    /// An environment whose rc file `test.rc` holds `rc`.
    fn new(rc: &str) -> Self {
        let env = Env::empty();
        std::fs::create_dir(env.data()).unwrap();
        env.write(
            "test.rc",
            &format!(
                "data.location={}\nhooks=off\nnews.version=2.6.0\n{rc}",
                env.data().display()
            ),
        );
        env
    }

    /// An environment with an empty `HOME` and no rc file.
    fn empty() -> Self {
        let env = Env {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir(env.home()).unwrap();
        env
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    fn data(&self) -> PathBuf {
        self.path("data")
    }

    fn rc(&self) -> PathBuf {
        self.path("test.rc")
    }

    /// The `override` verbosity headers C++ prints on stderr when `TASKRC`
    /// and `TASKDATA` are set (not for helper commands such as `_show`).
    fn override_headers(&self) -> String {
        format!(
            "TASKRC override: {}\nTASKDATA override: {}\n",
            self.rc().display(),
            self.data().display()
        )
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.path(name), contents).unwrap();
    }

    /// `task <args>` with `HOME` isolated and no `TASKRC`/`TASKDATA`.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_task"));
        command
            .args(args)
            .current_dir(self.dir.path())
            .env("HOME", self.home())
            .env_remove("TASKRC")
            .env_remove("TASKDATA")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("TASK_USE_PATH");
        command
    }

    /// `task <args>` with `TASKRC` pointing at the rc file and `TASKDATA`
    /// at the data directory, as the reference `basetest` harness does.
    fn task(&self, args: &[&str]) -> Output {
        self.command(args)
            .env("TASKRC", self.rc())
            .env("TASKDATA", self.data())
            .output()
            .unwrap()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// `_show` output as a map.
fn show(output: &Output) -> BTreeMap<String, String> {
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(output));
    stdout(output)
        .lines()
        .map(|line| {
            let (key, value) = line
                .split_once('=')
                .unwrap_or_else(|| panic!("bad line {line:?}"));
            (key.to_string(), value.to_string())
        })
        .collect()
}

fn current_year() -> i32 {
    // SAFETY: as in the crate: time(NULL) and localtime_r on a zeroed tm.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm.tm_year + 1900
    }
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

////////////////////////////////////////////////////////////////////////////////
// main.cpp

#[test]
fn dash_dash_version_needs_no_rc() {
    let env = Env::empty();
    let output = env.command(&["--version"]).output().unwrap();
    assert_eq!(stdout(&output), "3.5.0\n");
    assert_eq!(stderr(&output), "");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn dash_dash_version_with_more_args_is_a_command_line() {
    // Only `argc == 2` takes the shortcut.
    let env = Env::new("");
    let output = env.task(&["--version", "extra"]);
    assert_ne!(stdout(&output), "3.5.0\n");
}

#[test]
fn dash_dash_help_is_help() {
    // commands.test.py `test_help_option`: `--help` is the `help` command.
    let env = Env::new("");
    let option = env.task(&["--help"]);
    let help = env.task(&["help"]);
    assert!(stdout(&option).contains("Usage:"));
    assert_eq!(stdout(&option), stdout(&help));
    assert_eq!(stderr(&option), stderr(&help));
    assert_eq!(option.status.code(), Some(0));
}

#[test]
fn missing_rc_file_non_interactive() {
    let env = Env::empty();
    let output = env.command(&["_version"]).output().unwrap();
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "Cannot proceed without rc file.\n");
    assert_eq!(output.status.code(), Some(2));
    assert!(!env.home().join(".taskrc").exists());
}

#[test]
fn no_command_with_empty_default_command() {
    let env = Env::new("default.command=\n");
    let output = env.task(&[]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "You must specify a command or a task to modify.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn no_override_headers_without_taskrc_taskdata() {
    let env = Env::new("");
    std::fs::copy(env.rc(), env.home().join(".taskrc")).unwrap();
    let output = env.command(&["calc"]).output().unwrap();
    assert_eq!(stderr(&output), "No expression to evaluate.\n");
    assert_eq!(output.status.code(), Some(2));
}

////////////////////////////////////////////////////////////////////////////////
// CmdDiagnostics.cpp: diagnostics

#[test]
fn diagnostics_reports_build_configuration_hooks_and_tests() {
    let env = Env::new("editor=edlin\ndefaultwidth=80\ndefaultheight=24\n");
    let output = env.task(&["diag"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&output);
    let rc = env.rc().display().to_string();
    let data = env.data().display().to_string();
    assert!(out.starts_with("\ntask 3.5.0\n   Platform: "), "{out}");
    assert!(out.contains("\n\nCompiler\n    Version: rustc "), "{out}");
    assert!(out.contains(" +time_t64\n Compliance: C++17\n\nBuild Features\n"), "{out}");
    assert!(out.contains("\n Build type: "), "{out}");
    assert!(
        out.contains(&format!("\n\nConfiguration\n       File: {rc} (found), ")),
        "{out}"
    );
    assert!(out.contains(&format!("\n       Data: {data} (found), dir, mode 40")), "{out}");
    assert!(out.contains(&format!("\n     TASKRC: {rc}\n   TASKDATA: {data}\n")), "{out}");
    assert!(out.contains("\n  rc.editor: edlin\nHooks\n     System: Disabled\n"), "{out}");
    assert!(
        out.ends_with(
            "             (-none-)\n\nTests\n   Terminal: 120x30\n \
             Broken ref: Scanned 0 tasks for broken references:\n             \
             No broken references found\n\n"
        ),
        "{out}"
    );
    assert!(!out.contains("\x1b[1m"));
}

#[test]
fn diagnostics_bold_only_with_forced_colour() {
    let env = Env::new("");
    let plain = stdout(&env.task(&["rc.color:off", "diag"]));
    assert!(!plain.contains("\x1b[1m"), "{plain}");
    let forced = stdout(&env.task(&["rc._forcecolor:on", "diag"]));
    assert!(forced.starts_with("\n\x1b[1mtask 3.5.0\x1b[0m\n"), "{forced:?}");
    assert!(forced.contains("\x1b[1mTests\x1b[0m\n"), "{forced:?}");
}

#[test]
fn diagnostics_reports_broken_references() {
    let env = Env::new("");
    let missing = "00000000-0000-0000-0000-000000000009";
    let task = "00000000-0000-0000-0000-000000000001";
    env.write(
        "in.json",
        &format!(
            r#"[{{"uuid":"{task}","description":"x","status":"pending","entry":"20240101T000000Z","depends":["{missing}"]}}]"#
        ),
    );
    let output = env.task(&["import", env.path("in.json").to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&env.task(&["diag"]));
    assert!(
        out.ends_with(&format!(
            " Broken ref: Scanned 1 tasks for broken references:\n             \
             Task {task} depends on nonexistent task: {missing}\n\n"
        )),
        "{out}"
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdNews.cpp

/// The `wait_for_enter` prompt and the escapes that erase it.
const NEWS_PRESS_ENTER: &str = "\nPress enter to continue..\x1b[2K\x1b[A\x1b[2K";

/// `task <args>` (TASKRC/TASKDATA set) with `stdin` as input.
fn task_with_stdin(env: &Env, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = env
        .command(args)
        .env("TASKRC", env.rc())
        .env("TASKDATA", env.data())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn news_non_interactive_shows_all_unread_and_records_version() {
    // stdin at end of input: every wait returns at once.
    let env = Env::new("");
    let output = env.task(&["news"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stderr(&output), env.override_headers());
    let out = stdout(&output);
    // Not a TTY, so no colour - but the banner is bold regardless, as in C++.
    assert!(out.starts_with(concat!(
        "\x1b[1m\n",
        "================================================\n",
        "Taskwarrior 2.6.0 through 3.5.0 Release Highlights\n",
        "================================================\n",
        "\x1b[0m\n(1/7) New data model and sync backend (3.0.0)\n",
        "\nPress enter to continue..\x1b[2K\x1b[A\x1b[2K",
        "  What changed in 3.0.0?\n",
        "The sync functionality for Taskwarrior has been rewritten entirely, and no longer\n"
    )), "{out:?}");
    for title in [
        "\n(2/7) Purging Tasks, Manually or Automatically (3.1.0)\n",
        "\n(3/7) Improved 'task news' (3.1.0)\n",
        "\n(4/7) `task info` Journal Restored (3.2.0)\n",
        "\n(5/7) AWS S3 Sync (3.3.0)\n",
        "\n(6/7) Read-Only Access (3.4.0)\n",
        "\n(7/7) Scheduled attribute of recurring tasks (3.5.0)\n",
    ] {
        assert!(out.contains(title), "{title:?} in {out:?}");
    }
    assert!(out.ends_with(&format!(
        "that assume the old behavior.\n\n\n{NEWS_PRESS_ENTER}Thank you for catching up on the new features!\n{NEWS_PRESS_ENTER}"
    )), "{out:?}");
    // Two waits per item, plus the final one.
    assert_eq!(out.matches("Press enter to continue..").count(), 15);

    // news.version was rewritten in place.
    let rc = std::fs::read_to_string(env.rc()).unwrap();
    assert!(rc.contains("\nnews.version=3.5.0\n"), "{rc}");
    assert!(!rc.contains("2.6.0"), "{rc}");

    // Now up to date: nothing but the bold notice and one wait.
    let output = env.task(&["news"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!("\x1b[1mYou are up to date!\n\x1b[0m{NEWS_PRESS_ENTER}")
    );
    assert_eq!(std::fs::read_to_string(env.rc()).unwrap(), rc);

    // ...and custom reports no longer nag.
    let output = env.task(&["rc.verbose=footnote", "next"]);
    assert_eq!(stderr(&output), "No matches.\n");
}

#[test]
fn news_interactive_pages_with_enter() {
    let env = Env::new("news.version=3.4.0\n");
    // The rc now holds news.version twice; both lines are rewritten.
    let output = task_with_stdin(&env, &["rc.verbose=nothing", "news"], "\n\n\n");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stderr(&output), "");
    assert_eq!(
        stdout(&output),
        format!(
            concat!(
                "\x1b[1m\n",
                "================================================\n",
                "Taskwarrior 3.4.0 through 3.5.0 Release Highlights\n",
                "================================================\n",
                "\x1b[0m",
                "\n(1/1) Scheduled attribute of recurring tasks (3.5.0)\n",
                "{p}",
                "  What changed in 3.5.0?\n",
                "The `scheduled` attribute is now updated for new instances of recurring tasks\n",
                "Previously, the value of `scheduled` for a recurring task was copied from the\n",
                "parent task without modification. It is now offset from the recurrence time\n",
                "just like `wait`. While this fixes a bug, it may interact poorly with any hooks\n",
                "that assume the old behavior.\n\n\n",
                "{p}",
                "Thank you for catching up on the new features!\n",
                "{p}"
            ),
            p = NEWS_PRESS_ENTER
        )
    );
    let rc = std::fs::read_to_string(env.rc()).unwrap();
    assert_eq!(rc.matches("news.version=3.5.0\n").count(), 2, "{rc}");
}

#[test]
fn news_colors_headers_with_forcecolor() {
    let env = Env::new("news.version=3.4.0\n");
    let output = env.task(&[
        "rc._forcecolor=on",
        "rc.color.header=green",
        "rc.color.footnote=red",
        "rc.verbose=nothing",
        "news",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains(
        "\n(1/1) \x1b[1m\x1b[32mScheduled attribute of recurring tasks (3.5.0)\n\x1b[0m\x1b[0m"
    ), "{out:?}");
    assert!(out.contains("  \x1b[4mWhat changed in 3.5.0?\n\x1b[0m\x1b[31mThe `scheduled` attribute is now updated for new instances of recurring tasks\n\x1b[0m"), "{out:?}");
}

#[test]
fn news_sigint_while_waiting() {
    use std::io::Read;
    use std::process::Stdio;

    let env = Env::new("news.version=3.4.0\n");
    let mut child = env
        .command(&["rc._forcecolor=on", "rc.color.footnote=red", "news"])
        .env("TASKRC", env.rc())
        .env("TASKDATA", env.data())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _stdin = child.stdin.take().unwrap(); // kept open: the wait blocks
    let mut out = child.stdout.take().unwrap();
    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    while !String::from_utf8_lossy(&seen).contains("Press enter to continue..") {
        let n = out.read(&mut buf).unwrap();
        assert!(n > 0, "{}", String::from_utf8_lossy(&seen));
        seen.extend_from_slice(&buf[..n]);
    }
    // SAFETY: signalling our own child process.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGINT);
    }
    out.read_to_end(&mut seen).unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(1));
    assert!(String::from_utf8_lossy(&seen).ends_with(concat!(
        "\nPress enter to continue..",
        "\n\nCome back and read about new features later!\n",
        "\x1b[31m\nIf you enjoy Taskwarrior, please consider supporting the project at:\n",
        "    https://github.com/sponsors/GothenburgBitFactory/\n\x1b[0m"
    )), "{:?}", String::from_utf8_lossy(&seen));
    // Interrupted: news.version is unchanged.
    assert!(std::fs::read_to_string(env.rc()).unwrap().contains("news.version=3.4.0"));
}

#[test]
fn malformed_rc_file() {
    let env = Env::new("this is not a setting\n");
    let output = env.task(&["_version"]);
    assert_eq!(
        stderr(&output),
        "Malformed entry 'this is not a setting' in config file.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

////////////////////////////////////////////////////////////////////////////////
// version, _version

#[test]
fn under_version() {
    let env = Env::new("");
    let output = env.task(&["_version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), "");
    let out = stdout(&output);
    let words: Vec<&str> = out.split_whitespace().collect();
    assert_eq!(words[0], "3.5.0");
    assert!(out.ends_with('\n'));
    if manifest_dir().join(".git/index").exists() {
        // Built from a git checkout: `x.y.z (hash)`, as test/version.test.py expects.
        assert_eq!(words.len(), 2, "{out:?}");
        let hash = words[1]
            .strip_prefix('(')
            .and_then(|h| h.strip_suffix(')'))
            .unwrap();
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{out:?}");
    } else {
        assert_eq!(out, "3.5.0\n");
    }
}

#[test]
fn version_exact_when_piped() {
    // Piped stdout: color() turns detection off, so the width is
    // defaultwidth (80) and there is no bold.
    let env = Env::new("");
    let output = env.task(&["version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), env.override_headers());
    let out = stdout(&output);
    let first = out.lines().nth(1).unwrap();
    assert!(first.starts_with("task 3.5.0 built for "), "{out}");
    let expected = format!(
        "\n{first}\n\
         Copyright (C) 2006 - {} T. Babej, P. Beckingham, F. Hernandez.\n\
         \n\
         Taskwarrior may be copied only under the terms of the MIT license, which may be\n\
         found in the Taskwarrior source kit.\n\
         \n\
         Documentation for Taskwarrior can be found using 'man task', 'man taskrc', 'man\n\
         task-color', 'man task-sync' or at https://taskwarrior.org\n\
         \n",
        current_year()
    );
    assert_eq!(out, expected);
    assert!(out.contains("MIT license"));
    assert!(out.contains("https://taskwarrior.org"));
}

#[test]
fn version_width_and_forced_color() {
    let env = Env::new("defaultwidth=40\n_forcecolor=on\n");
    let out = stdout(&env.task(&["version"]));
    assert!(
        out.starts_with("\n\x1b[1mtask\x1b[0m \x1b[1m3.5.0\x1b[0m built for "),
        "{out:?}"
    );
    assert!(out.contains(
        "\nTaskwarrior may be copied only under the\nterms of the MIT license, which may be\n"
    ));
}

#[test]
fn version_copyright_follows_tz() {
    let env = Env::new("");
    let out = stdout(
        &env.command(&["version"])
            .env("TASKRC", env.rc())
            .env("TZ", "UTC")
            .output()
            .unwrap(),
    );
    assert!(out.contains("Copyright (C) 2006 - 20"), "{out}");
}

/// `faketime` runs `task` under libfaketime, which intercepts the libc time
/// calls `shared::localtime` makes (spec Design Decision 5). Skipped when the
/// `faketime` wrapper is not installed.
fn faketime_task(env: &Env, timestamp: &str, args: &[&str]) -> Option<Output> {
    let available = Command::new("faketime").arg("--version").output().is_ok();
    if !available {
        eprintln!("faketime not installed; skipping");
        return None;
    }
    let output = Command::new("faketime")
        .arg(timestamp)
        .arg(env!("CARGO_BIN_EXE_task"))
        .args(args)
        .current_dir(env.dir.path())
        .env("HOME", env.home())
        .env("TASKRC", env.rc())
        .env("TASKDATA", env.data())
        .env("TZ", "UTC")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("TASK_USE_PATH")
        .output()
        .unwrap();
    Some(output)
}

#[test]
fn faketime_drives_datetime_now() {
    let env = Env::new("");
    let Some(output) = faketime_task(&env, "2031-06-15 12:34:56", &["version"]) else {
        return;
    };
    let out = stdout(&output);
    assert!(
        out.contains("Copyright (C) 2006 - 2031 T. Babej, P. Beckingham, F. Hernandez."),
        "{out}"
    );

    // The rc.debug Perf line stamps Datetime ().toISO ().
    let Some(output) = faketime_task(&env, "2031-06-15 12:34:56", &["rc.debug=1", "version"])
    else {
        return;
    };
    let err = stderr(&output);
    assert!(err.contains("Perf task 3.5.0 "), "{err}");
    assert!(err.contains(" 20310615T1234"), "{err}");
}

////////////////////////////////////////////////////////////////////////////////
// _show and configuration loading

#[test]
fn show_sorted_key_value_lines() {
    let env = Env::new("");
    let output = env.task(&["_show"]);
    let out = stdout(&output);
    let keys: Vec<&str> = out.lines().map(|l| l.split_once('=').unwrap().0).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let map = show(&output);
    assert_eq!(map["data.location"], env.data().display().to_string());
    assert_eq!(map["hooks"], "off");
    assert_eq!(map["news.version"], "2.6.0");
    assert_eq!(map["default.command"], "next");
    // Non-TTY: Context::color() switches these off.
    assert_eq!(map["color"], "off");
    assert_eq!(map["detection"], "off");
    // default.theme is included by the compiled-in defaults.
    assert_eq!(map["color.active"], "rgb555 on rgb410");
}

#[test]
fn rc_colon_file_override() {
    let env = Env::new("");
    env.write(
        "other.rc",
        &format!("data.location={}\nfoo=from-other\n", env.data().display()),
    );
    let other = env.path("other.rc");
    let output = env
        .command(&[&format!("rc:{}", other.display()), "_show"])
        .output()
        .unwrap();
    let map = show(&output);
    assert_eq!(map["foo"], "from-other");
    assert!(!map.contains_key("news.version") || map["news.version"].is_empty());
}

#[test]
fn taskrc_environment() {
    let env = Env::new("foo=from-taskrc\n");
    let map = show(
        &env.command(&["_show"])
            .env("TASKRC", env.rc())
            .output()
            .unwrap(),
    );
    assert_eq!(map["foo"], "from-taskrc");
}

#[test]
fn rc_dot_overrides() {
    let env = Env::new("foo=file\n");
    assert_eq!(show(&env.task(&["rc.foo=bar", "_show"]))["foo"], "bar");
    assert_eq!(show(&env.task(&["rc.foo:bar", "_show"]))["foo"], "bar");
    assert_eq!(show(&env.task(&["rc.new.key=1", "_show"]))["new.key"], "1");
    assert_eq!(show(&env.task(&["_show"]))["foo"], "file");
}

#[test]
fn include_relative_file() {
    let env = Env::new("include extra.rc\n");
    env.write("extra.rc", "foo=included\ncolor.active=red\n");
    let map = show(&env.task(&["_show"]));
    assert_eq!(map["foo"], "included");
    assert_eq!(map["color.active"], "red");
}

#[test]
fn include_theme_from_rc_dir() {
    let env = Env::new("include dark-16.theme\n");
    let map = show(&env.task(&["_show"]));
    assert_eq!(map["color.active"], "black on bright green");
    assert_eq!(map["color.alternate"], "");
}

#[test]
fn include_missing_file() {
    let env = Env::new("include no-such.rc\n");
    let output = env.task(&["_show"]);
    assert_eq!(
        stderr(&output),
        "Could not find file in CWD, directory of config file or search paths 'no-such.rc'.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

/// Parses rc text the simple way (`key=value`, `#` comments, `include` of a
/// theme from `doc/rc`), independently of the crate's `Configuration`.
fn parse_rc_text(text: &str, home: &str, into: &mut BTreeMap<String, String>) {
    for line in text.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            let value = match value.strip_prefix('~') {
                Some(rest) => format!("{home}{rest}"),
                None => value.to_string(),
            };
            into.insert(key.trim().to_string(), value);
        } else {
            let name = line.strip_prefix("include").unwrap().trim();
            let theme = std::fs::read_to_string(manifest_dir().join("doc/rc").join(name)).unwrap();
            parse_rc_text(&theme, home, into);
        }
    }
}

#[test]
fn show_matches_compiled_in_defaults() {
    // The defaults text is checked verbatim against the C++ Context.cpp by a
    // unit test in src/context.rs.
    let env = Env::new("foo=bar\n");
    let home = env.home().display().to_string();
    let mut expected = BTreeMap::new();
    parse_rc_text(
        taskwarrior::context::CONFIGURATION_DEFAULTS,
        &home,
        &mut expected,
    );
    let rc = std::fs::read_to_string(env.rc()).unwrap();
    parse_rc_text(&rc, &home, &mut expected);
    // Context::color () on a non-TTY.
    expected.insert("color".into(), "off".into());
    expected.insert("detection".into(), "off".into());

    let output = env.task(&["_show"]);
    let actual = show(&output);
    for (key, value) in &expected {
        assert_eq!(actual.get(key), Some(value), "{key}");
    }
    assert_eq!(actual, expected);
    let text: String = expected.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    assert_eq!(stdout(&output), text);
    assert_eq!(stderr(&output), "");
}

////////////////////////////////////////////////////////////////////////////////
// CmdConfig.cpp: config, _config

/// `task rc:<rc> <args>` with `stdin` as input.
fn config_task(env: &Env, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let rc = format!("rc:{}", env.rc().display());
    let mut all = vec![rc.as_str()];
    all.extend_from_slice(args);
    let mut child = env
        .command(&all)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn rc_text(env: &Env) -> String {
    std::fs::read_to_string(env.rc()).unwrap()
}

#[test]
fn config_set_new_modify_blank_and_remove() {
    let env = Env::new("confirmation=off\n");
    let base = rc_text(&env);
    let modified = format!("Config file {} modified.\n", env.rc().display());

    let output = config_task(&env, &["config", "foo", "bar"], "");
    assert_eq!(stdout(&output), modified);
    assert_eq!(
        stderr(&output),
        format!("TASKRC override: {}\n", env.rc().display())
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rc_text(&env), format!("{base}foo=bar\n"));

    // Modify, joining the value words; the value is JSON-encoded.
    let output = config_task(&env, &["config", "foo", "one", "two/\"3\""], "");
    assert_eq!(stdout(&output), modified);
    assert_eq!(rc_text(&env), format!("{base}foo=one two\\/\\\"3\\\"\n"));
    let output = config_task(&env, &["_show"], "");
    assert_eq!(
        show(&output)["foo"],
        "one two/\"3\"",
        "decoded again on load"
    );

    // Blank value, with -- as the basetest harness does.
    let output = config_task(&env, &["config", "--", "foo", ""], "");
    assert_eq!(stdout(&output), modified);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rc_text(&env), format!("{base}foo=\n"));

    // Remove.
    let output = config_task(&env, &["config", "foo"], "");
    assert_eq!(stdout(&output), modified);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rc_text(&env), base);
}

#[test]
fn config_remove_nonexistent_and_missing_name() {
    let env = Env::new("confirmation=off\n");
    let base = rc_text(&env);
    let header = format!("TASKRC override: {}\n", env.rc().display());

    let output = config_task(&env, &["config", "nosuch"], "");
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        format!("{header}No entry named 'nosuch' found.\n")
    );
    assert_eq!(output.status.code(), Some(2));

    let output = config_task(&env, &["config"], "");
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        format!("{header}Specify the name of a config variable to modify.\n")
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rc_text(&env), base);
}

#[test]
fn config_keeps_comments_and_overrides_are_not_words() {
    let env = Env::new("confirmation=off\nfoo=old # why\n# foo=commented\n");
    let base = format!(
        "data.location={}\nhooks=off\nnews.version=2.6.0\nconfirmation=off\n",
        env.data().display()
    );

    let output = config_task(
        &env,
        &["rc.verbose=nothing", "config", "rc.x=1", "foo", "new"],
        "",
    );
    assert_eq!(
        stdout(&output),
        format!("Config file {} modified.\n", env.rc().display())
    );
    assert_eq!(
        rc_text(&env),
        format!("{base}foo=new  # why\n# foo=commented\n")
    );
}

#[test]
fn config_confirmation_prompts() {
    // confirmation defaults to on.
    let env = Env::new("foo=old\n");
    let base = rc_text(&env);

    // Declined (end of input counts as no).
    let output = config_task(&env, &["config", "foo", "new"], "");
    assert_eq!(
        stdout(&output),
        "Are you sure you want to change the value of 'foo' from 'old' to 'new'? (yes/no) No changes made.\n"
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rc_text(&env), base);

    let output = config_task(&env, &["config", "bar", "x"], "n\n");
    assert_eq!(
        stdout(&output),
        "Are you sure you want to add 'bar' with a value of 'x'? (yes/no) No changes made.\n"
    );
    assert_eq!(rc_text(&env), base);

    // A declined removal exits 1.
    let output = config_task(&env, &["config", "foo"], "no\n");
    assert_eq!(
        stdout(&output),
        "Are you sure you want to remove 'foo'? (yes/no) No changes made.\n"
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(rc_text(&env), base);

    // Accepted.
    let modified = format!("Config file {} modified.\n", env.rc().display());
    let output = config_task(&env, &["config", "foo", "new"], "y\n");
    assert_eq!(
        stdout(&output),
        format!("Are you sure you want to change the value of 'foo' from 'old' to 'new'? (yes/no) {modified}")
    );
    assert!(rc_text(&env).ends_with("\nfoo=new\n"));

    // rc.confirmation=off skips the prompt.
    let output = config_task(&env, &["rc.confirmation=off", "config", "foo"], "");
    assert_eq!(stdout(&output), modified);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rc_text(&env), base.replace("foo=old\n", ""));
}

#[test]
fn completion_config_lists_sorted_names() {
    let env = Env::new("zzz.custom=1\n");
    let output = config_task(&env, &["_config"], "");
    assert_eq!(output.status.code(), Some(0));
    let names: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert!(names.contains(&"zzz.custom".to_string()));
    assert!(names.contains(&"confirmation".to_string()));
    assert!(names.contains(&"data.location".to_string()));
    assert_eq!(names.len(), show(&config_task(&env, &["_show"], "")).len());
}

////////////////////////////////////////////////////////////////////////////////
// CmdCalc.cpp and calc.cpp

#[test]
fn task_calc() {
    let env = Env::new("verbose=nothing\n");
    // Bug 1254: no crash multiplying zero and negative numbers.
    let output = env.task(&["calc", "0*-1"]);
    assert_eq!(
        (stdout(&output).as_str(), stderr(&output).as_str()),
        ("0\n", "")
    );
    assert_eq!(output.status.code(), Some(0));

    // Bare numbers are not durations (Context::staticInitialization).
    let output = env.task(&["calc", "100", "*", "2"]);
    assert_eq!(stdout(&output), "200\n");

    let output = env.task(&["rc.expressions=postfix", "calc", "12 3600 * 34 60 * 56 + +"]);
    assert_eq!(stdout(&output), "45296\n");

    // Eval debug output goes to the Context debug sink (stderr).
    let output = env.task(&["rc.debug=1", "calc", "2- -3"]);
    assert_eq!(stdout(&output), "5\n");
    assert!(
        stderr(&output).contains("Eval _neg_ ↓'3' → ↑'-3'\n"),
        "{}",
        stderr(&output)
    );

    // Evaluation errors are C++ `std::string` throws, caught by Context::run.
    let output = env.task(&["calc", "1", "+"]);
    assert_eq!(
        (stdout(&output).as_str(), stderr(&output).as_str()),
        ("", "The expression could not be evaluated.\n")
    );
    assert_eq!(output.status.code(), Some(2));
}

fn calc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_calc"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn calc_binary() {
    let output = calc(&["12 * 3600 + 34 * 60 + 56"]);
    assert_eq!(
        (stdout(&output).as_str(), stderr(&output).as_str()),
        ("45296\n", "")
    );
    assert_eq!(output.status.code(), Some(0));

    let output = calc(&["100*2"]);
    assert_eq!(stdout(&output), "200\n");

    let output = calc(&["--debug", "--postfix", "12 3600 * 34 60 * 56 + +"]);
    let out = stdout(&output);
    assert!(out.contains("\nEval literal number ↑'3600'\n"), "{out}");
    assert!(out.ends_with("\n45296\n"), "{out}");

    let output = calc(&["--debug", "15min"]);
    assert!(
        stdout(&output).ends_with("Eval literal duration ↑'PT15M'\nPT15M\n"),
        "{}",
        stdout(&output)
    );

    let output = calc(&["--help"]);
    let usage = format!(
        "\nUsage: {} [options] '<expression>'\n\nOptions:\n",
        env!("CARGO_BIN_EXE_calc")
    );
    assert!(stdout(&output).starts_with(&usage), "{}", stdout(&output));
    assert_eq!(output.status.code(), Some(1));

    let output = calc(&["--version"]);
    assert!(
        stdout(&output).starts_with("\ncalc 3.5.0 built for "),
        "{}",
        stdout(&output)
    );
    assert_eq!(output.status.code(), Some(1));

    let output = calc(&[]);
    assert_eq!(
        (stdout(&output).as_str(), stderr(&output).as_str()),
        ("", "No expression to evaluate.\n")
    );
    assert_eq!(output.status.code(), Some(255));
}

////////////////////////////////////////////////////////////////////////////////
// Context::initialize / run / dispatch with the full CLI2 parser

/// The C++ title contains U+0456 (Cyrillic i) in "specifіc".
const FINAL_PARSE_TREE: &str = "Parse Tree (before command-specif\u{456}c processing)";

const ESC: &str = "\x1b";

#[test]
fn debug_parser_1_dumps_the_final_parse_tree() {
    let env = Env::new("");
    let output = env.task(&["rc.debug.parser=1", "calc", "1", "+", "2"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "3\n");
    let err = stderr(&output);
    // rc.debug.parser=1 implies rc.debug (propagateDebug): timer, the one
    // final tree dump, Eval tracing and the Perf line.
    assert!(
        err.starts_with(&format!("Timer Config::load ({}) ", env.rc().display())),
        "{err}"
    );
    assert_eq!(err.matches(FINAL_PARSE_TREE).count(), 1, "{err}");
    assert!(
        err.contains(&format!(
            "{ESC}[1m{FINAL_PARSE_TREE}{ESC}[0m\n  _original_args\n"
        )),
        "{err}"
    );
    assert!(err.contains("MISCELLANEOUS"), "{err}");
    assert!(err.contains("\nPerf task 3.5.0 "), "{err}");
    // No intermediate pass dumps at level 1.
    assert!(!err.contains("CLI2::analyze"), "{err}");
    assert!(
        err.ends_with(&(env.override_headers() + "Configuration override rc.debug.parser=1\n")),
        "{err}"
    );
}

#[test]
fn debug_parser_2_dumps_every_pass_but_no_final_tree() {
    let env = Env::new("");
    let output = env.task(&["rc.debug.parser=2", "calc", "1"]);
    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    for title in [
        "CLI2::analyze",
        "CLI2::analyze lexArguments",
        "CLI2::analyze findCommand",
        "CLI2::analyze categorizeArgs",
    ] {
        assert!(
            err.contains(&format!("{ESC}[1m{title}{ESC}[0m\n")),
            "{title}: {err}"
        );
    }
    assert!(!err.contains(FINAL_PARSE_TREE), "{err}");
}

#[test]
fn debug_without_parser_level_has_no_tree() {
    let env = Env::new("");
    let output = env.task(&["rc.debug=on", "rc.debug.parser=0", "_version"]);
    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(err.starts_with("Timer Config::load ("), "{err}");
    assert!(err.contains("\nPerf task 3.5.0 "), "{err}");
    assert!(!err.contains(FINAL_PARSE_TREE), "{err}");

    // Without any debug setting there is no debug output at all.
    let output = env.task(&["_version"]);
    assert_eq!(stderr(&output), "");
}

#[test]
fn default_command_without_arguments() {
    let env = Env::new("default.command=version\n");
    let output = env.task(&["rc.verbose=default"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("task 3.5.0 built for "));
    // 'default' verbosity implies 'header': the recomposed command line.
    assert_eq!(
        stderr(&output),
        format!(
            "[{} version rc.verbose=default]\n",
            env!("CARGO_BIN_EXE_task")
        )
    );

    // Not shown without the 'default' token (the stock verbosity lacks it).
    let output = env.task(&[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), env.override_headers());
}

#[test]
fn unknown_word_is_a_filter_for_the_default_command() {
    // The default command is used, and categorizeArgs rejects the word for a
    // command that accepts no arguments.
    let env = Env::new("default.command=version\n");
    let output = env.task(&["nosuchcommand"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "The 'version' command does not allow 'nosuchcommand'.\n"
    );
    assert_eq!(output.status.code(), Some(2));

    // With the stock default command ('next'): the word is a filter term.
    let env = Env::new("");
    let output = env.task(&["nosuchcommand"]);
    assert_eq!(stderr(&output), env.override_headers() + "No matches.\nRecently upgraded to 3.5.0. Please run 'task news' to read highlights about the new release.\n");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn command_that_allows_nothing_rejects_arguments() {
    let env = Env::new("");
    for args in [
        &["version", "foo"][..],
        &["version", "--", "foo"][..],
        &["_version", "+tag"][..],
    ] {
        let output = env.task(args);
        let word = args.last().unwrap();
        let command = args[0];
        let expected = format!("The '{command}' command does not allow '{word}'.\n");
        assert!(
            stderr(&output).ends_with(&expected),
            "{args:?}: {}",
            stderr(&output)
        );
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(stdout(&output), "", "{args:?}");
    }
}

#[test]
fn id_without_command_assumes_information() {
    let env = Env::new("");
    let output = env.task(&["rc.verbose=default", "1"]);
    assert_eq!(
        stderr(&output),
        "No command specified - assuming 'information'.\n"
    );
    assert_eq!(output.status.code(), Some(1));

    // The header needs the 'default' verbosity token; a UUID also counts.
    for args in [
        &["1", "3"][..],
        &["a1b2c3d4-0000-4000-8000-000000000000"][..],
    ] {
        let output = env.task(args);
        assert_eq!(
            stderr(&output),
            env.override_headers() + "No matches.\n",
            "{args:?}"
        );
    }

    // C++ defaultCommand only looks for number/uuid lexemes: a range or list
    // (a Lexer 'set') falls through to the default command.
    let output = env.task(&["1-2,4"]);
    assert_eq!(stderr(&output), env.override_headers() + "No matches.\nRecently upgraded to 3.5.0. Please run 'task news' to read highlights about the new release.\n");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn command_abbreviation() {
    let env = Env::new("");
    let output = env.task(&["rc.verbose=nothing", "vers"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("task 3.5.0 built for "));
    assert_eq!(stderr(&output), "");

    // Below abbreviation.minimum (2) it is not a command: a filter word for
    // the default command.
    // (`next` with a non-matching filter word: no output, status 1).
    let output = env.task(&["rc.verbose=nothing", "v"]);
    assert_eq!(
        (stdout(&output), stderr(&output)),
        (String::new(), String::new())
    );
    assert_eq!(output.status.code(), Some(1));
    let output = env.task(&["rc.verbose=nothing", "rc.abbreviation.minimum=5", "vers"]);
    assert_eq!(
        (stdout(&output), stderr(&output)),
        (String::new(), String::new())
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn attribute_abbreviation_end_to_end() {
    // abbreviation.test.py with count/_get in place of 'list'.
    let env = Env::new("verbose=nothing\nabbreviation.minimum=1\n");
    env.task(&["add", "project:home", "priority:H", "hasattributes"]);
    env.task(&["add", "noattributes"]);
    for arg in [
        "project:home",
        "projec:home",
        "proje:home",
        "proj:home",
        "pro:home",
    ] {
        assert_eq!(stdout(&env.task(&[arg, "_ids"])), "1\n", "{arg}");
    }
    for arg in [
        "priority:H",
        "priorit:H",
        "priori:H",
        "prior:H",
        "prio:H",
        "pri:H",
    ] {
        assert_eq!(stdout(&env.task(&[arg, "_ids"])), "1\n", "{arg}");
    }
    // Ambiguous (project/priority): a plain word, matching nothing.
    assert_eq!(stdout(&env.task(&["pr:home", "count"])), "0\n");
    // Abbreviated attribute names also apply to modifications.
    env.task(&["add", "pro:work", "pri:L", "third"]);
    assert_eq!(
        stdout(&env.task(&["_get", "3.project", "3.priority"])),
        "work L\n"
    );
    // ...but an ambiguous one is part of the description.
    env.task(&["add", "pr:x", "fourth"]);
    assert_eq!(
        stdout(&env.task(&["_get", "4.description", "4.project"])),
        "pr:x fourth \n"
    );
    // Below abbreviation.minimum the name is not recognized.
    assert_eq!(
        stdout(&env.task(&["rc.abbreviation.minimum=4", "pro:home", "count"])),
        "0\n"
    );
    assert_eq!(
        stdout(&env.task(&["rc.abbreviation.minimum=4", "proj:home", "count"])),
        "1\n"
    );
}

#[test]
fn alias_expansion_end_to_end() {
    // alias.test.py (bug 1031 and filter aliases) with count/_get.
    let env = Env::new("verbose=nothing\nalias.from=to\nalias.cnt=count\nalias.hometoday=count project:home or +today\n");
    env.task(&["add", "from"]);
    env.task(&["add", "from", "--", "to"]);
    env.task(&["add", "to", "--", "from"]);
    assert_eq!(stdout(&env.task(&["_get", "1.description"])), "to\n");
    assert_eq!(stdout(&env.task(&["_get", "2.description"])), "to to\n");
    assert_eq!(stdout(&env.task(&["_get", "3.description"])), "to from\n");
    env.task(&["add", "a", "project:home"]);
    env.task(&["add", "b", "+today"]);
    assert_eq!(stdout(&env.task(&["cnt"])), "5\n");
    assert_eq!(stdout(&env.task(&["hometoday"])), "2\n");
    // An alias from an rc override is loaded too.
    assert_eq!(
        stdout(&env.task(&["rc.alias.zz=_ids", "zz", "+today"])),
        "5\n"
    );
}

#[test]
fn terminator_handling() {
    let env = Env::new("");

    // Arguments after '--' are passed through as words.
    let output = env.task(&["rc.verbose=nothing", "calc", "1", "--", "+", "2"]);
    assert_eq!(stdout(&output), "3\n");
    assert_eq!(output.status.code(), Some(0));

    // Before '--' an rc.x=y argument is a configuration override (and not a
    // word); after it, it is just a word and is not applied.
    let output = env.task(&["calc", "rc.x=1", "2"]);
    assert_eq!(stdout(&output), "2\n");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "Configuration override rc.x=1\n"
    );
    let output = env.task(&["calc", "--", "rc.verbose=nothing", "2"]);
    assert!(
        stderr(&output).starts_with(&env.override_headers()),
        "{}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("Configuration override"),
        "{}",
        stderr(&output)
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn forced_color_message_colors() {
    let env = Env::new("");
    // Errors from initialize are coloured as footnotes (a C++ quirk)...
    let output = env.task(&[
        "rc._forcecolor=on",
        "rc.verbose=nothing",
        "rc.color.footnote=green",
        "rc.color.error=red",
        "version",
        "foo",
    ]);
    assert_eq!(
        stderr(&output),
        format!("{ESC}[32mThe 'version' command does not allow 'foo'.{ESC}[0m\n")
    );
    assert_eq!(output.status.code(), Some(2));

    // ...errors from run as errors; headers and footnotes in their colours.
    let output = env.task(&[
        "rc._forcecolor=on",
        "rc.color.header=blue",
        "rc.color.footnote=green",
        "rc.color.error=red",
        "rc.report.next.labels=X",
        "nosuchcommand",
    ]);
    assert_eq!(
        stderr(&output),
        format!(
            "{ESC}[34mTASKRC override: {}{ESC}[0m\n{ESC}[34mTASKDATA override: {}{ESC}[0m\n\
             {ESC}[32mConfiguration override rc._forcecolor=on{ESC}[0m\n\
             {ESC}[32mConfiguration override rc.color.header=blue{ESC}[0m\n\
             {ESC}[32mConfiguration override rc.color.footnote=green{ESC}[0m\n\
             {ESC}[32mConfiguration override rc.color.error=red{ESC}[0m\n\
             {ESC}[32mConfiguration override rc.report.next.labels=X{ESC}[0m\n\
             {ESC}[31mThere are different numbers of columns and labels for report 'next'.{ESC}[0m\n",
            env.rc().display(),
            env.data().display()
        )
    );

    // Debug messages in color.debug.
    let output = env.task(&[
        "rc._forcecolor=on",
        "rc.color.debug=blue",
        "rc.debug=on",
        "rc.debug.parser=0",
        "_version",
    ]);
    assert!(
        stderr(&output).starts_with(&format!("{ESC}[34mTimer Config::load (")),
        "{}",
        stderr(&output)
    );

    // An empty rule leaves the text alone; without a TTY or _forcecolor there
    // is no colour at all.
    let mismatch = "There are different numbers of columns and labels for report 'next'.\n";
    let output = env.task(&[
        "rc._forcecolor=on",
        "rc.color.error=",
        "rc.verbose=nothing",
        "rc.report.next.labels=X",
    ]);
    assert_eq!(stderr(&output), mismatch);
    let output = env.task(&[
        "rc.color=on",
        "rc.color.error=red",
        "rc.verbose=nothing",
        "rc.report.next.labels=X",
    ]);
    assert_eq!(stderr(&output), mismatch);
}

#[test]
fn bad_color_rule_is_reported() {
    let env = Env::new("");
    let output = env.task(&[
        "rc._forcecolor=on",
        "rc.color.error=chartreuse",
        "rc.verbose=nothing",
        "_version",
    ]);
    // initializeColorRules queues the error and initialization continues; the
    // error is printed by run.
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).starts_with("3.5.0"), "{}", stdout(&output));
    assert_eq!(
        stderr(&output),
        "The color 'chartreuse' is not recognized.\n"
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdAdd.cpp, CmdLog.cpp, CmdCount.cpp, CmdGet.cpp, CmdExport.cpp

/// `task rc.verbose=nothing <args>`: stdout, stderr and exit status.
fn quiet(env: &Env, args: &[&str]) -> (String, String, i32) {
    let mut all = vec!["rc.verbose=nothing"];
    all.extend_from_slice(args);
    let output = env.task(&all);
    (
        stdout(&output),
        stderr(&output),
        output.status.code().unwrap(),
    )
}

/// The exported tasks (`task <filter> export`), one JSON object per element.
fn export(env: &Env, args: &[&str]) -> Vec<serde_json::Value> {
    export_report(env, args, None)
}

/// `task <filter> export [<report>]` as parsed JSON.
fn export_report(env: &Env, args: &[&str], report: Option<&str>) -> Vec<serde_json::Value> {
    let mut all = args.to_vec();
    all.push("export");
    all.extend(report);
    let (out, err, code) = quiet(env, &all);
    assert_eq!((err.as_str(), code), ("", 0), "{out}");
    match serde_json::from_str(&out).unwrap() {
        serde_json::Value::Array(tasks) => tasks,
        other => panic!("not an array: {other}"),
    }
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

#[test]
fn add_reports_new_id_and_project_footnote() {
    let env = Env::new("");
    let output = env.task(&["add", "one", "project:home"]);
    assert_eq!(stdout(&output), "Created task 1.\n");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "The project 'home' has changed.  Project 'home' is 0% complete (1 task remaining).\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let output = env.task(&["rc.verbose=new-id", "add", "two", "project:home"]);
    assert_eq!(stdout(&output), "Created task 2.\n");
    assert_eq!(stderr(&output), "");

    let output = env.task(&["rc.verbose=project", "add", "three", "project:home"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "The project 'home' has changed.  Project 'home' is 0% complete (3 of 3 tasks remaining).\n"
    );

    // No project, no footnote.
    assert_eq!(
        quiet(&env, &["add", "four"]),
        (String::new(), String::new(), 0)
    );
    let tasks = export(&env, &[]);
    let descriptions: Vec<&str> = tasks
        .iter()
        .map(|t| t["description"].as_str().unwrap())
        .collect();
    assert_eq!(descriptions, ["one", "two", "three", "four"]);
    assert_eq!(tasks[0]["project"], "home");
    assert_eq!(tasks[0]["status"], "pending");
}

#[test]
fn add_new_uuid_and_status_variants() {
    let env = Env::new("");

    // new-uuid wins over new-id.
    let output = env.task(&["rc.verbose=new-id,new-uuid", "add", "one"]);
    let out = stdout(&output);
    let uuid = out
        .strip_prefix("Created task ")
        .unwrap()
        .strip_suffix(".\n")
        .unwrap();
    assert!(is_uuid(uuid), "{out}");
    assert_eq!(export(&env, &[])[0]["uuid"], uuid);

    // A completed task has no ID, so new-id shows the UUID.
    let output = env.task(&["rc.verbose=new-id", "add", "done", "status:completed"]);
    let out = stdout(&output);
    let uuid = out
        .strip_prefix("Created task ")
        .unwrap()
        .strip_suffix(".\n")
        .unwrap();
    assert!(is_uuid(uuid), "{out}");

    // Recurrence templates.
    let output = env.task(&[
        "rc.verbose=new-id",
        "add",
        "rec",
        "recur:weekly",
        "due:tomorrow",
    ]);
    assert_eq!(stdout(&output), "Created task 2 (recurrence template).\n");
    let output = env.task(&[
        "rc.verbose=new-uuid",
        "add",
        "rec",
        "recur:weekly",
        "due:tomorrow",
    ]);
    let out = stdout(&output);
    let uuid = out
        .strip_prefix("Created task ")
        .unwrap()
        .strip_suffix(" (recurrence template).\n")
        .unwrap();
    assert!(is_uuid(uuid), "{out}");

    // A waiting task still gets an ID.
    let output = env.task(&["rc.verbose=new-id", "add", "later", "wait:tomorrow"]);
    assert_eq!(stdout(&output), "Created task 4.\n");
}

#[test]
fn add_errors() {
    let env = Env::new("");
    assert_eq!(
        quiet(&env, &["add"]),
        (
            String::new(),
            "Additional text must be provided.\n".into(),
            2
        )
    );
    assert_eq!(
        quiet(&env, &["add", "rec", "recur:weekly"]),
        (
            String::new(),
            "A recurring task must also have a 'due' date.\n".into(),
            2
        )
    );
    assert_eq!(quiet(&env, &["count"]).0, "0\n");
}

#[test]
fn add_dom_refers_to_earlier_modifications() {
    let env = Env::new("");
    assert_eq!(
        quiet(&env, &["add", "x", "due:2030-01-01", "scheduled:due"]).2,
        0
    );
    let tasks = export(&env, &[]);
    assert_eq!(tasks[0]["scheduled"], tasks[0]["due"]);
}

#[test]
fn log_adds_completed_task() {
    let env = Env::new("");
    let output = env.task(&["log", "one", "project:home"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers()
            + "The project 'home' has changed.  Project 'home' is 100% complete (0 of 1 tasks remaining).\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let output = env.task(&["rc.verbose=new-uuid", "log", "two"]);
    let out = stdout(&output);
    let uuid = out
        .strip_prefix("Logged task ")
        .unwrap()
        .strip_suffix(".\n")
        .unwrap();
    assert!(is_uuid(uuid), "{out}");

    let tasks = export(&env, &[]);
    assert_eq!(tasks.len(), 2);
    for task in &tasks {
        assert_eq!(task["status"], "completed");
        assert_eq!(task["id"], 0);
        assert!(task["end"].is_string(), "{task}");
    }
    // Both have ID 0, so `id,uuid` orders them by UUID.
    assert!(tasks.iter().any(|t| t["uuid"] == uuid));
    assert!(tasks[0]["uuid"].as_str().unwrap() < tasks[1]["uuid"].as_str().unwrap());
}

#[test]
fn log_errors() {
    let env = Env::new("");
    assert_eq!(
        quiet(&env, &["log", "rec", "recur:weekly", "due:tomorrow"]),
        (String::new(), "You cannot log recurring tasks.\n".into(), 2)
    );
    assert_eq!(
        quiet(&env, &["log", "w", "wait:tomorrow"]),
        (String::new(), "You cannot log waiting tasks.\n".into(), 2)
    );
    assert_eq!(
        quiet(&env, &["log"]),
        (
            String::new(),
            "Additional text must be provided.\n".into(),
            2
        )
    );
    assert_eq!(quiet(&env, &["count"]).0, "0\n");
}

#[test]
fn count_skips_recurring_templates() {
    let env = Env::new("");
    assert_eq!(quiet(&env, &["count"]), ("0\n".into(), String::new(), 0));
    for args in [
        &["add", "one", "+tag"][..],
        &["add", "two"],
        &["log", "three"],
        &["add", "rec", "recur:weekly", "due:tomorrow"],
    ] {
        assert_eq!(quiet(&env, args).2, 0);
    }
    // `count` runs handleRecurrence first: the template's first instance is
    // created (pending), the template itself is never counted.
    assert_eq!(quiet(&env, &["count"]).0, "4\n");
    assert_eq!(quiet(&env, &["+tag", "count"]).0, "1\n");
    assert_eq!(quiet(&env, &["status:pending", "count"]).0, "3\n");
    assert_eq!(quiet(&env, &["parent.any:", "count"]).0, "1\n");
    assert_eq!(quiet(&env, &["status:recurring", "count"]).0, "0\n");
    assert_eq!(quiet(&env, &["/o/", "count"]).0, "2\n");
}

#[test]
fn get_dom_references() {
    let env = Env::new("");
    assert_eq!(quiet(&env, &["add", "one", "due:2030-02-03", "+a"]).2, 0);
    assert_eq!(
        quiet(
            &env,
            &[
                "_get",
                "1.description",
                "1.due.year",
                "rc.verbose",
                "system.version"
            ]
        ),
        ("one 2030 nothing 3.5.0\n".into(), String::new(), 0)
    );
    // An unresolvable reference is an empty string.
    assert_eq!(
        quiet(&env, &["_get", "2.description"]),
        ("\n".into(), String::new(), 0)
    );
    assert_eq!(
        quiet(
            &env,
            &["_get", "1.description", "2.description", "1.description"]
        ),
        ("one  one\n".into(), String::new(), 0)
    );
    assert_eq!(
        quiet(&env, &["_get"]),
        (String::new(), "No DOM reference specified.\n".into(), 2)
    );
    assert_eq!(
        quiet(&env, &["_get", "foo"]),
        (String::new(), "'foo' is not a DOM reference.\n".into(), 2)
    );
    assert_eq!(
        quiet(&env, &["_get", "1.description", "foo"]),
        (String::new(), "'foo' is not a DOM reference.\n".into(), 2)
    );
}

#[test]
fn export_json_array_and_decoration() {
    let env = Env::new("");
    assert_eq!(
        quiet(&env, &["export"]),
        ("[\n]\n".into(), String::new(), 0)
    );
    assert_eq!(
        quiet(&env, &["rc.json.array=off", "export"]),
        (String::new(), String::new(), 0)
    );

    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two"]);
    let (out, err, code) = quiet(&env, &["export"]);
    assert_eq!((err.as_str(), code), ("", 0));
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines.len(), 5, "{out}");
    assert_eq!(lines[0], "[");
    assert!(
        lines[1].starts_with("{\"id\":1,\"description\":\"one\",")
            && lines[1].ends_with("\"urgency\":0},")
    );
    assert!(
        lines[2].starts_with("{\"id\":2,\"description\":\"two\",")
            && lines[2].ends_with("\"urgency\":0}")
    );
    assert_eq!(&lines[3..], ["]", ""]);

    let (out, _, _) = quiet(&env, &["rc.json.array=off", "export"]);
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].ends_with('}') && lines[1].ends_with('}') && lines[2].is_empty());

    // limit:N, with a filter (a lone pseudo arg leaves a string result,
    // which C++ `get_bool` treats as false).
    let tasks = export(&env, &["/o/", "limit:1"]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["description"], "one");
}

#[test]
fn export_depends_array() {
    let env = Env::new("");
    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two", "depends:1"]);
    let uuid = export(&env, &["1"])[0]["uuid"]
        .as_str()
        .unwrap()
        .to_string();
    let task = &export(&env, &["2"])[0];
    assert_eq!(task["depends"], serde_json::json!([uuid]));
    // Taskwarrior 3.5.0 has no `json.depends.array` setting: composeJSON
    // always emits an array.
    let task = &export(&env, &["rc.json.depends.array=off", "2"])[0];
    assert_eq!(task["depends"], serde_json::json!([uuid]));
}

#[test]
fn export_report_filter_and_sort() {
    let env = Env::new(
        "report.byrev.columns=id\nreport.byrev.sort=description-\nreport.byrev.filter=+x\n\
         report.asgiven.columns=id\nreport.asgiven.sort=none\n",
    );
    quiet(&env, &["add", "a", "+x"]);
    quiet(&env, &["add", "b"]);
    quiet(&env, &["add", "c", "+x"]);
    let descriptions = |tasks: Vec<serde_json::Value>| -> Vec<String> {
        tasks
            .iter()
            .map(|t| t["description"].as_str().unwrap().to_string())
            .collect()
    };

    // Report filter added to the command-line filter, report sort applied;
    // report names are matched by closeEnough.
    assert_eq!(
        descriptions(export_report(&env, &[], Some("byrev"))),
        ["c", "a"]
    );
    assert_eq!(
        descriptions(export_report(&env, &[], Some("byr"))),
        ["c", "a"]
    );
    assert_eq!(
        descriptions(export_report(&env, &["/a/"], Some("byrev"))),
        ["a"]
    );
    // Without a report: `id,uuid`.
    assert_eq!(descriptions(export(&env, &[])), ["a", "b", "c"]);

    // sort=none: the command line's UUID order.
    let uuids: Vec<String> = export(&env, &[])
        .iter()
        .map(|t| t["uuid"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        descriptions(export_report(
            &env,
            &[&uuids[2], &uuids[0], &uuids[1]],
            Some("asgiven")
        )),
        ["c", "a", "b"]
    );

    assert_eq!(
        quiet(&env, &["export", "nosuchreport"]),
        (
            String::new(),
            "Unable to find report that matches 'nosuchreport'.\n".into(),
            2
        )
    );
    // Non-report commands do not match.
    assert_eq!(
        quiet(&env, &["export", "count"]),
        (
            String::new(),
            "Unable to find report that matches 'count'.\n".into(),
            2
        )
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdIDs.cpp

/// Pending `one` (1), `two +t` (2), `a:b` (3), completed `done` (no ID),
/// waiting `w` (4), pending `five +t` (5); returns the env.
fn ids_env() -> Env {
    let env = Env::new("");
    for args in [
        &["add", "one"][..],
        &["add", "two", "+t"],
        &["add", "a:b"],
        &["log", "done"],
        &["add", "w", "wait:2099-01-01"],
        &["add", "five", "+t"],
    ] {
        assert_eq!(quiet(&env, args).2, 0, "{args:?}");
    }
    env
}

#[test]
fn ids_compress_ranges_and_skip_id_zero() {
    let env = ids_env();
    assert_eq!(quiet(&env, &["ids"]), ("1-5\n".into(), String::new(), 0));
    assert_eq!(quiet(&env, &["+t", "ids"]).0, "2 5\n");
    assert_eq!(quiet(&env, &["1", "3-5", "ids"]).0, "1 3-5\n");
    assert_eq!(quiet(&env, &["1,2,4", "ids"]).0, "1-2 4\n");
    // Completed tasks have ID 0 and are skipped.
    assert_eq!(quiet(&env, &["status:completed", "ids"]).0, "\n");
    // Headers (here the TASKRC/TASKDATA overrides) are cleared.
    let output = env.task(&["ids"]);
    assert_eq!(
        (stdout(&output), stderr(&output)),
        ("1-5\n".into(), String::new())
    );
}

#[test]
fn completion_ids_list_and_zsh_format() {
    let env = ids_env();
    assert_eq!(
        quiet(&env, &["_ids"]),
        ("1\n2\n3\n4\n5\n".into(), String::new(), 0)
    );
    assert_eq!(quiet(&env, &["+t", "_ids"]).0, "2\n5\n");
    // Completed tasks are excluded; an empty list still ends in a newline.
    assert_eq!(quiet(&env, &["status:completed", "_ids"]).0, "\n");
    // id:description, with ':' in descriptions replaced by ','.  The order
    // is the filter's (taskchampion `pending_task_data`, hash order in C++
    // too), so compare sorted lines.
    let sorted_lines = |text: &str| {
        assert!(text.is_empty() || text.ends_with('\n'), "{text:?}");
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        lines.sort();
        lines
    };
    let (out, err, code) = quiet(&env, &["_zshids"]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert_eq!(
        sorted_lines(&out),
        ["1:one", "2:two", "3:a,b", "4:w", "5:five"]
    );
    assert_eq!(quiet(&env, &["status:completed", "_zshids"]).0, "");
    let output = env.task(&["_zshids", "+t"]);
    assert_eq!(stderr(&output), "");
    assert_eq!(sorted_lines(&stdout(&output)), ["2:two", "5:five"]);
}

#[test]
fn uuids_sorted_and_zsh_format() {
    let env = ids_env();
    let tasks = export(&env, &[]);
    let field = |t: &serde_json::Value, f: &str| t[f].as_str().unwrap().to_string();
    let mut uuids: Vec<String> = tasks.iter().map(|t| field(t, "uuid")).collect();
    assert!(uuids.iter().all(|u| is_uuid(u)));
    uuids.sort();
    assert_eq!(uuids.len(), 6);

    // All statuses, sorted.
    assert_eq!(
        quiet(&env, &["uuids"]),
        (uuids.join(" ") + "\n", String::new(), 0)
    );
    assert_eq!(
        quiet(&env, &["_uuids"]),
        (uuids.join("\n") + "\n", String::new(), 0)
    );
    let done = tasks
        .iter()
        .find(|t| field(t, "description") == "done")
        .unwrap();
    assert_eq!(
        quiet(&env, &["status:completed", "uuids"]).0,
        field(done, "uuid") + "\n"
    );
    assert_eq!(quiet(&env, &["/nomatch/", "uuids"]).0, "\n");
    assert_eq!(quiet(&env, &["/nomatch/", "_uuids"]).0, "\n");
    assert_eq!(quiet(&env, &["/nomatch/", "_zshuuids"]).0, "");

    // uuid:description for every matching task (completed included).
    let mut expected: Vec<String> = tasks
        .iter()
        .map(|t| {
            format!(
                "{}:{}",
                field(t, "uuid"),
                field(t, "description").replace(':', ",")
            )
        })
        .collect();
    expected.sort();
    let (out, err, code) = quiet(&env, &["_zshuuids"]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert!(out.ends_with('\n'));
    let mut lines: Vec<String> = out.lines().map(String::from).collect();
    lines.sort();
    assert_eq!(lines, expected);

    let output = env.task(&["uuids"]);
    assert_eq!(stderr(&output), "");
}

////////////////////////////////////////////////////////////////////////////////
// CmdUnique.cpp

#[test]
fn unique_attribute_values() {
    let env = Env::new("uda.size.type=string\n");
    for args in [
        &["add", "one", "project:A", "+x"][..],
        &["add", "two", "project:A", "size:big"],
        &["add", "three", "project:B", "+x", "+y"],
        &["log", "five", "project:D", "size:small"],
    ] {
        assert_eq!(quiet(&env, args).2, 0, "{args:?}");
    }
    // Sorted and distinct; completed tasks included.
    assert_eq!(
        quiet(&env, &["_unique", "project"]),
        ("A\nB\nD\n".into(), String::new(), 0)
    );
    assert_eq!(
        quiet(&env, &["status:pending", "_unique", "project"]).0,
        "A\nB\n"
    );
    assert_eq!(
        quiet(&env, &["_unique", "status"]).0,
        "completed\npending\n"
    );
    // Abbreviations and UDAs are canonicalized.
    assert_eq!(quiet(&env, &["_unique", "proj"]).0, "A\nB\nD\n");
    assert_eq!(quiet(&env, &["_unique", "size"]).0, "big\nsmall\n");
    // Tags as stored (comma-separated).
    assert_eq!(quiet(&env, &["_unique", "tags"]).0, "x\nx,y\n");
    // `id` is synthesized for non-completed tasks only.
    assert_eq!(quiet(&env, &["_unique", "id"]).0, "1\n2\n3\n");
    // No match: empty output.
    assert_eq!(
        quiet(&env, &["/nomatch/", "_unique", "project"]),
        (String::new(), String::new(), 0)
    );
    // Headers are cleared.
    let output = env.task(&["_unique", "project"]);
    assert_eq!(
        (stdout(&output), stderr(&output)),
        ("A\nB\nD\n".into(), String::new())
    );

    assert_eq!(
        quiet(&env, &["_unique"]),
        (
            String::new(),
            "An attribute must be specified.  See 'task _columns'.\n".into(),
            2
        )
    );
    assert_eq!(
        quiet(&env, &["_unique", "nosuchattr"]),
        (
            String::new(),
            "You must specify an attribute or UDA.\n".into(),
            2
        )
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdAttributes.cpp

#[test]
fn zsh_attributes_lists_sorted_columns() {
    let builtin = [
        "depends",
        "description",
        "due",
        "end",
        "entry",
        "id",
        "imask",
        "last",
        "mask",
        "modified",
        "parent",
        // `priority` is a UDA in the compiled-in defaults.
        "priority",
        "project",
        "recur",
        "rtype",
        "scheduled",
        "start",
        "status",
        "tags",
        "template",
        "until",
        "urgency",
        "uuid",
        "wait",
    ];
    let lines = |names: &[&str]| {
        names
            .iter()
            .map(|n| format!("{n}:{n}\n"))
            .collect::<String>()
    };

    let env = Env::new("");
    assert_eq!(
        quiet(&env, &["_zshattributes"]),
        (lines(&builtin), String::new(), 0)
    );

    // UDAs are columns too, sorted in with the built-ins.
    let env = Env::new("uda.size.type=string\nuda.aaa.type=numeric\n");
    let mut all = builtin.to_vec();
    all.extend(["size", "aaa"]);
    all.sort();
    let output = env.task(&["_zshattributes"]);
    // Helper (`_`) commands run with verbosity `nothing`: no override headers.
    assert_eq!(
        (stdout(&output), stderr(&output)),
        (lines(&all), String::new())
    );
    // Accepts no filter.
    assert_eq!(quiet(&env, &["1", "_zshattributes"]).2, 2);
}

////////////////////////////////////////////////////////////////////////////////
// CmdColor.cpp

#[test]
fn colors_off_sample_legend_and_chart() {
    let env = Env::new("");
    // Not a TTY and no _forcecolor: the hint, exit status 1.
    assert_eq!(
        quiet(&env, &["colors"]),
        (
            "Color is currently turned off in your .taskrc file.  To enable color, remove the line \
             'color=off', or change the 'off' to 'on'.\n"
                .into(),
            String::new(),
            1
        )
    );

    // A sample of the words as a colour spec.
    let (out, err, rc) = quiet(&env, &["rc._forcecolor:on", "colors", "red"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        out,
        "\nUse this command to see how colors are displayed by your terminal.\n\n\n\
         16-color usage (supports underline, bold text, bright background):\n\
         \x20 \x1b[30;103mtask color black on bright yellow\x1b[0m\n\
         \x20 \x1b[4;36;104mtask color underline cyan on bright blue\x1b[0m\n\n\
         256-color usage (supports underline):\n\
         \x20 \x1b[38;5;214;48;5;202mtask color color214 on color202\x1b[0m\n\
         \x20 \x1b[38;5;82;48;5;28mtask color rgb150 on rgb020\x1b[0m\n\
         \x20 \x1b[4;38;5;242;48;5;235mtask color underline grey10 on grey3\x1b[0m\n\
         \x20 \x1b[38;5;1;48;5;173mtask color red on color173\x1b[0m\n\n\
         Your sample:\n\n\
         \x20 \x1b[31mtask color red\x1b[0m\n\n"
    );

    // Unrecognized colour: the Color error, exit status 2.
    let (out, err, rc) = quiet(&env, &["rc._forcecolor:on", "colors", "bogus"]);
    assert_eq!((out.as_str(), rc), ("", 2));
    assert!(
        err.contains("The color 'bogus' is not recognized."),
        "{err:?}"
    );

    // Legend (any prefix of "legend"), obeying rc overrides.
    let (out, _, rc) = quiet(
        &env,
        &["rc._forcecolor:on", "rc.color.debug:red", "colors", "leg"],
    );
    assert_eq!(rc, 0);
    assert!(
        out.starts_with("\nHere are the colors currently in use:\n"),
        "{out:?}"
    );
    assert!(
        out.contains("color.debug              \x1b[0m \x1b[31mred"),
        "{out:?}"
    );
    assert!(!out.contains("_forcecolor"));
    assert!(out.ends_with("\x1b[0m\n\n"), "{out:?}");

    // The full chart.
    let (out, _, rc) = quiet(&env, &["rc._forcecolor:on", "colors"]);
    assert_eq!(rc, 0);
    assert!(
        out.starts_with("\nBasic colors\n \x1b[30m black \x1b[0m \x1b[31m red \x1b[0m"),
        "{out:?}"
    );
    assert!(out.contains("\n\ncolor0 - color15\n  0 1 2 . . .\n  \x1b[48;5;0m  \x1b[0m"));
    assert!(out.contains("Color cube rgb\x1b[1;31m0\x1b[0m"));
    assert!(out.contains(
        "\x1b[48;5;231m  \x1b[0m \n\nGray ramp gray0 - gray23 (also color232 - color255)\n"
    ));
    assert!(
        out.ends_with("\x1b[48;5;255m  \x1b[0m\n\nTry running 'task color white on red'.\n\n"),
        "{out:?}"
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdUrgency.cpp

#[test]
fn urgency_per_task_and_no_match() {
    let env = Env::new("");
    assert_eq!(quiet(&env, &["add", "one"]).2, 0);
    assert_eq!(quiet(&env, &["add", "two", "+x", "project:P"]).2, 0);
    assert_eq!(
        quiet(&env, &["1", "_urgency"]),
        ("task 1 urgency 0\n".into(), String::new(), 0)
    );
    assert_eq!(
        quiet(&env, &["2", "_urgency"]),
        ("task 2 urgency 1.8\n".into(), String::new(), 0)
    );
    let (out, _, rc) = quiet(&env, &["_urgency"]);
    let mut lines: Vec<&str> = out.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        (lines, rc),
        (vec!["task 1 urgency 0", "task 2 urgency 1.8"], 0)
    );
    // Nothing selected: footnote and exit status 1 (helper commands are quiet).
    let output = env.task(&["99", "_urgency"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        (stdout(&output), stderr(&output)),
        (String::new(), String::new())
    );
}

#[test]
fn information_tables_and_no_match() {
    let env = Env::new("");
    assert_eq!(
        quiet(&env, &["add", "one", "+x", "project:P", "due:2030-01-01"]).2,
        0
    );
    assert_eq!(quiet(&env, &["add", "two", "depends:1"]).2, 0);
    let (out, err, rc) = quiet(&env, &["rc.dateformat.info=Y-M-D", "1", "information"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Name                  Value");
    assert!(lines[1].starts_with("--------------------- ---"), "{out}");
    assert_eq!(lines[2], "ID                    1");
    assert_eq!(lines[3], "Description           one");
    assert_eq!(lines[4], "Status                Pending");
    assert_eq!(lines[5], "Project               P");
    assert_eq!(lines[6], "This task is blocking 2 two");
    assert!(lines.contains(&"Due                   2030-01-01"), "{out}");
    assert!(lines.contains(&"Tags                  x"), "{out}");
    assert!(
        lines.contains(&"Virtual tags          BLOCKING PENDING PROJECT READY TAGGED UNBLOCKED"),
        "{out}"
    );
    // Urgency breakdown and journal.
    assert!(
        lines.contains(&"    project       1 *    1 =      1"),
        "{out}"
    );
    assert!(
        lines.contains(&"    blocking      1 *    8 =      8"),
        "{out}"
    );
    assert!(
        lines.contains(&"                               12.2"),
        "{out}"
    );
    assert!(
        out.contains("Date       Modification\n---------- "),
        "{out}"
    );
    assert!(out.contains("Description set to 'one'."), "{out}");
    assert!(out.contains("Due set to '2030-01-01'."), "{out}");
    assert!(out.contains("Tag 'x' added."), "{out}");
    // rc.journal.info=off drops the journal table.
    let (out, _, _) = quiet(&env, &["rc.journal.info=off", "1", "information"]);
    assert!(!out.contains("Modification"), "{out}");
    // Abbreviated keyword; nothing selected: footnote and exit status 1.
    let output = env.task(&["rc.verbose=footnote", "99", "info"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        (stdout(&output), stderr(&output)),
        (String::new(), "No matches.\n".to_string())
    );
}

#[test]
fn columns_substring_table_and_errors() {
    let env = Env::new("defaultwidth=80\ndetection=off\nfontunderline=off\n");
    // Case-insensitive substring filter; the <uda> rows and footnote text always follow.
    let (out, err, rc) = quiet(&env, &["columns", "PROJ"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        out,
        "Columns Type   Modifiable Supported Formats Example\n\
         ------- ------ ---------- ----------------- -------------\n\
         project string Modifiable full*             home.garden\n\
         \x20                         parent            home\n\
         \x20                         indented            home.garden\n\
         <uda>   <type> Modifiable default*\n\
         \x20                         indicator\n\
         \n\
         * Means default format, and therefore optional.  For example, 'due' and 'due.formatted' are equivalent.\n"
    );
    // Blank line with the default verbosity; `escr` matches only description.
    let output = env.task(&["columns", "escr"]);
    let out = stdout(&output);
    assert!(
        out.starts_with("\nColumns     Type   Modifiable"),
        "{out:?}"
    );
    assert!(
        out.contains("description string Modifiable combined*")
            && !out.contains("uuid")
            && !out.contains("project")
    );
    // fontunderline (default on) without colour underlines the labels instead of dashes.
    let (out, _, _) = quiet(&env, &["rc.fontunderline=on", "columns", "proj"]);
    assert!(
        out.starts_with("\x1b[4mColumns\x1b[0m \x1b[4mType  \x1b[0m"),
        "{out:?}"
    );
    // More than one search word.
    let (out, err, rc) = quiet(&env, &["columns", "a", "b"]);
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "You can only specify one search string.\n", 2)
    );
}

#[test]
fn completion_columns_include_udas() {
    let env = Env::new("uda.zzz.type=string\n");
    let (out, err, rc) = quiet(&env, &["_columns"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    let names: Vec<&str> = out.lines().collect();
    assert_eq!(names.first(), Some(&"depends"));
    assert_eq!(names.last(), Some(&"zzz"));
    assert!(
        names.contains(&"description") && names.contains(&"urgency") && names.contains(&"uuid"),
        "{out}"
    );
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
}

#[test]
fn reports_table_and_count() {
    let env = Env::new("defaultwidth=80\ndetection=off\nfontunderline=off\nreport.mine.columns=id\nreport.mine.description=Mine\n");
    let output = env.task(&["reports"]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "");
    assert_eq!(lines[1], "Report           Description");
    assert_eq!(
        lines[2],
        "---------------- --------------------------------------------------"
    );
    assert_eq!(lines[3], "active           Active tasks");
    assert!(lines.contains(&"mine             Mine"), "{out}");
    assert!(
        lines.contains(&"information      Shows all data and metadata"),
        "{out}"
    );
    assert!(
        out.ends_with("waiting          Waiting (hidden) tasks\n\n30 reports\n"),
        "{out:?}"
    );
    // Forced colour: the header takes color.label, alternate rows color.alternate.
    let (out, _, rc) = quiet(
        &env,
        &[
            "rc._forcecolor=on",
            "rc.color.alternate=on gray2",
            "rc.color.label=bold",
            "reports",
        ],
    );
    assert_eq!(rc, 0);
    assert!(out.starts_with("\x1b[1mReport          \x1b[0m"), "{out:?}");
    assert!(
        out.contains("\x1b[48;5;234mactive          \x1b[0m"),
        "{out:?}"
    );
    assert!(
        out.ends_with("\n30 reports\n") && !out.contains("\n\n30"),
        "{out:?}"
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdCustom: report.* reports

/// The `news` nag footnote CmdCustom adds (`news.version=2.6.0` in the rc).
const NEWS_NAG: &str = "Recently upgraded to 3.5.0. Please run 'task news' to read highlights about the new release.\n";

/// Three tasks with fixed entry dates, for reports run at 2031-06-15 12:00 UTC.
fn report_env(rc: &str) -> Option<Env> {
    let env = Env::new(rc);
    for args in [
        &[
            "rc.verbose=nothing",
            "add",
            "one",
            "entry:2031-06-01",
            "project:Home",
            "+x",
            "due:2031-06-20",
        ][..],
        &[
            "rc.verbose=nothing",
            "add",
            "two",
            "entry:2031-06-14",
            "priority:H",
        ][..],
        &[
            "rc.verbose=nothing",
            "add",
            "three",
            "entry:2031-06-15T11:00:00",
        ][..],
    ] {
        let output = faketime_task(&env, "2031-06-15 12:00:00", args)?;
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    Some(env)
}

#[test]
fn list_report_end_to_end() {
    let Some(env) = report_env("") else {
        return;
    };
    let output = faketime_task(&env, "2031-06-15 12:00:00", &["list"]).unwrap();
    assert_eq!(
        stdout(&output),
        "\n\
         ID Age  P Project Tags Due        Description Urg\n\
         -- ---- - ------- ---- ---------- ----------- ----\n \
         1 2w     Home    x    2031-06-20 one         8.62\n \
         2 1d   H                         two         6.01\n \
         3 1h                             three          0\n\
         \n\
         3 tasks\n"
    );
    assert_eq!(stderr(&output), env.override_headers() + NEWS_NAG);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn next_report_limit_and_affected() {
    let Some(env) = report_env("") else {
        return;
    };
    // limit:N truncates rows; the affected line says how many were shown.
    let output = faketime_task(
        &env,
        "2031-06-15 12:00:00",
        &["rc.verbose=affected,label", "next", "limit:2"],
    )
    .unwrap();
    assert_eq!(
        stdout(&output),
        "ID Age  P Project Tag Due  Description Urg\n\
         -- ---- - ------- --- ---- ----------- ----\n \
         1 2w     Home    x   4d   one         8.62\n \
         2 1d   H                  two         6.01\n\
         3 tasks, 2 shown\n"
    );
    assert_eq!(stderr(&output), "");
    assert_eq!(output.status.code(), Some(0));

    // No matches: footnote and status 1.
    let output = faketime_task(&env, "2031-06-15 12:00:00", &["next", "nosuchword"]).unwrap();
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "No matches.\n" + NEWS_NAG
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn custom_report_labels_sort_breaks_and_limit() {
    let Some(env) = report_env(
        "verbose=label,affected\n\
         report.mine.columns=id,project,description\n\
         report.mine.labels=Num,Proj,What\n\
         report.mine.sort=project-/,description+\n\
         report.mine.filter=status:pending\n",
    ) else {
        return;
    };
    // Custom labels, a descending break column (blank line between groups),
    // and the report filter.
    let output = faketime_task(&env, "2031-06-15 12:00:00", &["mine"]).unwrap();
    assert_eq!(
        stdout(&output),
        "Num Proj What\n--- ---- -----\n  1 Home one\n\n  3      three\n  2      two\n3 tasks\n"
    );
    assert_eq!(output.status.code(), Some(0));

    // limit:N (with the report filter present; a lone `limit:N` term
    // evaluates false in C++ Filter::subset).
    let output = faketime_task(&env, "2031-06-15 12:00:00", &["mine", "limit:2"]).unwrap();
    assert_eq!(
        stdout(&output),
        "Num Proj What\n--- ---- -----\n  1 Home one\n\n  3      three\n3 tasks, 2 shown\n"
    );

    // Forced colour: color.label for headers, color.label.sort for sorted
    // columns (widths are measured over the shown rows only).
    let output = faketime_task(
        &env,
        "2031-06-15 12:00:00",
        &[
            "rc._forcecolor=on",
            "rc.fontunderline=off",
            "rc.color.label=bold",
            "rc.color.label.sort=red",
            "mine",
            "limit:1",
        ],
    )
    .unwrap();
    assert!(
        stdout(&output).starts_with(
            "\x1b[1mNum\x1b[0m \x1b[31mProj\x1b[0m \x1b[31mWhat\x1b[0m\n\x1b[1m---\x1b[0m \x1b[31m----\x1b[0m \x1b[31m----\x1b[0m\n"
        ),
        "{:?}",
        stdout(&output)
    );

    // Mismatched columns and labels.
    let output = env.task(&["rc.verbose=nothing", "rc.report.mine.labels=Num", "mine"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "There are different numbers of columns and labels for report 'mine'.\n"
    );
    assert_eq!(output.status.code(), Some(2));

    // An unknown column.
    let output = env.task(&[
        "rc.verbose=nothing",
        "rc.report.mine.columns=id,bogus",
        "rc.report.mine.labels=",
        "mine",
    ]);
    assert_eq!(stderr(&output), "Unrecognized column name 'bogus'.\n");
    assert_eq!(output.status.code(), Some(2));
}

////////////////////////////////////////////////////////////////////////////////
// sort.cpp, util.cpp setHeaderUnderline and the Column factory, end to end
// through custom reports (`rc.report.X.*` overrides).

/// An environment with tasks added by `task rc.verbose=nothing add <args>`.
fn tasks_env(rc: &str, tasks: &[&[&str]]) -> Env {
    let env = Env::new(rc);
    for args in tasks {
        let mut all = vec!["add"];
        all.extend_from_slice(args);
        let (_, err, rc) = quiet(&env, &all);
        assert_eq!((err.as_str(), rc), ("", 0), "add {args:?}");
    }
    env
}

/// The stdout of the custom report `r` with `columns` and `sort`, run
/// quietly (plus any extra `args`); asserts success and an empty stderr.
fn sorted_report(env: &Env, columns: &str, sort: &str, args: &[&str]) -> String {
    let columns = format!("rc.report.r.columns={columns}");
    let sort = format!("rc.report.r.sort={sort}");
    let mut all = vec![columns.as_str(), sort.as_str()];
    all.extend_from_slice(args);
    all.push("r");
    let (out, err, rc) = quiet(env, &all);
    assert_eq!((err.as_str(), rc), ("", 0), "{all:?}: {out}");
    out
}

#[test]
fn sort_ascending_and_descending() {
    let env = tasks_env("", &[&["bravo"], &["alpha"], &["charlie"]]);
    assert_eq!(
        sorted_report(&env, "id,description", "description+", &[]),
        " 2 alpha\n 1 bravo\n 3 charlie\n"
    );
    assert_eq!(
        sorted_report(&env, "id,description", "description-", &[]),
        " 3 charlie\n 1 bravo\n 2 alpha\n"
    );
    // No direction means ascending.
    assert_eq!(
        sorted_report(&env, "id,description", "description", &[]),
        " 2 alpha\n 1 bravo\n 3 charlie\n"
    );
}

#[test]
fn sort_multiple_keys() {
    let env = tasks_env(
        "",
        &[
            &["b1", "project:B"],
            &["a2", "project:A"],
            &["b2", "project:B"],
            &["a1", "project:A"],
        ],
    );
    // The second key only orders tasks equal on the first.
    assert_eq!(
        sorted_report(&env, "id,project,description", "project+,description-", &[]),
        " 2 A       a2\n 4 A       a1\n 3 B       b2\n 1 B       b1\n"
    );
    assert_eq!(
        sorted_report(&env, "id,project,description", "project-,id+", &[]),
        " 1 B       b1\n 3 B       b2\n 2 A       a2\n 4 A       a1\n"
    );
}

#[test]
fn sort_break_marker_groups_rows() {
    let env = tasks_env(
        "",
        &[
            &["b1", "project:B"],
            &["none"],
            &["a1", "project:A"],
            &["b2", "project:B"],
        ],
    );
    // `/` after the direction: a blank line wherever the project changes.
    assert_eq!(
        sorted_report(&env, "id,project,description", "project+/,id+", &[]),
        " 2         none\n\n 3 A       a1\n\n 1 B       b1\n 4 B       b2\n"
    );
    // A break column without a direction (`project/`) keeps the slash in
    // the key name (C++ Context::decomposeSortField), so it is rejected.
    let (out, err, rc) = quiet(
        &env,
        &["rc.report.r.columns=id", "rc.report.r.sort=project/", "r"],
    );
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "The 'project/' column is not a valid sort field.\n", 2)
    );
}

#[test]
fn sort_uda_custom_values_order() {
    let env = tasks_env(
        "uda.size.type=string\nuda.size.label=Size\nuda.size.values=S,M,,L\n",
        &[
            &["large", "size:L"],
            &["none"],
            &["small", "size:S"],
            &["medium", "size:M"],
        ],
    );
    // Column widths include the label ("Size") even without the `label`
    // verbosity (C++ ViewTask::render).
    // C++ Context stores uda.X.values reversed in Task::customOrder, so
    // ascending runs from the last value to the first; the empty value
    // sorts at its position in the list.
    assert_eq!(
        sorted_report(&env, "id,size,description", "size+", &[]),
        " 1 L    large\n 2      none\n 4 M    medium\n 3 S    small\n"
    );
    assert_eq!(
        sorted_report(&env, "id,size,description", "size-", &[]),
        " 3 S    small\n 4 M    medium\n 2      none\n 1 L    large\n"
    );
    // Without uda.X.values: plain string order, empty values always last.
    assert_eq!(
        sorted_report(
            &env,
            "id,size,description",
            "size+",
            &["rc.uda.size.values="]
        ),
        " 1 L    large\n 4 M    medium\n 3 S    small\n 2      none\n"
    );
    assert_eq!(
        sorted_report(
            &env,
            "id,size,description",
            "size-",
            &["rc.uda.size.values="]
        ),
        " 3 S    small\n 4 M    medium\n 1 L    large\n 2      none\n"
    );
}

#[test]
fn sort_by_depends() {
    let env = tasks_env(
        "",
        &[
            &["first"],
            &["second"],
            &["needs2", "depends:2"],
            &["needs1", "depends:1"],
            &["free"],
        ],
    );
    // Tasks without dependencies first (ascending), then by the ID of the
    // first dependency.
    assert_eq!(
        sorted_report(&env, "id,depends,description", "depends+,id+", &[]),
        " 1         first\n 2         second\n 5         free\n 4 1       needs1\n 3 2       needs2\n"
    );
    assert_eq!(
        sorted_report(&env, "id,depends,description", "depends-,id+", &[]),
        " 3 2       needs2\n 4 1       needs1\n 1         first\n 2         second\n 5         free\n"
    );
}

#[test]
fn sort_by_urgency() {
    let env = tasks_env(
        "urgency.age.coefficient=0\n",
        &[
            &["low", "priority:L"],
            &["high", "priority:H"],
            &["none"],
            &["medium", "priority:M"],
        ],
    );
    assert_eq!(
        sorted_report(&env, "id,urgency,description", "urgency-", &[]),
        " 2       6 high\n 4     3.9 medium\n 1     1.8 low\n 3       0 none\n"
    );
    assert_eq!(
        sorted_report(&env, "id,urgency,description", "urgency+", &[]),
        " 3       0 none\n 1     1.8 low\n 4     3.9 medium\n 2       6 high\n"
    );
}

#[test]
fn sort_ids_numerically() {
    let tasks: Vec<Vec<String>> = (1..=11).map(|i| vec![format!("t{i}")]).collect();
    let tasks: Vec<Vec<&str>> = tasks
        .iter()
        .map(|t| t.iter().map(String::as_str).collect())
        .collect();
    let tasks: Vec<&[&str]> = tasks.iter().map(Vec::as_slice).collect();
    let env = tasks_env("", &tasks);
    // Natural (numeric) ID order: 1, 2, 10, 11 rather than 1, 10, 11, 2.
    assert_eq!(
        sorted_report(&env, "id,description", "id+", &["1,2,10,11"]),
        " 1 t1\n 2 t2\n10 t10\n11 t11\n"
    );
    assert_eq!(
        sorted_report(&env, "id,description", "id-", &["1,2,10,11"]),
        "11 t11\n10 t10\n 2 t2\n 1 t1\n"
    );
    // Descriptions sort as strings.
    assert_eq!(
        sorted_report(&env, "id,description", "description+", &["1,2,10,11"]),
        " 1 t1\n10 t10\n11 t11\n 2 t2\n"
    );
}

#[test]
fn sort_projects_with_hierarchy() {
    let env = tasks_env(
        "",
        &[
            &["work", "project:Work"],
            &["garden", "project:Home.Garden"],
            &["home", "project:Home"],
            &["hw", "project:Homework"],
            &["none"],
        ],
    );
    // Project names compare as strings: a subproject follows its parent
    // ('.' sorts before letters), empty projects first.
    assert_eq!(
        sorted_report(&env, "id,project,description", "project+", &[]),
        " 5             none\n 3 Home        home\n 2 Home.Garden garden\n 4 Homework    hw\n 1 Work        work\n"
    );
    assert_eq!(
        sorted_report(&env, "id,project.indented,description", "project-", &[]),
        " 1 Work     work\n 4 Homework hw\n 2   Garden garden\n 3 Home     home\n 5          none\n"
    );
}

#[test]
fn report_header_underline_color_and_fontunderline() {
    let env = tasks_env(
        "color.label=bold\ncolor.label.sort=red\ncolor.alternate=on gray2\n",
        &[&["one"], &["two"]],
    );
    let header = |color: &str, underline: &str| {
        let color = format!("rc._forcecolor={color}");
        let underline = format!("rc.fontunderline={underline}");
        sorted_report(
            &env,
            "id,description",
            "id+",
            &["rc.verbose=label", &color, &underline],
        )
    };
    // No colour: dashes, whatever fontunderline says.
    let plain = "ID Description\n-- -----------\n 1 one\n 2 two\n";
    assert_eq!(header("off", "off"), plain);
    assert_eq!(header("off", "on"), plain);
    // Rows are trimmed on the right (C++ ViewTask::render), so only the
    // coloured alternate row keeps its padding.
    // Colour without fontunderline: label colours (color.label.sort for the
    // sorted ID column) on both the label and the dashes.
    assert_eq!(
        header("on", "off"),
        "\x1b[31mID\x1b[0m \x1b[1mDescription\x1b[0m\n\
         \x1b[31m--\x1b[0m \x1b[1m-----------\x1b[0m\n \
         1 one\n\
         \x1b[48;5;234m 2\x1b[0m\x1b[48;5;234m \x1b[0m\x1b[48;5;234mtwo        \x1b[0m\n"
    );
    // Colour and fontunderline: underlined labels, no dash line.
    assert_eq!(
        header("on", "on"),
        "\x1b[4;31mID\x1b[0m \x1b[1;4mDescription\x1b[0m\n \
         1 one\n\
         \x1b[48;5;234m 2\x1b[0m\x1b[48;5;234m \x1b[0m\x1b[48;5;234mtwo        \x1b[0m\n"
    );
}

#[test]
fn set_header_underline_on_reports_table() {
    let env =
        Env::new("defaultwidth=80\ndetection=off\ncolor.label=bold\ncolor.alternate=on gray2\n");
    let head = |color: &str, underline: &str| {
        let color = format!("rc._forcecolor={color}");
        let underline = format!("rc.fontunderline={underline}");
        let (out, err, rc) = quiet(&env, &[&color, &underline, "reports"]);
        assert_eq!((err.as_str(), rc), ("", 0));
        out.lines().take(3).collect::<Vec<_>>().join("\n")
    };
    let label = "Report          ";
    let desc = "Description                                       ";
    assert_eq!(
        head("off", "off"),
        format!(
            "Report           Description\n{} {}\nactive           Active tasks",
            "-".repeat(16),
            "-".repeat(50)
        )
    );
    // Without colour C++ still applies Color("underline") to the header:
    // CmdReports never calls Table::withColor (whose default is true).
    assert_eq!(
        head("off", "on"),
        format!("\x1b[4m{label}\x1b[0m \x1b[4m{desc}\x1b[0m\nactive           Active tasks\nall              All tasks")
    );
    let alt = "\x1b[48;5;234mactive          \x1b[0m\x1b[48;5;234m \x1b[0m\x1b[48;5;234mActive tasks                                      \x1b[0m";
    assert_eq!(
        head("on", "off"),
        format!(
            "\x1b[1m{label}\x1b[0m \x1b[1m{desc}\x1b[0m\n\x1b[1m{}\x1b[0m \x1b[1m{}\x1b[0m\n{alt}",
            "-".repeat(16),
            "-".repeat(50)
        )
    );
    assert_eq!(
        head("on", "on"),
        format!(
            "\x1b[1;4m{label}\x1b[0m \x1b[1;4m{desc}\x1b[0m\n{alt}\nall              All tasks"
        )
    );
}

#[test]
fn report_uda_columns_of_each_type() {
    let env = tasks_env(
        "uda.size.type=string\nuda.size.label=Size\n\
         uda.cost.type=numeric\nuda.cost.label=Cost\n\
         uda.when.type=date\nuda.when.label=When\n\
         uda.span.type=duration\nuda.span.label=Span\n\
         uda.ref.type=uuid\nuda.ref.label=Ref\n",
        &[
            &[
                "one",
                "size:big",
                "cost:3",
                "when:2030-01-02",
                "span:2d",
                "ref:a0b1c2d3-e4f5-4789-abcd-0123456789ab",
            ],
            &["two", "cost:12.5", "span:10d"],
        ],
    );
    // Durations render right-aligned in ISO form; dates in rc.dateformat.
    let columns = "id,size,cost,when,span,ref,ref.short,description";
    assert_eq!(
        sorted_report(
            &env,
            columns,
            "id+",
            &["rc.verbose=label", "rc.defaultwidth=120"]
        ),
        "ID Size Cost When       Span Ref                                  Ref      Description\n\
         -- ---- ---- ---------- ---- ------------------------------------ -------- -----------\n \
         1 big     3 2030-01-02  P2D a0b1c2d3-e4f5-4789-abcd-0123456789ab a0b1c2d3 one\n \
         2      12.5            P10D                                               two\n"
    );
    // UDA styles: indicators (right-aligned, default "U"; widths include
    // the labels), and an unknown style.
    assert_eq!(
        sorted_report(
            &env,
            "id,size.indicator,when.indicator,description",
            "id+",
            &["rc.uda.when.indicator=W"]
        ),
        " 1    U    W one\n 2           two\n"
    );
    let (_, err, rc) = quiet(&env, &["rc.report.r.columns=id,cost.donkey", "r"]);
    assert_eq!(
        (err.as_str(), rc),
        ("Unrecognized column format 'cost.donkey'\n", 2)
    );
}

#[test]
fn report_column_factory_errors() {
    let env = tasks_env("", &[&["one"], &["two"]]);
    let (out, err, rc) = quiet(&env, &["rc.report.r.columns=id,bogus", "r"]);
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "Unrecognized column name 'bogus'.\n", 2)
    );
    let (out, err, rc) = quiet(&env, &["rc.report.r.columns=id,description.donkey", "r"]);
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "Unrecognized column format 'description.donkey'\n", 2)
    );
    // An unknown sort key fails in the comparator (C++ validateSortColumns
    // only maps legacy names), so it needs two tasks to compare.
    let (out, err, rc) = quiet(
        &env,
        &["rc.report.r.columns=id", "rc.report.r.sort=bogus+", "r"],
    );
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "The 'bogus' column is not a valid sort field.\n", 2)
    );
    let (out, err, rc) = quiet(
        &env,
        &[
            "rc.report.r.columns=id",
            "rc.report.r.sort=bogus+",
            "r",
            "1",
        ],
    );
    assert_eq!((out.as_str(), err.as_str(), rc), (" 1\n", "", 0));
}

#[test]
fn report_without_labels_uses_column_labels() {
    let env = tasks_env(
        "uda.size.type=string\nuda.size.label=Size\nuda.nolabel.type=string\n",
        &[&["one", "project:P", "+t", "size:x", "nolabel:y"]],
    );
    // Each column's default label (Column metadata); a UDA without
    // uda.X.label has an empty one, rendered as blanks (C++ ViewTask::render).
    assert_eq!(
        sorted_report(
            &env,
            "id,project,tags,status,size,nolabel,description",
            "id+",
            &["rc.verbose=label"]
        ),
        "ID Project Tags Status  Size   Description\n\
         -- ------- ---- ------- ----   -----------\n \
         1 P       t    Pending x    y one\n"
    );
}

/// Writes an executable `/bin/sh` hook script `<tmp>/hooks/<name>`.
fn hook_script(env: &Env, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let dir = env.path("hooks");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn hooks_env(env_rc: &str) -> Env {
    let env = Env::new(env_rc);
    let rc = std::fs::read_to_string(env.rc())
        .unwrap()
        .replace("hooks=off\n", "");
    let hooks = env.path("hooks");
    env.write(
        "test.rc",
        &format!("{rc}hooks=on\nhooks.location={}\n", hooks.display()),
    );
    env
}

#[test]
fn on_add_hook_modifies_the_task_and_gives_feedback() {
    let env = hooks_env("");
    hook_script(
        &env,
        "on-add",
        "read line; echo \"$line\" | sed 's/\"first\"/\"changed\"/'; echo 'hook says hi'",
    );
    let output = env.task(&["add", "first"]);
    assert_eq!(stdout(&output), "Created task 1.\n");
    assert_eq!(stderr(&output), env.override_headers() + "hook says hi\n");
    assert_eq!(output.status.code(), Some(0));

    let output = env.task(&["_get", "1.description"]);
    assert_eq!(stdout(&output), "changed\n");
}

#[test]
fn failing_on_launch_hook_stops_with_status_4() {
    let env = hooks_env("");
    hook_script(&env, "on-launch", "echo 'not today'; exit 1");
    let output = env.task(&["add", "first"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), env.override_headers() + "not today\n");
    assert_eq!(output.status.code(), Some(4));

    // Nothing was added.
    std::fs::remove_file(env.path("hooks/on-launch")).unwrap();
    let output = env.task(&["count"]);
    assert_eq!(stdout(&output), "0\n");
}

#[test]
fn on_exit_hook_reads_the_changed_tasks_and_rejects_json() {
    let env = hooks_env("");
    hook_script(&env, "on-exit", "while read line; do echo \"$line\" | grep -q '\"description\":\"first\"' && echo saw first; done");
    let output = env.task(&["add", "first"]);
    assert_eq!(stdout(&output), "Created task 1.\n");
    assert_eq!(stderr(&output), env.override_headers() + "saw first\n");
    assert_eq!(output.status.code(), Some(0));

    // JSON output from on-exit is an error, status 4; the command output is
    // still printed, as in C++ `Context::run`.
    hook_script(&env, "on-exit", "echo '{\"a\":1}'");
    let output = env.task(&["count"]);
    assert_eq!(stdout(&output), "1\n");
    assert_eq!(
        stderr(&output),
        env.override_headers()
            + "Hook Error: Expected 0 JSON task(s), found 1, in hook script: on-exit\n"
    );
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn until_expires_tasks_before_reports() {
    let env = Env::new("");
    assert_eq!(quiet(&env, &["add", "gone", "until:now-1h"]).2, 0);
    assert_eq!(quiet(&env, &["add", "kept", "until:now+1h"]).2, 0);
    // `count` runs handleUntil first: the expired task is deleted. (An
    // unfiltered count includes deleted tasks, as in C++.)
    assert_eq!(quiet(&env, &["count"]).0, "2\n");
    assert_eq!(quiet(&env, &["status:pending", "count"]).0, "1\n");
    assert_eq!(quiet(&env, &["status:deleted", "count"]).0, "1\n");
    assert_eq!(
        quiet(&env, &["status:deleted", "_unique", "description"]).0,
        "gone\n"
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdModify.cpp: modify

/// `task <args>` (with `TASKRC`/`TASKDATA`) reading `stdin` as input.
fn task_input(env: &Env, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = env
        .command(args)
        .env("TASKRC", env.rc())
        .env("TASKDATA", env.data())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn modify_feedback_substitution_and_project_change() {
    let env = Env::new("confirmation=off\n");
    quiet(&env, &["add", "one", "project:home"]);
    quiet(&env, &["add", "two"]);

    let output = env.task(&["1", "modify", "/one/uno/", "project:work"]);
    assert_eq!(
        stdout(&output),
        "Modifying task 1 'uno'.\nModified 1 task.\n"
    );
    assert_eq!(
        stderr(&output),
        env.override_headers()
            + "The project 'home' has changed.  Project 'home' is 0% complete (0 of 0 tasks remaining).\n"
            + "The project 'work' has changed.  Project 'work' is 0% complete (1 task remaining).\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let output = env.task(&["rc.verbose=affected", "1-2", "modify", "+x"]);
    assert_eq!(
        stdout(&output),
        "This command will alter 2 tasks.\nModifying task 1 'uno'.\nModifying task 2 'two'.\nModified 2 tasks.\n"
    );
    let tasks = export(&env, &[]);
    assert_eq!(tasks[0]["description"], "uno");
    assert_eq!(tasks[0]["project"], "work");
    assert_eq!(tasks[1]["tags"], serde_json::json!(["x"]));

    // An unchanged task is not counted.
    assert_eq!(
        quiet(&env, &["rc.verbose=affected", "1", "modify", "uno"]),
        ("Modified 0 tasks.\n".into(), String::new(), 0)
    );
}

#[test]
fn modify_errors_and_exit_codes() {
    let env = Env::new("");
    quiet(&env, &["add", "one"]);

    let output = env.task(&["99", "modify", "x"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "No tasks specified.\n"
    );
    assert_eq!(output.status.code(), Some(1));

    let output = env.task(&["1", "modify", "recur:daily"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "You cannot specify a recurring task without a due date.\n"
    );
    assert_eq!(output.status.code(), Some(2));

    let output = env.task(&["1", "modify", "end:now"]);
    assert_eq!(
        stderr(&output),
        env.override_headers()
            + "Could not modify task 1. You cannot set an end date on a pending task.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn modify_bulk_confirmation_yes_then_quit() {
    let env = Env::new("bulk=2\nconfirmation=on\nverbose=affected,blank\n");
    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two"]);

    let output = task_input(&env, &["1-2", "modify", "+y"], "yes\nquit\n");
    assert_eq!(
        stdout(&output),
        "This command will alter 2 tasks.\n\
         Tag y will be added.\n\
         Modify task 1 'one'? (yes/no/all/quit) Modifying task 1 'one'.\n\
         \n\
         Tag y will be added.\n\
         Modify task 2 'two'? (yes/no/all/quit) Task not modified.\n\
         Modified 1 task.\n"
    );
    assert_eq!(output.status.code(), Some(1));
    let tasks = export(&env, &[]);
    assert_eq!(tasks[0]["tags"], serde_json::json!(["y"]));
    assert!(tasks[1].get("tags").is_none());
}

#[test]
fn modify_recurring_child_propagates_with_confirmation() {
    let env = Env::new("recurrence.confirmation=yes\nrecurrence.limit=2\n");
    quiet(&env, &["add", "rec", "due:yesterday", "recur:daily"]);
    // The `list` report runs the recurrence handler, creating the children.
    quiet(&env, &["list"]);

    let (out, _, code) = quiet(&env, &["rc.verbose=affected", "2", "modify", "project:p"]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with("Modifying task 2 'rec'.\nModifying recurring task "),
        "{out}"
    );
    let tasks = export(&env, &[]);
    assert!(tasks.len() >= 3, "{tasks:?}");
    for task in &tasks {
        assert_eq!(task["project"], "p", "{task}");
    }

    // rc.recurrence.confirmation=no only modifies the selected task.
    let (_, _, code) = quiet(
        &env,
        &["rc.recurrence.confirmation=no", "2", "modify", "project:q"],
    );
    assert_eq!(code, 0);
    let tasks = export(&env, &[]);
    let changed: Vec<_> = tasks.iter().filter(|t| t["project"] == "q").collect();
    assert_eq!(changed.len(), 1);
}

////////////////////////////////////////////////////////////////////////////////
// CmdDone.cpp: done

#[test]
fn done_feedback_unblocked_project_and_exit_codes() {
    let env = Env::new("confirmation=off\n");
    quiet(&env, &["add", "one", "project:home"]);
    quiet(&env, &["add", "two", "depends:1"]);

    let output = env.task(&["1", "done"]);
    assert_eq!(
        stdout(&output),
        "Completed task 1 'one'.\nUnblocked 2 'two'.\nCompleted 1 task.\n"
    );
    assert_eq!(
        stderr(&output),
        env.override_headers()
            + "The project 'home' has changed.  Project 'home' is 100% complete (0 of 1 tasks remaining).\n"
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &[]);
    assert_eq!(tasks[0]["status"], "completed");
    assert!(tasks[0].get("end").is_some());

    // Already completed: not pending nor waiting, exit 1.
    let uuid = tasks[0]["uuid"].as_str().unwrap().to_string();
    let output = env.task(&[&uuid, "done"]);
    assert_eq!(
        stdout(&output),
        format!(
            "Task {} 'one' is neither pending nor waiting.\nCompleted 0 tasks.\n",
            &uuid[..8]
        )
    );
    assert_eq!(output.status.code(), Some(1));

    let output = env.task(&["99", "done"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "No tasks specified.\n"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn done_stops_with_journal_annotation_and_bulk_quit() {
    let env = Env::new("journal.time=on\nbulk=2\nconfirmation=on\nverbose=affected\n");
    quiet(&env, &["add", "one", "start:now"]);
    quiet(&env, &["add", "two"]);

    let output = task_input(&env, &["1-2", "done"], "no\nquit\n");
    let out = stdout(&output);
    assert!(
        out.starts_with("This command will alter 2 tasks.\n"),
        "{out}"
    );
    assert!(
        out.contains("Complete task 1 'one'? (yes/no/all/quit) Task not completed.\n"),
        "{out}"
    );
    assert!(
        out.ends_with(
            "Complete task 2 'two'? (yes/no/all/quit) Task not completed.\nCompleted 0 tasks.\n"
        ),
        "{out}"
    );
    assert_eq!(output.status.code(), Some(1));

    let (out, _, code) = quiet(
        &env,
        &["rc.verbose=affected", "rc.confirmation=off", "1", "done"],
    );
    assert_eq!(out, "Completed task 1 'one'.\nCompleted 1 task.\n");
    assert_eq!(code, 0);
    let tasks = export(&env, &["status:completed"]);
    assert!(tasks[0].get("start").is_none());
    assert_eq!(tasks[0]["annotations"][0]["description"], "Stopped task");
}

////////////////////////////////////////////////////////////////////////////////
// CmdDelete.cpp: delete

#[test]
fn delete_confirmation_and_not_deletable() {
    let env = Env::new("confirmation=on\nverbose=affected\n");
    quiet(&env, &["add", "one"]);

    let output = task_input(&env, &["1", "delete"], "no\n");
    assert_eq!(
        stdout(&output),
        "Delete task 1 'one'? (yes/no) Task not deleted.\nDeleted 0 tasks.\n"
    );
    assert_eq!(output.status.code(), Some(1));

    let output = task_input(&env, &["1", "delete"], "yes\n");
    assert_eq!(
        stdout(&output),
        "Delete task 1 'one'? (yes/no) Deleting task 1 'one'.\nDeleted 1 task.\n"
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &[]);
    assert_eq!(tasks[0]["status"], "deleted");
    assert!(tasks[0].get("end").is_some());

    let uuid = tasks[0]["uuid"].as_str().unwrap().to_string();
    let (out, _, code) = quiet(
        &env,
        &[
            "rc.verbose=affected",
            "rc.confirmation=off",
            &uuid,
            "delete",
        ],
    );
    assert_eq!(
        out,
        format!(
            "Task {} 'one' is not deletable.\nDeleted 0 tasks.\n",
            &uuid[..8]
        )
    );
    assert_eq!(code, 1);
}

#[test]
fn delete_recurring_child_with_and_without_siblings() {
    let env = Env::new("confirmation=off\nrecurrence.limit=2\nverbose=affected\n");
    quiet(&env, &["add", "rec", "due:yesterday", "recur:daily"]);
    quiet(&env, &["list"]);
    let children: Vec<String> = export(&env, &["status:pending"])
        .iter()
        .map(|t| t["uuid"].as_str().unwrap().to_string())
        .collect();
    assert!(children.len() >= 2, "{children:?}");

    // rc.recurrence.confirmation=no: only the selected child.
    let (out, _, code) = quiet(
        &env,
        &[
            "rc.verbose=affected",
            "rc.recurrence.confirmation=no",
            &children[0],
            "delete",
        ],
    );
    assert_eq!(out, "Deleting task 2 'rec'.\nDeleted 1 task.\n");
    assert_eq!(code, 0);
    assert_eq!(export(&env, &["status:pending"]).len(), children.len() - 1);
    // The parent mask records the deletion, so it is not regenerated.
    let parent = export(&env, &["status:recurring"]);
    assert!(
        parent[0]["mask"].as_str().unwrap().starts_with('X'),
        "{parent:?}"
    );

    // Prompt answered yes: every sibling and the parent template.
    let output = task_input(
        &env,
        &["rc.recurrence.confirmation=prompt", &children[1], "delete"],
        "yes\n",
    );
    let out = stdout(&output);
    assert!(
        out.contains(
            "This is a recurring task.  Do you want to delete all pending recurrences of this same task? (yes/no) Deleting recurring task "
        ),
        "{out}"
    );
    assert!(
        out.ends_with(&format!("Deleted {} tasks.\n", children.len() - 1)),
        "{out}"
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(export(&env, &["status:pending"]).is_empty());
    assert!(export(&env, &["status:recurring"]).is_empty());
}

////////////////////////////////////////////////////////////////////////////////
// CmdStart.cpp: start

#[test]
fn start_feedback_already_started_and_exit_codes() {
    let env = Env::new("confirmation=off\nverbose=affected,project\n");
    quiet(&env, &["add", "one", "project:home"]);

    let output = env.task(&["1", "start"]);
    assert_eq!(stdout(&output), "Starting task 1 'one'.\nStarted 1 task.\n");
    assert_eq!(
        stderr(&output),
        "Project 'home' is 0% complete (1 task remaining).\n"
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &[]);
    assert!(tasks[0].get("start").is_some());

    let output = env.task(&["1", "start"]);
    assert_eq!(
        stdout(&output),
        "Task 1 'one' already started.\nStarted 0 tasks.\n"
    );
    assert_eq!(output.status.code(), Some(1));

    let output = env.task(&["99", "start"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "No tasks specified.\n");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn start_journal_annotation_reopens_completed_and_bulk_quit() {
    let env = Env::new("journal.time=on\nbulk=2\nconfirmation=on\nverbose=affected\n");
    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two"]);

    let output = task_input(&env, &["1-2", "start"], "quit\n");
    let out = stdout(&output);
    assert!(
        out.starts_with("This command will alter 2 tasks.\n"),
        "{out}"
    );
    assert!(
        out.ends_with(
            "Start task 1 'one'? (yes/no/all/quit) Task not started.\nStarted 0 tasks.\n"
        ),
        "{out}"
    );
    assert_eq!(output.status.code(), Some(1));

    quiet(&env, &["rc.confirmation=off", "2", "done"]);
    let uuid = export(&env, &["status:completed"])[0]["uuid"]
        .as_str()
        .unwrap()
        .to_string();
    let (out, _, code) = quiet(
        &env,
        &["rc.verbose=affected", "rc.confirmation=off", &uuid, "start"],
    );
    // Like C++, the reopened task keeps id 0, so the short UUID is shown.
    assert_eq!(
        out,
        format!("Starting task {} 'two'.\nStarted 1 task.\n", &uuid[..8])
    );
    assert_eq!(code, 0);
    let tasks = export(&env, &["2"]);
    assert_eq!(tasks[0]["status"], "pending");
    assert_eq!(tasks[0]["annotations"][0]["description"], "Started task");
}

////////////////////////////////////////////////////////////////////////////////
// CmdStop.cpp: stop

#[test]
fn stop_feedback_not_started_journal_and_exit_codes() {
    let env = Env::new("confirmation=off\nverbose=affected\njournal.time=on\n");
    quiet(&env, &["add", "one", "start:now"]);
    quiet(&env, &["add", "two"]);

    let output = env.task(&["1", "stop"]);
    assert_eq!(stdout(&output), "Stopping task 1 'one'.\nStopped 1 task.\n");
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &["1"]);
    assert!(tasks[0].get("start").is_none());
    assert_eq!(tasks[0]["annotations"][0]["description"], "Stopped task");

    let output = env.task(&["1-2", "stop"]);
    assert_eq!(
        stdout(&output),
        "This command will alter 2 tasks.\nTask 1 'one' not started.\nTask 2 'two' not started.\nStopped 0 tasks.\n"
    );
    assert_eq!(output.status.code(), Some(1));

    let output = env.task(&["99", "stop"]);
    assert_eq!(stderr(&output), "No tasks specified.\n");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn stop_declined_permission() {
    let env = Env::new("confirmation=on\nbulk=2\nverbose=affected\n");
    quiet(&env, &["add", "one", "start:now"]);
    quiet(&env, &["add", "two", "start:now"]);
    let output = task_input(&env, &["1-2", "stop"], "no\nno\n");
    let out = stdout(&output);
    assert!(
        out.contains("Stop task 1 'one'? (yes/no/all/quit) Task not stopped.\n"),
        "{out}"
    );
    assert!(
        out.ends_with("Stop task 2 'two'? (yes/no/all/quit) Task not stopped.\nStopped 0 tasks.\n"),
        "{out}"
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(export(&env, &["+ACTIVE"]).len(), 2);
}

#[test]
fn annotate_feedback_recurring_siblings_and_exit_codes() {
    let env = Env::new("confirmation=off\nverbose=affected,project\n");
    quiet(&env, &["add", "one", "project:home"]);

    let output = env.task(&["1", "annotate", "note", "text"]);
    assert_eq!(
        stdout(&output),
        "Annotating task 1 'one'.\nAnnotated 1 task.\n"
    );
    assert_eq!(
        stderr(&output),
        "Project 'home' is 0% complete (1 task remaining).\n"
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &["1"]);
    assert_eq!(tasks[0]["annotations"][0]["description"], "note text");

    let output = env.task(&["1", "annotate"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "Additional text must be provided.\n");
    assert_eq!(output.status.code(), Some(2));

    let output = env.task(&["99", "annotate", "x"]);
    assert_eq!(stderr(&output), "No tasks specified.\n");
    assert_eq!(output.status.code(), Some(1));

    // Recurring child: siblings and parent are annotated too.
    quiet(&env, &["add", "rec", "due:tomorrow", "recur:daily"]);
    quiet(&env, &["list"]);
    let output = env.task(&[
        "rc.recurrence.confirmation=yes",
        "status:pending",
        "description:rec",
        "annotate",
        "shared",
    ]);
    let out = stdout(&output);
    assert!(out.starts_with("Annotating task "), "{out}");
    assert!(out.ends_with("Annotated 1 task.\n"), "{out}");
    assert_eq!(output.status.code(), Some(0));
    for task in export(&env, &["description:rec"]) {
        assert_eq!(task["annotations"][0]["description"], "shared", "{task}");
    }
}

#[test]
fn annotate_bulk_quit() {
    let env = Env::new("bulk=2\nconfirmation=on\nverbose=affected\n");
    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two"]);

    let output = task_input(&env, &["1-2", "annotate", "x"], "quit\n");
    let out = stdout(&output);
    assert!(
        out.starts_with("This command will alter 2 tasks.\n"),
        "{out}"
    );
    assert!(
        out.ends_with(
            "Annotate task 1 'one'? (yes/no/all/quit) Task not annotated.\nAnnotated 0 tasks.\n"
        ),
        "{out}"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn denotate_exact_partial_and_no_match() {
    let env = Env::new("confirmation=off\nverbose=affected\n");
    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two"]);
    quiet(&env, &["1", "annotate", "foo bar"]);
    quiet(&env, &["1", "annotate", "foo"]);

    let output = env.task(&["1", "denotate", "foo"]);
    assert_eq!(
        stdout(&output),
        "Found annotation 'foo' and deleted it.\nDenotated 1 task.\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let output = env.task(&["1", "denotate", "zzz"]);
    assert_eq!(
        stdout(&output),
        "Did not find any matching annotation to be deleted for 'zzz'.\nDenotated 0 tasks.\n"
    );
    assert_eq!(output.status.code(), Some(1));

    let output = env.task(&["1", "denotate", "bar"]);
    assert_eq!(
        stdout(&output),
        "Found annotation 'foo bar' and deleted it.\nDenotated 1 task.\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let output = env.task(&["2", "denotate", "foo"]);
    assert_eq!(
        stderr(&output),
        "The specified task has no annotations that can be deleted.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn append_and_prepend_feedback_and_exit_codes() {
    let env = Env::new("confirmation=off\nverbose=affected\n");
    quiet(&env, &["add", "one"]);

    let output = env.task(&["1", "append", "tail"]);
    assert_eq!(
        stdout(&output),
        "Appending to task 1 'one tail'.\nAppended 1 task.\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let output = env.task(&["1", "prepend", "head"]);
    assert_eq!(
        stdout(&output),
        "Prepending to task 1 'head one tail'.\nPrepended 1 task.\n"
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(export(&env, &["1"])[0]["description"], "head one tail");

    for command in ["append", "prepend"] {
        let output = env.task(&["1", command]);
        assert_eq!(stderr(&output), "Additional text must be provided.\n");
        assert_eq!(output.status.code(), Some(2));
    }

    quiet(&env, &["add", "two"]);
    let output = task_input(
        &env,
        &["rc.confirmation=on", "rc.bulk=2", "1-2", "prepend", "x"],
        "no\nno\n",
    );
    let out = stdout(&output);
    assert_eq!(out.matches("Task not prepended.\n").count(), 2, "{out}");
    assert!(out.ends_with("Prepended 0 tasks.\n"), "{out}");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn duplicate_feedback_recurring_and_exit_codes() {
    let env = Env::new("confirmation=off\nverbose=affected,new-id,project\n");
    quiet(&env, &["add", "one", "project:home"]);

    let output = env.task(&["1", "duplicate", "+copy"]);
    assert_eq!(
        stdout(&output),
        "Duplicated task 1 'one'.\nCreated task 2.\nDuplicated 1 task.\n"
    );
    assert_eq!(
        stderr(&output),
        "The project 'home' has changed.  Project 'home' is 0% complete (2 of 2 tasks remaining).\n"
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &["2"]);
    assert_eq!(tasks[0]["description"], "one");
    assert_eq!(tasks[0]["tags"][0], "copy");

    let output = env.task(&["99", "duplicate"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "No tasks specified.\n");
    assert_eq!(output.status.code(), Some(1));

    // A recurring child becomes a plain task.
    quiet(&env, &["add", "rec", "due:tomorrow", "recur:daily"]);
    quiet(&env, &["list"]);
    let child = export(&env, &["status:pending", "description:rec"]);
    let child_id = child[0]["id"].as_i64().unwrap().to_string();
    let output = env.task(&[&child_id, "duplicate"]);
    let out = stdout(&output);
    assert!(
        out.starts_with(&format!(
            "Note: task {child_id} was a recurring task.  The duplicated task is not.\n"
        )),
        "{out}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn duplicate_declined_is_rc_1() {
    let env = Env::new("bulk=1\nconfirmation=on\nverbose=affected\n");
    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two"]);

    let output = task_input(&env, &["1-2", "duplicate"], "quit\n");
    assert_eq!(
        stdout(&output),
        "Duplicate task 1 'one'? (yes/no/all/quit) Task not duplicated.\nDuplicated 0 tasks.\n"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn import_stdin_files_update_and_errors() {
    let env = Env::new("");
    let output = task_input(
        &env,
        &["import"],
        r#"[{"uuid":"a0000000-a000-a000-a000-a00000000000","description":"zero"},{"uuid":"a1111111-a111-a111-a111-a11111111111","description":"one"}]"#,
    );
    assert_eq!(
        stdout(&output),
        "Importing 'STDIN'\n add  a0000000-a000-a000-a000-a00000000000 zero\n add  a1111111-a111-a111-a111-a11111111111 one\n"
    );
    assert_eq!(
        stderr(&output),
        env.override_headers() + "Imported 2 tasks.\n"
    );
    assert_eq!(output.status.code(), Some(0));

    // Line-delimited objects from a file: one update, one unchanged.
    env.write(
        "tasks.json",
        "{\"uuid\":\"a0000000-a000-a000-a000-a00000000000\",\"description\":\"zero\",\"project\":\"p\"}\n{\"uuid\":\"a1111111-a111-a111-a111-a11111111111\",\"description\":\"one\"}\n",
    );
    let output = env.task(&["import", "tasks.json"]);
    assert_eq!(
        stdout(&output),
        "Importing 'tasks.json'\nModifying task a0000000 'zero'.\n mod  a0000000-a000-a000-a000-a00000000000 zero\n skip a1111111-a111-a111-a111-a11111111111 one\n"
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &["a0000000-a000-a000-a000-a00000000000"]);
    assert_eq!(tasks[0]["project"], "p");

    let output = env.task(&["import", "missing.json"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "File 'missing.json' not found.\n"
    );
    assert_eq!(output.status.code(), Some(2));

    let output = task_input(&env, &["import"], "{}");
    assert_eq!(stdout(&output), "Importing 'STDIN'\n");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "Cannot import an empty task.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn import_v2_reads_data_files() {
    let env = Env::new("");
    std::fs::write(
        env.data().join("pending.data"),
        "[description:\"bing\" entry:\"1734397061\" status:\"pending\" uuid:\"ad7f7585-bff3-4b57-a116-abfc9f71ee4a\"]\n",
    )
    .unwrap();
    std::fs::write(
        env.data().join("completed.data"),
        "[description:\"bar\" end:\"1734397065\" entry:\"1734397056\" status:\"completed\" uuid:\"51921813-7abb-412d-8ada-7c1417d01209\"]\n[description:\"orphan\"]\n",
    )
    .unwrap();
    let output = env.task(&["import-v2"]);
    assert_eq!(
        stdout(&output),
        " add ad7f7585-bff3-4b57-a116-abfc9f71ee4a bing\n add 51921813-7abb-412d-8ada-7c1417d01209 bar\n err  - Task with no UUID\n"
    );
    assert_eq!(
        stderr(&output),
        env.override_headers()
            + "Imported 2 tasks from `*.data` files. You may now delete these files.\n"
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &["51921813-7abb-412d-8ada-7c1417d01209"]);
    assert_eq!(tasks[0]["status"], "completed");

    // A second run updates the same tasks.
    std::fs::remove_file(env.data().join("completed.data")).unwrap();
    let output = env.task(&["import-v2"]);
    assert_eq!(
        stdout(&output),
        " mod ad7f7585-bff3-4b57-a116-abfc9f71ee4a bing\n"
    );

    std::fs::write(env.data().join("pending.data"), "oops\n").unwrap();
    let output = env.task(&["import-v2"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers()
            + &format!(
                "Record not recognized as format 4. in {} at line 1\n",
                env.data().join("pending.data").display()
            )
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn undo_reverts_one_invocation_at_a_time() {
    let env = Env::new("confirmation=off\nverbose=nothing\n");
    quiet(&env, &["add", "one"]);
    quiet(&env, &["add", "two"]);
    quiet(&env, &["1-2", "modify", "+tagged", "priority:H"]);

    // Declining keeps the change.
    let output = task_input(&env, &["rc.confirmation=on", "undo"], "no\n");
    let out = stdout(&output);
    assert!(
        out.starts_with("The following 8 operations would be reverted:\nUuid "),
        "{out}"
    );
    assert!(out.contains("Add tag 'tagged'\n"), "{out}");
    assert!(
        out.contains("Add property 'priority' with value 'H'\n"),
        "{out}"
    );
    assert!(
        out.ends_with(
            "\nThe undo command is not reversible.  Are you sure you want to revert to the previous state? (yes/no) "
        ),
        "{out}"
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(export(&env, &["+tagged"]).len(), 2);

    // One undo reverts the whole bulk modification, and nothing else.
    let output = env.task(&["undo"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), "");
    assert_eq!(export(&env, &["+tagged"]).len(), 0);
    assert_eq!(export(&env, &[]).len(), 2);

    // Then the adds, one per invocation.
    let output = env.task(&["undo"]);
    assert!(stdout(&output).contains(" Create task\n"));
    assert_eq!(export(&env, &[]).len(), 1);
    env.task(&["undo"]);
    assert_eq!(export(&env, &[]).len(), 0);

    let output = env.task(&["undo"]);
    assert_eq!(
        stdout(&output),
        "No operations to undo.\nCould not undo: other operations have occurred."
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn undo_reverts_an_import_and_recurrence_generation_as_one() {
    let env = Env::new("confirmation=off\nverbose=nothing\n");
    task_input(
        &env,
        &["import"],
        r#"[{"description":"a"},{"description":"b"}]"#,
    );
    assert_eq!(export(&env, &[]).len(), 2);
    env.task(&["undo"]);
    assert_eq!(export(&env, &[]).len(), 0);

    quiet(&env, &["add", "rec", "due:tomorrow", "recur:daily"]);
    quiet(&env, &["list"]);
    assert_eq!(
        export(&env, &["rc.recurrence=off", "status:pending"]).len(),
        1
    );
    env.task(&["rc.recurrence=off", "undo"]);
    let tasks = export(&env, &["rc.recurrence=off"]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["status"], "recurring");
    assert!(tasks[0].get("mask").is_none(), "{:?}", tasks[0]);
}

/// `task <args>` with `rc.verbose=sync`, as (stdout, stderr, exit code).
fn sync_task(env: &Env, args: &[&str]) -> (String, String, i32) {
    let mut all = vec!["rc.verbose=sync"];
    all.extend_from_slice(args);
    let output = env.task(&all);
    (
        stdout(&output),
        stderr(&output),
        output.status.code().unwrap(),
    )
}

#[test]
fn sync_local_server_dir_between_two_data_dirs() {
    let server = tempfile::tempdir().unwrap();
    let dir = server.path().display().to_string();
    let rc = format!("sync.local.server_dir={dir}\nconfirmation=off\n");
    let first = Env::new(&rc);
    let second = Env::new(&rc);

    quiet(&first, &["add", "one"]);
    let (out, err, code) = sync_task(&first, &["synchronize"]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert!(
        out.starts_with(&format!("Syncing with {dir}\nSuccess!\nSent ")),
        "{out}"
    );
    assert!(out.ends_with(" local operations to the server\n"), "{out}");

    // Nothing more to send: no "Sent" line. `[initialize]` is ignored.
    assert_eq!(
        sync_task(&second, &["sync", "initialize"]),
        (format!("Syncing with {dir}\nSuccess!\n"), String::new(), 0)
    );
    let tasks = export(&second, &[]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["description"], "one");

    // Changes flow back.
    quiet(&second, &["1", "done"]);
    sync_task(&second, &["sync"]);
    assert_eq!(
        sync_task(&first, &["sync"]),
        (format!("Syncing with {dir}\nSuccess!\n"), String::new(), 0)
    );
    assert_eq!(export(&first, &["status:completed"]).len(), 1);

    // Without the `sync` verbosity token, sync is silent.
    assert_eq!(quiet(&first, &["sync"]), (String::new(), String::new(), 0));
}

#[test]
fn sync_deprecated_origin_and_purge_on_sync() {
    let server = tempfile::tempdir().unwrap();
    let dir = server.path().display().to_string();
    let env = Env::new(&format!(
        "sync.local.server_dir={dir}\nsync.server.origin=https://u:p@h\n"
    ));
    assert_eq!(
        sync_task(&env, &["sync"]),
        (
            format!(
                "sync.server.origin is deprecated. Use sync.server.url instead.\nSyncing with {dir}\nSuccess!\n"
            ),
            String::new(),
            0
        )
    );
    assert_eq!(
        quiet(&env, &["sync"]),
        (
            "sync.server.origin is deprecated. Use sync.server.url instead.\n".into(),
            String::new(),
            0
        )
    );

    // purge.on-sync expires deleted tasks older than 6 months.
    let old = r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000009","description":"old","status":"deleted","entry":"20000101T000000Z","end":"20000101T000000Z","modified":"20000101T000000Z"}"#;
    env.write("old.json", old);
    let path = env.path("old.json").display().to_string();
    quiet(&env, &["import", &path]);
    assert_eq!(export(&env, &["status:deleted"]).len(), 1);
    quiet(&env, &["rc.purge.on-sync=1", "sync"]);
    assert_eq!(export(&env, &["status:deleted"]).len(), 0);
}

#[test]
fn sync_git_backend() {
    let has_git = Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_git {
        eprintln!("git not available; skipping");
        return;
    }
    let repo = tempfile::tempdir().unwrap();
    let path = repo.path().display().to_string();
    let rc = format!(
        "sync.git.local_path={path}\nsync.git.branch=main\nsync.encryption_secret=s3cret\n"
    );
    let first = Env::new(&rc);
    let second = Env::new(&rc);
    quiet(&first, &["add", "via git"]);
    let (out, err, code) = sync_task(&first, &["sync"]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert!(
        out.starts_with(&format!(
            "Syncing with local git repository at {path}\nSuccess!\nSent "
        )),
        "{out}"
    );
    assert_eq!(
        sync_task(&second, &["sync"]),
        (
            format!("Syncing with local git repository at {path}\nSuccess!\n"),
            String::new(),
            0
        )
    );
    assert_eq!(export(&second, &[])[0]["description"], "via git");

    // With a remote configured, the message names the remote instead
    // (`local_only` keeps the sync away from the network).
    assert_eq!(
        sync_task(
            &second,
            &[
                "rc.sync.git.remote=nowhere",
                "rc.sync.git.local_only=1",
                "sync"
            ]
        ),
        (
            "Syncing with git remote nowhere\nSuccess!\n".into(),
            String::new(),
            0
        )
    );
}

#[test]
fn sync_configuration_errors() {
    let env = Env::new("");
    let cases: &[(&[&str], &str)] = &[
        (&[], "No sync.* settings are configured. See task-sync(5)."),
        (&["rc.sync.aws.bucket=b"], "sync.aws.region is required"),
        (
            &["rc.sync.aws.bucket=b", "rc.sync.aws.region=r"],
            "sync.encryption_secret is required",
        ),
        (
            &[
                "rc.sync.aws.bucket=b",
                "rc.sync.aws.region=r",
                "rc.sync.encryption_secret=e",
            ],
            "exactly one method of specifying AWS credentials is required",
        ),
        (
            &["rc.sync.gcp.bucket=b"],
            "sync.encryption_secret is required",
        ),
        (
            &["rc.sync.server.url=http://localhost:1"],
            "sync.server.client_id and sync.encryption_secret are required",
        ),
        (
            &["rc.sync.server.origin=http://localhost:1"],
            "sync.server.client_id and sync.encryption_secret are required",
        ),
        (&["rc.sync.git.local_path=x"], "sync.git.branch is required"),
        (
            &["rc.sync.git.local_path=x", "rc.sync.git.branch=main"],
            "sync.encryption_secret is required",
        ),
    ];
    for (overrides, message) in cases {
        let mut args = vec!["rc.verbose=nothing"];
        args.extend_from_slice(overrides);
        args.push("synchronize");
        let output = env.task(&args);
        assert_eq!(stdout(&output), "", "{overrides:?}");
        assert_eq!(stderr(&output), format!("{message}\n"), "{overrides:?}");
        assert_eq!(output.status.code(), Some(2), "{overrides:?}");
    }
}

#[test]
fn purge_confirmation_dependencies_and_feedback() {
    let env = Env::new("confirmation=on\nverbose=affected,footnote\n");
    quiet(&env, &["rc.confirmation=off", "add", "one"]);
    quiet(&env, &["rc.confirmation=off", "add", "two", "depends:1"]);

    // Only deleted tasks are purgeable.
    let output = task_input(&env, &["1", "purge"], "");
    assert_eq!(stdout(&output), "Purged 0 tasks.\n");
    assert_eq!(
        stderr(&output),
        "No deleted tasks specified. Maybe you forgot to delete tasks first?\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let uuid = export(&env, &["1"])[0]["uuid"]
        .as_str()
        .unwrap()
        .to_string();
    quiet(&env, &["rc.confirmation=off", "1", "delete"]);

    // Declining keeps the task, exit code unchanged.
    let output = task_input(&env, &[&uuid, "purge"], "no\n");
    assert_eq!(
        stdout(&output),
        format!(
            "Permanently remove task {} 'one'? (yes/no) Purged 0 tasks.\n",
            &uuid[..8]
        )
    );
    assert_eq!(output.status.code(), Some(0));

    let output = task_input(&env, &[&uuid, "purge"], "yes\n");
    assert_eq!(
        stdout(&output),
        format!(
            "Permanently remove task {} 'one'? (yes/no) Purged 1 task.\n",
            &uuid[..8]
        )
    );
    assert_eq!(output.status.code(), Some(0));
    let tasks = export(&env, &[]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["description"], "two");
    assert!(tasks[0].get("depends").is_none(), "{:?}", tasks[0]);

    // Nothing matched.
    let output = task_input(&env, &[&uuid, "purge"], "");
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "No tasks specified.\n");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn purge_recurrence_template_children() {
    let env = Env::new("confirmation=off\nverbose=affected\n");
    quiet(&env, &["add", "rec", "due:tomorrow", "recur:daily"]);
    quiet(&env, &["list"]);
    let parent = export(&env, &["status:recurring"])[0]["uuid"]
        .as_str()
        .unwrap()
        .to_string();
    quiet(&env, &["rc.recurrence.confirmation=no", &parent, "delete"]);

    // A pending child blocks the purge.
    let output = env.task(&[&parent, "purge"]);
    assert!(
        stderr(&output).starts_with("Task 'rec' is a recurrence template. Its child task "),
        "{}",
        stderr(&output)
    );
    assert_ne!(output.status.code(), Some(0));

    quiet(
        &env,
        &[
            "rc.recurrence=off",
            "rc.recurrence.confirmation=no",
            "status:pending",
            "delete",
        ],
    );
    let output = task_input(
        &env,
        &["rc.recurrence.confirmation=prompt", &parent, "purge"],
        "no\n",
    );
    assert!(stdout(&output).ends_with(
        "Task 'rec' is a recurrence template. All its 1 deleted children tasks will be purged as well. Continue? (yes/no) "
    ), "{}", stdout(&output));
    assert_eq!(stderr(&output), "Purge operation aborted.\n");

    let output = env.task(&["rc.recurrence.confirmation=yes", &parent, "purge"]);
    assert_eq!(stdout(&output), "Purged 2 tasks.\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(export(&env, &["rc.recurrence=off"]).len(), 0);
}

/// `calendar <MM> <YYYY>`: the month grid (C++ `CmdCalendar::renderMonths`),
/// week numbers (`%U` with weekstart Sunday), `weekstart=monday`,
/// `displayweeknumber=off` and the month-number / month-name arguments.
#[test]
fn calendar_month_grid_layout() {
    let env = Env::new("defaultwidth=80\ndetection=off\nfontunderline=off\n");
    let (out, err, rc) = quiet(&env, &["calendar", "2", "2021", "rc.calendar.monthsperline:1"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        out,
        "\n       February 2021      \n\
         \x20    Su Mo Tu We Th Fr Sa\n\
         ---- -- -- -- -- -- -- --\n\
         \x20  5     1  2  3  4  5  6\n\
         \x20  6  7  8  9 10 11 12 13\n\
         \x20  7 14 15 16 17 18 19 20\n\
         \x20  8 21 22 23 24 25 26 27\n\
         \x20  9 28\n\n"
    );
    let (out, err, rc) = quiet(
        &env,
        &[
            "calendar",
            "feb",
            "2021",
            "rc.monthsperline:1",
            "rc.weekstart:monday",
            "rc.displayweeknumber:off",
        ],
    );
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        out,
        "\n       February 2021      \n\
         \x20Mo Tu We Th Fr Sa Su\n\
         \x20-- -- -- -- -- -- --\n\
         \x20 1  2  3  4  5  6  7\n\
         \x20 8  9 10 11 12 13 14\n\
         \x2015 16 17 18 19 20 21\n\
         \x2022 23 24 25 26 27 28\n\n"
    );
    // Three months per line at width 80, headers centred in 26 columns.
    let (out, _, rc) = quiet(&env, &["calendar", "12", "2021"]);
    assert_eq!(rc, 0);
    assert!(
        out.starts_with(
            "\n       December 2021             January 2022              February 2022      \n"
        ),
        "{out:?}"
    );
}

/// `calendar y`, `calendar YYYY`, `calendar due` and `calendar.offset`.
#[test]
fn calendar_range_arguments() {
    let env = Env::new("defaultwidth=80\ndetection=off\ncalendar.monthsperline=2\n");
    let headers = |out: &str| -> Vec<String> {
        out.lines()
            .filter(|l| l.contains(" 20") && l.trim_start().starts_with(char::is_alphabetic))
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    };
    let (out, err, rc) = quiet(&env, &["calendar", "2021"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        headers(&out),
        [
            "January 2021 February 2021",
            "March 2021 April 2021",
            "May 2021 June 2021",
            "July 2021 August 2021",
            "September 2021 October 2021",
            "November 2021 December 2021"
        ]
    );
    let (out, _, rc) = quiet(&env, &["calendar", "11", "2021", "y"]);
    assert_eq!(rc, 0);
    assert_eq!(headers(&out).len(), 6);
    assert_eq!(headers(&out)[0], "November 2021 December 2021");
    assert_eq!(headers(&out)[5], "September 2022 October 2022");

    // `due`: the oldest pending due date not tagged nocal.
    quiet(&env, &["add", "a", "due:2021-05-03"]);
    quiet(&env, &["add", "b", "due:2020-01-03", "+nocal"]);
    let (out, _, rc) = quiet(&env, &["calendar", "due"]);
    assert_eq!(rc, 0);
    assert_eq!(headers(&out), ["May 2021 June 2021"]);

    // A negative offset wraps into the previous year.
    let (out, _, rc) = quiet(
        &env,
        &[
            "calendar",
            "2",
            "2021",
            "rc.calendar.offset:on",
            "rc.calendar.offset.value:-3",
        ],
    );
    assert_eq!(rc, 0);
    assert_eq!(headers(&out), ["November 2020 December 2020"]);
}

/// The C++ argument errors (exit status 2 through `Context::run`).
#[test]
fn calendar_argument_errors() {
    let env = Env::new("");
    let (out, err, rc) = quiet(&env, &["calendar", "donkey"]);
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "Could not recognize argument 'donkey'.\n", 2)
    );
    let (out, err, rc) = quiet(&env, &["calendar", "13"]);
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "Argument '13' is not a valid month.\n", 2)
    );
    let (_, err, rc) = quiet(
        &env,
        &["calendar", "rc.calendar.details:full", "rc.calendar.details.report:nope"],
    );
    assert_eq!(
        (err.as_str(), rc),
        (
            "The setting 'calendar.details.report' must contain a single report name.\n",
            2
        )
    );
}

/// `calendar.holidays=full` with holidays from an included
/// `doc/rc/holidays.*.rc` file (resolved through the rc include path), and
/// a multi-day `holiday.X.start`/`.end` entry.
#[test]
fn calendar_holidays_table_from_include() {
    let env = Env::new(
        "defaultwidth=80\ndetection=off\nfontunderline=off\ninclude holidays.en-US.rc\n\
         holiday.zz.name=Donkey week\nholiday.zz.start=20210208\nholiday.zz.end=20210212\n",
    );
    let (out, err, rc) = quiet(
        &env,
        &[
            "calendar",
            "1",
            "2021",
            "rc.monthsperline:2",
            "rc.calendar.holidays:full",
        ],
    );
    assert_eq!((err.as_str(), rc), ("", 0));
    assert!(
        out.ends_with(
            "\nDate       Holiday\n\
             ---------- -----------------------------------\n\
             2021-01-01 New Year's Day\n\
             2021-01-18 Birthday of Martin Luther King, Jr.\n\
             2021-02-08 Start of Donkey week\n\
             2021-02-12 End of Donkey week\n\
             2021-02-15 Washington's Birthday\n\n"
        ),
        "{out:?}"
    );
}

/// `calendar.details=full` re-runs the details report as a child `task`
/// process (C++ `::execute`) for the displayed range, excluding `+nocal`.
#[test]
fn calendar_details_report() {
    let env = Env::new("defaultwidth=80\ndetection=off\nfontunderline=off\n");
    quiet(&env, &["add", "shown", "due:2021-05-03"]);
    quiet(&env, &["add", "hidden", "due:2021-05-04", "+nocal"]);
    quiet(&env, &["add", "later", "due:2021-07-04"]);
    let (out, err, rc) = quiet(
        &env,
        &[
            "calendar",
            "5",
            "2021",
            "rc.monthsperline:1",
            "rc.calendar.details:full",
            "rc.calendar.details.report:list",
        ],
    );
    assert_eq!((err.as_str(), rc), ("", 0));
    assert!(out.contains("2021-05-03 shown"), "{out:?}");
    assert!(!out.contains("hidden"), "{out:?}");
    assert!(!out.contains("later"), "{out:?}");
    assert!(out.ends_with("\n1 task\n"), "{out:?}");
}

/// Colour: the legend, weekend/holiday/due cell colours and the
/// `color.calendar.*` settings.
#[test]
fn calendar_colors_and_legend() {
    let env = Env::new(
        "defaultwidth=80\ndetection=off\nfontunderline=off\ncolor.alternate=\n\
         color.calendar.weekend=red\ncolor.calendar.holiday=green\ncolor.calendar.due=blue\n\
         color.calendar.weeknumber=\ncolor.label=\n\
         calendar.holidays=sparse\nholiday.x.name=X\nholiday.x.date=20210203\n",
    );
    quiet(&env, &["add", "d", "due:2099-02-04"]);
    let (out, err, rc) = quiet(
        &env,
        &["calendar", "2", "2099", "rc.monthsperline:1", "rc._forcecolor:on"],
    );
    assert_eq!((err.as_str(), rc), ("", 0));
    // Sunday 1 Feb 2099 is a weekend, 4 Feb is due (rc.due is forced to 0).
    assert!(out.contains("\x1b[31m 1\x1b[0m"), "{out:?}");
    assert!(out.contains("\x1b[34m 4\x1b[0m"), "{out:?}");
    assert!(out.ends_with(
        "Legend: \x1b[38;5;15;48;5;25mtoday\x1b[0m, \x1b[31mweekend\x1b[0m, \
         \x1b[34mdue\x1b[0m, \x1b[38;5;15;48;5;1mdue-today\x1b[0m, \
         \x1b[38;5;0;48;5;9moverdue\x1b[0m, \x1b[38;5;25;48;5;15mscheduled\x1b[0m, \
         \x1b[32mholiday\x1b[0m, weeknumber.\n"
    ), "{out:?}");
    // A holiday date in 2021 is coloured green.
    let (out, _, _) = quiet(
        &env,
        &["calendar", "2", "2021", "rc.monthsperline:1", "rc._forcecolor:on"],
    );
    assert!(out.contains("\x1b[32m 3\x1b[0m"), "{out:?}");
    // No legend without colour or with calendar.legend=off.
    let (out, _, _) = quiet(
        &env,
        &["calendar", "2", "2099", "rc._forcecolor:on", "rc.calendar.legend:off"],
    );
    assert!(!out.contains("Legend"), "{out:?}");
}

////////////////////////////////////////////////////////////////////////////////
// burndown.daily / weekly / monthly / annual

/// The burndown scenario at a frozen 2021-06-15 12:00:00 UTC: pending
/// `one` (entry 06-01), started `two` (06-03), `three` (06-05, done today),
/// logged `four` (06-02 to 06-08) and pending `five` (06-04). Returns `None`
/// when `faketime` is not installed.
fn burndown_env() -> Option<Env> {
    let env = Env::new("defaultwidth=80\ndefaultheight=24\ndetection=off\nconfirmation=off\n");
    let at = "2021-06-15 12:00:00";
    for args in [
        &["add", "one", "entry:2021-06-01"][..],
        &["add", "two", "entry:2021-06-03"],
        &["2", "start"],
        &["add", "three", "entry:2021-06-05"],
        &["3", "done"],
        &["log", "four", "entry:2021-06-02", "end:2021-06-08"],
        &["add", "five", "entry:2021-06-04"],
    ] {
        let mut all = vec!["rc.verbose=nothing"];
        all.extend_from_slice(args);
        let output = faketime_task(&env, at, &all)?;
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    }
    Some(env)
}

/// `faketime` `task rc.verbose=nothing <args>` at the burndown scenario time.
fn burndown(env: &Env, args: &[&str]) -> (String, String, i32) {
    let mut all = vec!["rc.verbose=nothing"];
    all.extend_from_slice(args);
    let output = faketime_task(env, "2021-06-15 12:00:00", &all).unwrap();
    (
        stdout(&output),
        stderr(&output),
        output.status.code().unwrap(),
    )
}

/// `burndown.daily`: 21 daily bars, the y axis rounded by `burndown_size`,
/// the legend, the net fix rate (peak of 5 on 06-05, 3 still open) and the
/// estimated completion. Each major x-axis label (`May`, `Jun`) replaces
/// one byte fewer than it writes (C++ `std::string::replace`), so the rate
/// lines start two columns left of the bars.
#[test]
fn burndown_daily_chart() {
    let Some(env) = burndown_env() else {
        return;
    };
    let (out, err, rc) = burndown(&env, &["burndown.daily"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(out, &DAILY[1..]);
    const DAILY: &str = r#"
                                 Daily Burndown
6 |
  |
  |
  |                                X  X  X  .  .  .  .  .  .  .  .
  |                                X  X  X  .  .  .  .  .  .  .  .
  |                             X  X  X  X  X  X  X  X  X  X  X  .     . Done
  |                             X  X  X  X  X  X  X  X  X  X  X  .     + Started
3 |                             X  X  X  X  X  X  X  X  X  X  X  .     X Pending
  |                          X  X  X  X  X  X  X  X  X  X  X  X  +
  |                          X  X  X  X  X  X  X  X  X  X  X  X  +
  |                       X  X  X  X  X  X  X  X  X  X  X  X  X  X
  |                       X  X  X  X  X  X  X  X  X  X  X  X  X  X
  |                       X  X  X  X  X  X  X  X  X  X  X  X  X  X
  |                    X  X  X  X  X  X  X  X  X  X  X  X  X  X  X
  |                    X  X  X  X  X  X  X  X  X  X  X  X  X  X  X
0 +-----------------------------------------------------------------
    26 27 28 29 30 31 01 02 03 04 05 06 07 08 09 10 11 12 13 14 15
    May               Jun

  Net Fix Rate:         0.2/d
  Estimated completion: 2021-07-01 (2w)
"#;

    // Colour replaces the markers by colorized blanks.
    let (out, _, rc) = burndown(
        &env,
        &["burndown.daily", "rc._forcecolor=on", "rc.color.burndown.pending=on red"],
    );
    assert_eq!(rc, 0);
    assert!(!out.contains(" X"), "{out}");
    assert!(out.contains("  |                   \x1b[41m  \x1b[0m \x1b[41m  \x1b[0m"), "{out:?}");
}

/// `burndown.monthly` (cumulative) and `burndown.annual` with
/// `burndown.cumulative=0`: no major labels for annual, so no shift.
#[test]
fn burndown_monthly_and_annual() {
    let Some(env) = burndown_env() else {
        return;
    };
    let (out, err, rc) = burndown(&env, &["burndown.monthly"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(out, &MONTHLY[1..]);
    const MONTHLY: &str = r#"
                                Monthly Burndown
6 |
  |
  |
  |                                                              .
  |                                                              .
  |                                                              .     . Done
  |                                                              .     + Started
3 |                                                              .     X Pending
  |                                                              +
  |                                                              +
  |                                                              X
  |                                                              X
  |                                                              X
  |                                                              X
  |                                                              X
0 +-----------------------------------------------------------------
    10 11 12 01 02 03 04 05 06 07 08 09 10 11 12 01 02 03 04 05 06
    2019     2020                                2021

 Net Fix Rate:         0.2/d
 Estimated completion: 2021-07-01 (2w)
"#;

    let (out, err, rc) = burndown(&env, &["burndown.annual", "rc.burndown.cumulative=0"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(out, &ANNUAL[1..]);
    const ANNUAL: &str = r#"
                                Annual Burndown
6 |
  |
  |
  |                                                              .
  |                                                              .
  |                                                              .     . Done
  |                                                              .     + Started
3 |                                                              .     X Pending
  |                                                              +
  |                                                              +
  |                                                              X
  |                                                              X
  |                                                              X
  |                                                              X
  |                                                              X
0 +-----------------------------------------------------------------
    01 02 03 04 05 06 07 08 09 10 11 12 13 14 15 16 17 18 19 20 21


    Net Fix Rate:         0.2/d
    Estimated completion: 2021-07-01 (2w)
"#;
}

/// A filter selects through `Filter::subset`: one pending task, no
/// convergence; no match gives the "No matches." footnote; bad weekstart,
/// small terminals and bad colours fail like C++.
#[test]
fn burndown_filter_and_errors() {
    let Some(env) = burndown_env() else {
        return;
    };
    let (out, err, rc) = burndown(&env, &["description:one", "burndown.weekly"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(out, &WEEKLY[1..]);
    const WEEKLY: &str = r#"
                                Weekly Burndown
2 |
  |
  |
  |
  |
  |                                                                    . Done
  |                                                                    + Started
1 |                                                                    X Pending
  |                                                        X  X  X
  |                                                        X  X  X
  |                                                        X  X  X
  |                                                        X  X  X
  |                                                        X  X  X
  |                                                        X  X  X
  |                                                        X  X  X
0 +-----------------------------------------------------------------
    04 05 06 07 08 09 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24
    2021

   Net Fix Rate:         -
   Estimated completion: No convergence
"#;

    let (out, err, rc) = burndown(
        &env,
        &["description:zzz", "burndown.daily", "rc.verbose=footnote"],
    );
    assert_eq!((err.as_str(), rc), ("No matches.\n", 0));
    assert!(out.ends_with("\n  Net Fix Rate:         -\n\n"), "{out:?}");
    assert!(!out.contains("Estimated completion"), "{out}");

    let (out, err, rc) = burndown(&env, &["burndown.daily", "rc.defaultheight=11"]);
    assert_eq!((out.as_str(), err.as_str(), rc), ("Terminal window too small to draw a graph.\n", "", 0));
    let (out, _, rc) = burndown(&env, &["burndown.daily", "rc.defaultheight=1100"]);
    assert_eq!((out.as_str(), rc), ("Terminal window too large to draw a graph.\n", 0));

    let (out, err, rc) = burndown(&env, &["burndown.weekly", "rc.weekstart=tuesday"]);
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "The 'weekstart' configuration variable may only contain 'Sunday' or 'Monday'.\n", 2)
    );
    let (out, err, rc) = burndown(&env, &["burndown.daily", "rc.color.burndown.done=nocolour"]);
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "The color 'nocolour' is not recognized.\n", 2)
    );
}

////////////////////////////////////////////////////////////////////////////////
// history.daily / weekly / monthly / annual, ghistory.daily / weekly / monthly / annual

/// Pending `PL` (entry 2015-01-02), deleted `DLN` and completed `CLN`
/// (entry 01-02, end 02-02), completed `CNN` (entry and end 02-02).
const HISTORY_DATA: &str = r#"[
{"uuid":"00000000-0000-0000-0000-000000000001","description":"PL","status":"pending","entry":"20150102T120000Z"},
{"uuid":"00000000-0000-0000-0000-000000000002","description":"DLN","status":"deleted","entry":"20150102T120000Z","end":"20150202T120000Z"},
{"uuid":"00000000-0000-0000-0000-000000000005","description":"CLN","status":"completed","entry":"20150102T120000Z","end":"20150202T120000Z"},
{"uuid":"00000000-0000-0000-0000-000000000007","description":"CNN","status":"completed","entry":"20150202T120000Z","end":"20150202T120000Z"}
]"#;

fn history_env() -> Env {
    let env = Env::new("defaultwidth=80\ndetection=off\nconfirmation=off\n");
    let output = task_input(&env, &["rc.verbose=nothing", "import", "-"], HISTORY_DATA);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    env
}

/// `TZ=UTC task <args>`, returning (stdout, stderr, status).
fn history(env: &Env, args: &[&str]) -> (String, String, i32) {
    let output = env
        .command(args)
        .env("TASKRC", env.rc())
        .env("TASKDATA", env.data())
        .env("TZ", "UTC")
        .output()
        .unwrap();
    (
        stdout(&output),
        stderr(&output),
        output.status.code().unwrap(),
    )
}

/// The tabular reports: one row per interval, a blank row, the integer
/// averages (`total / rows`) and the Net column. `history.monthly` prints
/// the year only when it changes (from 1970 for the first row).
#[test]
fn history_tabular_reports() {
    let env = history_env();
    let (out, err, rc) = history(&env, &["rc.verbose=nothing", "history.monthly"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        out,
        "\x1b[4mYear\x1b[0m \x1b[4mMonth   \x1b[0m \x1b[4mAdded\x1b[0m \x1b[4mCompleted\x1b[0m \x1b[4mDeleted\x1b[0m \x1b[4mNet\x1b[0m\n\
         2015 January      3         0       0   3\n     February     1         2       1  -2\n\n     Average      2         1       0   0\n\n"
    );

    let (out, _, rc) = history(&env, &["rc.verbose=nothing", "history.annual"]);
    assert_eq!(rc, 0);
    assert_eq!(
        out,
        "\x1b[4mYear   \x1b[0m \x1b[4mAdded\x1b[0m \x1b[4mCompleted\x1b[0m \x1b[4mDeleted\x1b[0m \x1b[4mNet\x1b[0m\n\
         2015        4         2       1   1\n\nAverage     4         2       1   1\n\n"
    );

    // Weeks start on Sunday (2014-12-28); the Day column holds "Average".
    let (out, _, rc) = history(&env, &["rc.verbose=nothing", "history.weekly"]);
    assert_eq!(rc, 0);
    assert!(
        out.contains("\n2014 December      28     3         0       0   3\n2015 February       1     1         2       1  -2\n\n              Average     2         1       0   0\n"),
        "{out:?}"
    );

    let (out, _, rc) = history(&env, &["rc.verbose=nothing", "history.daily"]);
    assert_eq!(rc, 0);
    assert!(
        out.contains("\n2015 January        2     3         0       0   3\n     February       2     1         2       1  -2\n"),
        "{out:?}"
    );

    // With colour: alternate rows, a red/green Net and a bold Average row.
    let (out, _, rc) = history(
        &env,
        &["rc.verbose=nothing", "rc._forcecolor:on", "rc.color.alternate:", "history.monthly"],
    );
    assert_eq!(rc, 0);
    assert!(out.contains("0 \x1b[31m  3\x1b[0m\n"), "{out:?}");
    assert!(out.contains("1 \x1b[32m -2\x1b[0m\n"), "{out:?}");
    assert!(out.contains("\n     \x1b[1mAverage \x1b[0m \x1b[1m    2\x1b[0m"), "{out:?}");
}

/// The graphical reports: bars scaled by `(widthOfBar * n) / maxLine`, with
/// `widthOfBar = width - labelWidth` and the added bars right-aligned at
/// `leftOffset`.
#[test]
fn ghistory_bars_and_legend() {
    let env = history_env();
    // 80 - 15 = 65; maxLine 3 + 3; leftOffset 32; February 10/21/10.
    let (out, err, rc) = history(&env, &["rc.verbose=nothing", "ghistory.monthly"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        out,
        format!(
            "\x1b[4mYear\x1b[0m \x1b[4mMonth   \x1b[0m \x1b[4mNumber Added/Completed/Deleted{}\x1b[0m\n\
             2015 January  {}\n     February {}{}{}{}\n\nLegend: + Added, X Completed, - Deleted\n",
            " ".repeat(33),
            "+".repeat(32),
            " ".repeat(22),
            "+".repeat(10),
            "X".repeat(21),
            "-".repeat(10)
        )
    );

    // 80 - 19 = 61: 30, then 10/20/10 after a 20-column offset.
    let (out, _, rc) = history(&env, &["rc.verbose=nothing", "ghistory.weekly"]);
    assert_eq!(rc, 0);
    assert!(
        out.contains(&format!(
            "\n2014 December  28 {}\n2015 February   1 {}{}{}{}\n",
            "+".repeat(30),
            " ".repeat(20),
            "+".repeat(10),
            "X".repeat(20),
            "-".repeat(10)
        )),
        "{out:?}"
    );

    let (out, _, rc) = history(&env, &["rc.verbose=nothing", "ghistory.daily"]);
    assert_eq!(rc, 0);
    assert!(out.contains("\n2015 January    2 +"), "{out:?}");
    assert!(out.contains("\n     February   2 "), "{out:?}");

    // 80 - 5 = 75; maxLine 4 + 3: 42/21/10.
    let (out, _, rc) = history(&env, &["rc.verbose=nothing", "ghistory.annual"]);
    assert_eq!(rc, 0);
    assert!(
        out.contains(&format!(
            "\n2015 {}{}{}\n\nLegend: + Added, X Completed, - Deleted\n",
            "+".repeat(42),
            "X".repeat(21),
            "-".repeat(10)
        )),
        "{out:?}"
    );

    // With colour the counts are right-aligned in coloured bars, and the
    // legend is coloured and followed by the optional blank line.
    let (out, _, rc) = history(
        &env,
        &["rc.verbose=blank", "rc._forcecolor:on", "rc.color.alternate:", "ghistory.annual"],
    );
    assert_eq!(rc, 0);
    let add = "\x1b[38;5;0;48;5;196m";
    let done = "\x1b[38;5;0;48;5;46m";
    let del = "\x1b[38;5;0;48;5;226m";
    assert!(
        out.contains(&format!(
            "\n2015 {add}{}4\x1b[0m{done}{}2\x1b[0m{del}{}1\x1b[0m\n",
            " ".repeat(41),
            " ".repeat(20),
            " ".repeat(9)
        )),
        "{out:?}"
    );
    assert!(
        out.ends_with(&format!(
            "\nLegend: {add}Added\x1b[0m, {done}Completed\x1b[0m, {del}Deleted\x1b[0m\n\n"
        )),
        "{out:?}"
    );
}

/// No matching task: no output, the "No tasks." footnote and status 1.
#[test]
fn history_no_tasks() {
    let env = history_env();
    for keyword in ["history.monthly", "ghistory.daily"] {
        let (out, err, rc) = history(&env, &["rc.verbose=footnote", "description:nomatch", keyword]);
        assert_eq!((out.as_str(), err.as_str(), rc), ("", "No tasks.\n", 1), "{keyword}");
    }
}

/// A coloured count wider than `leftOffset`: C++ `std::string (leftOffset -
/// aBar.length (), ' ')` wraps and throws `std::length_error`, reported by
/// `catch (...)`.
#[test]
fn ghistory_colour_bar_wider_than_offset() {
    let env = Env::new("detection=off\nconfirmation=off\n");
    let tasks: Vec<String> = (0..10)
        .map(|i| format!(r#"{{"description":"t{i}","entry":"20150102T120000Z"}}"#))
        .collect();
    let output = task_input(
        &env,
        &["rc.verbose=nothing", "import", "-"],
        &format!("[{}]", tasks.join(",")),
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let (out, err, rc) = history(
        &env,
        &["rc.verbose=nothing", "rc.defaultwidth=20", "rc._forcecolor:on", "ghistory.daily"],
    );
    assert_eq!(
        (out.as_str(), err.as_str(), rc),
        ("", "\x1b[37;41mUnknown error. Please report.\x1b[0m\n", 3)
    );
    // Without colour the bar is one '+'.
    let (out, _, rc) = history(&env, &["rc.verbose=nothing", "rc.defaultwidth=20", "ghistory.daily"]);
    assert_eq!(rc, 0);
    assert!(out.contains("\n2015 January   2 +\n"), "{out:?}");
}

////////////////////////////////////////////////////////////////////////////////
// summary / timesheet / stats

/// A frozen 2021-06-15 12:00:00 UTC scenario: pending `one` (a.b, +x),
/// started `two` (a, due 06-20), `three` (c, done today), logged `four`
/// (a.b.c, 06-02 to 06-08), pending `five` (+x +y) and deleted `six`
/// (depends on `one`). Returns `None` when `faketime` is not installed.
fn graphs_env() -> Option<Env> {
    let env = Env::new("defaultwidth=80\ndetection=off\nconfirmation=off\ndateformat=Y-M-D\n");
    for args in [
        &["add", "one", "entry:2021-06-01", "project:a.b", "+x"][..],
        &["add", "two", "entry:2021-06-03", "project:a", "due:2021-06-20"],
        &["2", "start"],
        &["add", "three", "entry:2021-06-05", "project:c"],
        &["3", "done"],
        &["log", "four", "entry:2021-06-02", "end:2021-06-08", "project:a.b.c"],
        &["add", "five", "entry:2021-06-04", "+y", "+x"],
        &["add", "six", "entry:2021-06-06", "depends:1"],
        &["description:six", "delete"],
    ] {
        let mut all = vec!["rc.verbose=nothing"];
        all.extend_from_slice(args);
        let output = faketime_task(&env, "2021-06-15 12:00:00", &all)?;
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    }
    Some(env)
}

/// `faketime` `task <args>` at the report scenario time.
fn graphs(env: &Env, args: &[&str]) -> (String, String, i32) {
    let output = faketime_task(env, "2021-06-15 12:00:00", args).unwrap();
    (
        stdout(&output),
        stderr(&output),
        output.status.code().unwrap(),
    )
}

/// `summary`: the project tree of pending projects with parents indented,
/// remaining counts, average ages, % complete and the `=` bars; with
/// `summary.all.projects` the completed-only projects too; colour bars use
/// `color.summary.bar`/`background`. As in C++ the average age divides by
/// every task of the project (`counter`), so the deleted `six` halves the
/// `(none)` age although it adds nothing to the sum.
#[test]
fn summary_project_tree() {
    let Some(env) = graphs_env() else {
        return;
    };
    let (out, err, rc) = graphs(&env, &["rc.verbose=blank,footnote", "summary"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    assert_eq!(
        out,
        "
Project Remaining Avg age Complete 0%                        100%
------- --------- ------- -------- ------------------------------
(none)          1      5d       0%
a               2     11d      33% ==========
  b             1     10d      50% ===============

3 projects
"
    );

    let (out, _, rc) = graphs(&env, &["rc.verbose=nothing", "rc.summary.all.projects=1", "summary"]);
    assert_eq!(rc, 0);
    assert!(out.contains("\n    c           0      6d     100% ==============================\n"), "{out}");
    assert!(out.contains("\nc               0     10d     100% ==============================\n"), "{out}");
    assert!(out.ends_with("\n5 projects\n"), "{out}");

    let (out, _, _) = graphs(
        &env,
        &[
            "rc.verbose=nothing",
            "rc._forcecolor=on",
            "rc.color.alternate=",
            "rc.color.summary.bar=on red",
            "rc.color.summary.background=on blue",
            "project:a.b",
            "summary",
        ],
    );
    assert!(
        out.contains("\x1b[41m               \x1b[0m\x1b[44m               \x1b[0m"),
        "{out:?}"
    );

    let (out, err, rc) = graphs(&env, &["rc.verbose=nothing", "project:none", "summary"]);
    assert_eq!((out.as_str(), err.as_str(), rc), ("No projects.\n", "", 1));
}

/// `timesheet`: the default 4-week filter, weekly blocks separated by a
/// blank row, repeated week/date/day cells blanked, and the affected line.
#[test]
fn timesheet_weekly_blocks() {
    let Some(env) = graphs_env() else {
        return;
    };
    let (out, err, rc) = graphs(&env, &["rc.verbose=blank,affected", "rc.fontunderline=0", "timesheet"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    assert_eq!(lines[0], "");
    assert_eq!(lines[1], "Wk  Date       Day ID       Action    Project Due        Task");
    assert!(lines[3].starts_with("W23 2021-06-08 Tue "), "{out}");
    assert!(lines[3].ends_with(" Completed a.b.c              four"), "{out}");
    assert_eq!(lines[4], "");
    // `two` was started and `three` completed in the same (frozen) second:
    // equal `_key`s keep the unstable C++ `std::sort` order, which follows
    // the replica's load order, so either may come first. The second row of
    // the week has blank week/date/day cells; the first one's day is blank
    // too, as 06-08 was also a Tuesday.
    let (started, completed) = if lines[5].contains("Started") { (5, 6) } else { (6, 5) };
    let lead = |i: usize| if i == 5 { "W24 2021-06-15     " } else { "                   " };
    assert!(lines[started].starts_with(lead(started)), "{out}");
    assert!(lines[started].ends_with("2        Started   a       2021-06-20 two"), "{out}");
    assert!(lines[completed].starts_with(lead(completed)), "{out}");
    assert!(lines[completed].ends_with(" Completed c                  three"), "{out}");
    assert_eq!(&lines[7..], ["", "2 completed, 1 started."]);

    // An explicit filter replaces the default one.
    let (out, _, rc) = graphs(&env, &["rc.verbose=affected", "description:four", "timesheet"]);
    assert_eq!(rc, 0);
    assert!(out.contains("four") && !out.contains("three"), "{out}");
    assert!(out.ends_with("\n1 completed, 0 started.\n"), "{out}");
}

/// `stats`: status counts, tags/projects, blocked/blocking, replica undo and
/// backlog counts, then the derived dates and averages.
#[test]
fn stats_database_statistics() {
    let Some(env) = graphs_env() else {
        return;
    };
    let (out, err, rc) = graphs(&env, &["rc.verbose=nothing", "stats"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Category                   Data");
    assert_eq!(lines[1], "-------------------------  ------------");
    let expected = [
        "Pending                    3",
        "Waiting                    0",
        "Recurring                  0",
        "Completed                  2",
        "Deleted                    1",
        "Total                      6",
        "Annotations                0",
        "Unique tags                2",
        "Projects                   4",
        "Blocked tasks              0",
        "Blocking tasks             0",
    ];
    assert_eq!(&lines[2..13], expected, "{out}");
    assert!(lines[13].starts_with("Undo transactions          "), "{out}");
    assert!(lines[14].starts_with("Sync backlog transactions  "), "{out}");
    assert_eq!(
        &lines[15..],
        [
            "Tasks tagged               33.3%",
            "Oldest task                2021-06-01",
            "Newest task                2021-06-06",
            "Task used for              5d",
            "Task added every           20h",
            "Task completed every       2d",
            "Task deleted every         5d",
            "Average time pending       11d",
            "Average desc length        3 characters",
        ],
        "{out}"
    );
}

/// The `projects`/`tags`/`udas` scenario: pending tasks in `a.b`, `a` and
/// no project, a completed `c` and a deleted `d` (in `completed_tasks`), tags
/// `foo`/`next`, the UDAs `size` and `extra` (default `x`) and an orphan
/// `gone`.
fn inventory_env() -> Env {
    let env = Env::new(
        "defaultwidth=80\nconfirmation=off\nuda.size.type=numeric\nuda.size.label=Size\n\
         uda.size.values=1,2,3\nuda.extra.type=string\nuda.extra.default=x\n",
    );
    for args in [
        &["add", "one", "project:a.b", "+foo", "size:2"][..],
        &["add", "two", "project:a", "+foo", "+next"],
        &["add", "three"],
        &["add", "four", "project:c", "+old"],
        &["4", "done"],
        &["add", "five", "project:d"],
        &["5", "delete"],
        &["add", "six", "rc.uda.gone.type=string", "gone:1"],
    ] {
        let mut all = vec!["rc.verbose=nothing"];
        all.extend_from_slice(args);
        let output = env.task(&all);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    }
    env
}

/// `projects`, `_projects` and `list.all.projects`: the indented tree with
/// counts, "(none)", the "N projects (M tasks)" line and rc 1 with
/// "No projects.".
#[test]
fn projects_and_completion_projects() {
    let env = inventory_env();
    let output = env.task(&["rc.verbose=blank", "projects"]);
    assert_eq!((stderr(&output).as_str(), output.status.code()), ("", Some(0)));
    assert_eq!(
        stdout(&output),
        "
Project Tasks
------- -----
(none)      2
a           2
  b         1

2 projects (4 tasks)
"
    );

    let output = env.task(&["rc.verbose=nothing", "rc.list.all.projects=1", "projects"]);
    assert_eq!(
        stdout(&output),
        "Project Tasks\n------- -----\n(none)      2\na           2\n  b         1\nc           1\n3 projects (5 tasks)\n"
    );

    let output = env.task(&["rc.verbose=nothing", "project:zzz", "projects"]);
    assert_eq!((stdout(&output).as_str(), output.status.code()), ("No projects.\n", Some(1)));

    let output = env.task(&["_projects"]);
    assert_eq!((stdout(&output).as_str(), output.status.code()), ("a\na.b\n", Some(0)));
    // Unlike `projects`, `_projects` does not skip the deleted task.
    let output = env.task(&["rc.list.all.projects=1", "_projects"]);
    assert_eq!(stdout(&output), "a\na.b\nc\nd\n");
}

/// `tags` (counts, bold special tags under colour, footnotes counting the
/// unfiltered tasks, "No tags." + rc 1) and `_tags` (with the built-ins).
#[test]
fn tags_and_completion_tags() {
    let env = inventory_env();
    let output = env.task(&["rc.verbose=footnote", "rc.fontunderline=0", "tags"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "Tag  Count\n---- -----\nfoo      2\nnext     1\n\n");
    assert_eq!(stderr(&output), "2 tags\n(4 tasks)\n");

    let output = env.task(&["rc.verbose=footnote", "rc.fontunderline=0", "+next", "tags"]);
    assert_eq!(stdout(&output), "Tag  Count\n---- -----\nfoo      1\nnext     1\n\n");
    assert_eq!(stderr(&output), "2 tags\n(4 tasks)\n");

    let output = env.task(&["rc.verbose=footnote", "rc.list.all.tags=1", "+old", "tags"]);
    // The count includes the completed and the deleted task.
    assert_eq!(stderr(&output), "1 tag\n(6 tasks)\n");

    let output = env.task(&["rc.verbose=footnote", "rc._forcecolor=on", "rc.color.alternate=", "tags"]);
    assert!(stdout(&output).contains("\u{1b}[1mnext\u{1b}[0m"), "{}", stdout(&output));
    assert!(!stdout(&output).contains("\u{1b}[1mfoo"), "{}", stdout(&output));

    let output = env.task(&["rc.verbose=footnote", "project:zzz", "tags"]);
    assert_eq!((stdout(&output).as_str(), output.status.code()), ("", Some(1)));
    assert_eq!(stderr(&output), "No tags.\n");

    let output = env.task(&["_tags"]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 37, "{out}");
    assert_eq!(&lines[32..], ["foo", "next", "nocal", "nocolor", "nonag"]);
    let output = env.task(&["rc.complete.all.tags=1", "_tags"]);
    assert!(stdout(&output).contains("\nold\n"));
}

/// `udas` (the definition table with usage counts over all tasks, deleted
/// included, and the orphan table) and `_udas`.
#[test]
fn udas_and_completion_udas() {
    let env = inventory_env();
    let output = env.task(&["rc.verbose=blank", "rc.fontunderline=0", "udas"]);
    assert_eq!((stderr(&output).as_str(), output.status.code()), ("", Some(0)));
    assert_eq!(
        stdout(&output),
        "
Name     Type    Label    Allowed Values Default Usage Count
-------- ------- -------- -------------- ------- -----------
extra    string  extra                   x       6
priority string  Priority H,M,L,                 0
size     numeric Size     1,2,3                  1

3 UDAs defined

Orphan UDA Usage Count
---------- -----------
gone       1

1 Orphan UDAs
"
    );

    let output = env.task(&["_udas"]);
    assert_eq!(
        (stdout(&output).as_str(), output.status.code()),
        ("extra\npriority\nsize\n", Some(0))
    );
}

////////////////////////////////////////////////////////////////////////////////
// show

#[test]
fn show_pattern_default_differences_and_unrecognized() {
    let env = Env::new("fontunderline=0\nrecurrence.limit=3\nshow.bogus=1\n");
    let output = env.task(&["show", "recurrence.limit"]);
    assert_eq!(stderr(&output), env.override_headers());
    assert_eq!(output.status.code(), Some(0));
    // Unrecognized variables are only reported when displayed.
    assert_eq!(
        stdout(&output),
        "
Config Variable  Value
---------------- -----
recurrence.limit 3
  Default value  1

Some of your .taskrc variables differ from the default values.

"
    );

    let output = env.task(&["show", "bogus"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        "
Config Variable Value
--------------- -----
show.bogus      1

Your .taskrc file contains these unrecognized variables:
  show.bogus



"
    );

    let output = env.task(&["show", "zzzzzz"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        "\nConfig Variable Value\n--------------- -----\nNo matching configuration variables.\n\n\n"
    );

    let output = env.task(&["show", "a", "b"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "You can only specify 'all' or a search string.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn show_all_and_config_errors() {
    let env = Env::new("calendar.details=bogus\n");
    let output = env.task(&["show", "all"]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(out.starts_with("\n\x1b[4mConfig Variable "), "{out}");
    assert!(out.contains("\ncalendar.details "), "{out}");
    assert!(out.contains("\nxterm.title "), "{out}");
    assert!(
        out.ends_with(
            "Configuration error: calendar.details contains an unrecognized value 'bogus'.\n"
        ),
        "{out}"
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdContext.cpp, CLI2::addContext, Context::getTaskContext

#[test]
fn context_define_set_list_show_delete() {
    let env = Env::new("confirmation=off\n");
    let ok = |args: &[&str], expected: &str| {
        let output = env.task(args);
        assert_eq!(stderr(&output), env.override_headers(), "{args:?}");
        assert_eq!(stdout(&output), expected, "{args:?}");
        assert_eq!(output.status.code(), Some(0), "{args:?}");
    };
    let fails = |args: &[&str], message: &str| {
        let output = env.task(args);
        assert_eq!(stdout(&output), "", "{args:?}");
        assert_eq!(
            stderr(&output),
            env.override_headers() + message + "\n",
            "{args:?}"
        );
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    };

    fails(&["context"], "No contexts defined.");
    fails(&["context", "list"], "No contexts defined.");
    fails(
        &["context", "define", "home"],
        "Both context name and its definition must be provided.",
    );
    fails(
        &["context", "define", "none", "+x"],
        "The name 'none' is reserved and not allowed to use as a context name.",
    );
    ok(
        &["context", "define", "home", "project:Home"],
        "Context 'home' defined (read, write). Use 'task context home' to activate.\n",
    );
    ok(
        &["context", "define", "either", "+a", "or", "+b"],
        "The filter '+a or +b' is not a valid modification string, because it contains the 'OR' operator.\n\
         As such, value for the write context cannot be set (context will not apply on task add / task log).\n\n\
         Please use 'task config context.either.write <default mods>' to set default attribute values for new tasks in this context manually.\n\n\
         Context 'either' defined (read only). Use 'task context either' to activate.\n",
    );
    let rc = rc_text(&env);
    assert!(
        rc.contains("context.home.read=project:Home\ncontext.home.write=project:Home\n"),
        "{rc}"
    );
    assert!(rc.contains("context.either.read=+a or +b\n"), "{rc}");
    assert!(!rc.contains("context.either.write"), "{rc}");

    fails(&["context", "nope"], "Context 'nope' not found.");
    ok(
        &["context", "home"],
        "Context 'home' set. Use 'task context none' to remove.\n",
    );
    // Helper commands print no override headers.
    let output = env.task(&["_context"]);
    assert_eq!(
        (stdout(&output).as_str(), stderr(&output).as_str()),
        ("either\nhome\n", "")
    );
    assert_eq!(output.status.code(), Some(0));
    ok(
        &["context", "show"],
        "Context 'home' with \n\n* read filter: 'project:Home'\n* write filter: 'project:Home'\n\nis currently applied.\n",
    );
    let table = "\n\x1b[4mName  \x1b[0m \x1b[4mType \x1b[0m \x1b[4mDefinition  \x1b[0m \x1b[4mActive\x1b[0m\n\
                 either read  +a or +b     no\n       write              no\n\
                 home   read  project:Home yes\n       write project:Home yes\n\n";
    ok(&["context", "list"], table);
    ok(
        &["context"],
        &format!("{table}Use 'task context none' to unset the current context.\n"),
    );

    ok(&["context", "none"], "Context unset.\n");
    fails(&["context", "none"], "Context not unset.");
    ok(&["context", "show"], "No context is currently applied.\n");

    ok(
        &["context", "home"],
        "Context 'home' set. Use 'task context none' to remove.\n",
    );
    ok(&["context", "delete", "home"], "Context 'home' deleted.\n");
    let rc = rc_text(&env);
    assert!(
        !rc.contains("context.home") && !rc.contains("context=home"),
        "{rc}"
    );
    fails(&["context", "delete", "home"], "Context 'home' not found.");
    fails(
        &["context", "delete"],
        "Context name needs to be specified.",
    );
    fails(
        &["_context", "x"],
        "The '_context' command does not allow 'x'.",
    );
}

#[test]
fn context_confirmation_prompts() {
    let env = Env::new("confirmation=on\n");
    // No pending tasks: the filter matches nothing, so that is confirmed too.
    let output = config_task(&env, &["context", "define", "h", "+x"], "no\n");
    assert_eq!(
        stdout(&output),
        "The filter '+x' matches 0 pending tasks. Do you wish to continue? (yes/no) "
    );
    let header = format!("TASKRC override: {}\n", env.rc().display());
    assert_eq!(
        stderr(&output),
        format!("{header}Context definition aborted.\n")
    );
    assert_eq!(output.status.code(), Some(2));

    let output = config_task(&env, &["context", "define", "h", "+x"], "yes\nyes\nno\n");
    assert_eq!(
        stdout(&output),
        "The filter '+x' matches 0 pending tasks. Do you wish to continue? (yes/no) \
         Are you sure you want to add 'context.h.read' with a value of '+x'? (yes/no) \
         Are you sure you want to add 'context.h.write' with a value of '+x'? (yes/no) \
         Context 'h' defined (read only). Use 'task context h' to activate.\n"
    );
    assert_eq!(output.status.code(), Some(0));

    let output = config_task(&env, &["context", "delete", "h"], "no\n");
    assert_eq!(
        stdout(&output),
        "Do you want to delete context 'h'? (yes/no) "
    );
    assert_eq!(
        stderr(&output),
        format!("{header}Context 'h' not deleted.\n")
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(rc_text(&env).contains("context.h.read=+x"));
}

#[test]
fn context_filter_and_write_context_application() {
    let env = Env::new("confirmation=off\nverbose=nothing\n");
    let quiet = |args: &[&str]| {
        let output = env.task(args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            stderr(&output)
        );
        stdout(&output)
    };
    quiet(&["add", "other", "project:Other"]);
    quiet(&["context", "define", "home", "project:Home"]);
    // Old-style context.<name> definitions are read contexts.
    quiet(&["config", "context.legacy", "+old"]);
    quiet(&["context", "home"]);

    // Write context: applied to add and log modifications.
    quiet(&["add", "inside"]);
    quiet(&["log", "logged"]);
    let export = quiet(&["export", "all"]);
    let mut projects: Vec<&str> = export
        .lines()
        .filter_map(|l| l.split("\"project\":\"").nth(1))
        .map(|p| p.split('"').next().unwrap())
        .collect();
    projects.sort();
    assert_eq!(projects, ["Home", "Home", "Other"], "{export}");

    // Read context: a filter for reports, with the context footnote (count
    // includes the completed task).
    let output = env.task(&["rc.verbose=footnote,context", "count"]);
    assert_eq!(stdout(&output), "2\n");
    assert_eq!(
        stderr(&output),
        "Context 'home' set. Use 'task context none' to remove.\n"
    );
    // Without the `context` verbosity token there is no footnote.
    let output = env.task(&["rc.verbose=footnote", "count"]);
    assert_eq!(
        (stdout(&output).as_str(), stderr(&output).as_str()),
        ("2\n", "")
    );

    // An ID or UUID bypasses the read context.
    let output = env.task(&["rc.verbose=footnote,context", "1", "count"]);
    assert_eq!(stdout(&output), "1\n");
    assert_eq!(stderr(&output), "");
    let output = env.task(&["rc.verbose=footnote,context", "1-2", "count"]);
    assert_eq!(stdout(&output), "2\n");

    // The legacy definition is used as the read context; it has no write
    // context, so add is unaffected.
    quiet(&["context", "legacy"]);
    assert_eq!(quiet(&["count"]), "0\n");
    quiet(&["add", "plain"]);
    assert_eq!(quiet(&["_get", "3.project", "3.tags"]), " \n");

    quiet(&["context", "none"]);
    assert_eq!(quiet(&["count"]), "4\n");
}

////////////////////////////////////////////////////////////////////////////////
// `edit` (CmdEdit.cpp) with scripted editors.

/// `task rc.verbose=<verbose> <args>` with `TASKRC`/`TASKDATA`, `VISUAL` and
/// `EDITOR` removed unless given in `vars`, and `stdin` piped in.
fn edit_task(env: &Env, args: &[&str], vars: &[(&str, &str)], stdin: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut command = env.command(args);
    command
        .env("TASKRC", env.rc())
        .env("TASKDATA", env.data())
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in vars {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// Writes an executable `/bin/sh` editor script `name` and returns its path.
fn editor_script(env: &Env, name: &str, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = env.path(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

/// An environment with one task; returns it and the edit temp file name.
fn edit_env() -> (Env, String) {
    let env = Env::new("verbose=footnote\nconfirmation=off\n");
    let output = env.task(&["add", "hello", "project:x", "+t"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let uuid = stdout(&env.task(&["_get", "1.uuid"]));
    let file = format!("task.{}.task", &uuid[..8]);
    (env, file)
}

#[test]
fn edit_scripted_editor_modifies_task() {
    let (env, file) = edit_env();
    let editor = editor_script(
        &env,
        "ed.sh",
        "sed -i -e 's/^  Project:.*/  Project: y/' -e 's/^  Description:.*/  Description: bye/' \
         -e 's/^  Tags:.*/  Tags: u v/' \"$1\"",
    );
    let rc = format!("rc.editor={editor}");
    let output = edit_task(&env, &[&rc, "1", "edit"], &[], "");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!(
            "Launching '{editor} \"{file}\"' now...\nEditing complete.\nEdits were detected.\n"
        )
    );
    assert_eq!(
        stderr(&output),
        "Project modified.\nDescription modified.\n"
    );
    assert_eq!(
        stdout(&env.task(&["_get", "1.project", "1.description", "1.tags"])),
        "y bye u,v\n"
    );
    // The temp file is removed.
    assert!(!env.data().join(&file).exists());
}

/// With stdout on a pipe, C++ `std::cout` is fully buffered, so the
/// editor's own output comes before "Launching ..."; the `std::cerr` tie
/// flushes it before the parse error.
#[test]
fn edit_piped_stdout_is_buffered_like_cpp() {
    let (env, file) = edit_env();
    let editor = editor_script(&env, "echo.sh", "echo from-editor");
    let rc = format!("rc.editor={editor}");
    let output = edit_task(&env, &[&rc, "1", "edit"], &[], "");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!(
            "from-editor\nLaunching '{editor} \"{file}\"' now...\nEditing complete.\n\
             No edits were detected.\n"
        )
    );

    let editor = editor_script(
        &env,
        "bad.sh",
        "echo from-editor; sed -i -e 's/^  Due:.*/  Due: notadate/' \"$1\"",
    );
    let rc = format!("rc.editor={editor}");
    let output = edit_task(&env, &[&rc, "1", "edit"], &[], "no\n");
    let out = stdout(&output);
    assert!(
        out.starts_with(&format!(
            "from-editor\nLaunching '{editor} \"{file}\"' now...\nEditing complete.\n\
             Edits were detected.\nTaskwarrior couldn't handle your edits."
        )),
        "{out}"
    );
    assert!(stderr(&output).starts_with("Error: "), "{}", stderr(&output));
}

#[test]
fn edit_without_changes_and_editor_precedence() {
    let (env, file) = edit_env();
    // $VISUAL wins over $EDITOR.
    let output = edit_task(
        &env,
        &["1", "edit"],
        &[("VISUAL", "true"), ("EDITOR", "false")],
        "",
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!("Launching 'true \"{file}\"' now...\nEditing complete.\nNo edits were detected.\n")
    );
    // $EDITOR when $VISUAL is unset; rc.editor wins over both.
    let output = edit_task(&env, &["1", "edit"], &[("EDITOR", "true")], "");
    assert!(stdout(&output).starts_with("Launching 'true "));
    let output = edit_task(&env, &["rc.editor=true", "1", "edit"], &[("VISUAL", "false")], "");
    assert!(stdout(&output).starts_with("Launching 'true "));
}

#[test]
fn edit_editor_failure_and_no_matches() {
    let (env, file) = edit_env();
    let editor = editor_script(&env, "fail.sh", "exit 3");
    let rc = format!("rc.editor={editor}");
    let output = edit_task(&env, &[&rc, "1", "edit"], &[], "");
    assert_eq!(output.status.code(), Some(0));
    // The raw system () wait status, as C++ prints it.
    assert_eq!(
        stdout(&output),
        format!("Launching '{editor} \"{file}\"' now...\nEditing failed with exit code 768.\n")
    );
    assert!(!env.data().join(&file).exists());

    let output = edit_task(&env, &["rc.editor=true", "99", "edit"], &[], "");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "No matches.\n");
}

#[test]
fn edit_already_being_edited() {
    let (env, file) = edit_env();
    std::fs::write(env.data().join(&file), "").unwrap();
    let output = edit_task(&env, &["rc.editor=true", "1", "edit"], &[], "");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "Task is already being edited.\n");
}

#[test]
fn edit_parse_error_declined_and_retried() {
    let (env, file) = edit_env();
    let editor = editor_script(
        &env,
        "bad.sh",
        "sed -i 's/^  Description:.*/  Description:/' \"$1\"",
    );
    let rc = format!("rc.editor={editor}");
    let output = edit_task(&env, &[&rc, "1", "edit"], &[], "no\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!(
            "Launching '{editor} \"{file}\"' now...\nEditing complete.\nEdits were detected.\n\
             Taskwarrior couldn't handle your edits.  Would you like to try again? (yes/no) "
        )
    );
    assert_eq!(stderr(&output), "Error: Cannot remove description.\n");
    assert_eq!(stdout(&env.task(&["_get", "1.description"])), "hello\n");
    assert!(!env.data().join(&file).exists());

    // The second launch sees the preserved (broken) edits and fixes them.
    let marker = env.path("second");
    let editor = editor_script(
        &env,
        "retry.sh",
        &format!(
            "if [ -e '{m}' ]; then grep -q '^  Description:$' \"$1\" && \
             sed -i 's/^  Description:.*/  Description: fixed/' \"$1\"; \
             else touch '{m}'; sed -i 's/^  Description:.*/  Description:/' \"$1\"; fi",
            m = marker.display()
        ),
    );
    let rc = format!("rc.editor={editor}");
    let output = edit_task(&env, &[&rc, "1", "edit"], &[], "yes\n");
    assert_eq!(output.status.code(), Some(0));
    let launching = format!("Launching '{editor} \"{file}\"' now...\n");
    assert_eq!(
        stdout(&output),
        format!(
            "{launching}Editing complete.\nEdits were detected.\n\
             Taskwarrior couldn't handle your edits.  Would you like to try again? (yes/no) \
             {launching}Editing complete.\nEdits were detected.\n"
        )
    );
    assert_eq!(
        stderr(&output),
        "Error: Cannot remove description.\nDescription modified.\n"
    );
    assert_eq!(stdout(&env.task(&["_get", "1.description"])), "fixed\n");
}

#[test]
fn edit_annotations_dependencies_and_udas() {
    let (env, _file) = edit_env();
    assert!(env.task(&["add", "other"]).status.success());
    assert!(env
        .task(&["1", "annotate", "keep me"])
        .status
        .success());
    let editor = editor_script(
        &env,
        "ed.sh",
        "sed -i -e 's/^  Dependencies:.*/  Dependencies: 2/' \
         -e 's/^  UDA priority:.*/  UDA priority: H/' \"$1\"",
    );
    let rc = format!("rc.editor={editor}");
    let output = edit_task(&env, &[&rc, "1", "edit"], &[], "");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stderr(&output), "UDA priority modified.\n");
    let uuid2 = stdout(&env.task(&["_get", "2.uuid"]));
    assert_eq!(
        stdout(&env.task(&["_get", "1.depends", "1.priority", "1.annotations.count"])),
        format!("{} H 1\n", uuid2.trim_end())
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdAliases.cpp, CmdCommands.cpp, CmdHelp.cpp, CmdExec.cpp, CmdLogo.cpp

#[test]
fn completion_aliases_lists_alias_names() {
    let env = Env::new("alias.foo=list\n");
    let output = env.task(&["_aliases"]);
    // The default aliases plus `foo`, in key order.
    assert_eq!(stdout(&output), "burndown\nfoo\nghistory\nhistory\nrm\n");
    assert_eq!(stderr(&output), "");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn commands_table_shows_command_dna() {
    let env = Env::new("defaultwidth=200\nfontunderline=off\n");
    let output = env.task(&["commands"]);
    let out = stdout(&output);
    let row = |name: &str| -> Vec<String> {
        out.lines()
            .find(|l| l.starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("no {name} row in {out}"))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    assert_eq!(row("add")[..6], ["add", "operation", "RW", "Ctxt", "Mods", "Adds"]);
    assert_eq!(
        row("list")[..9],
        ["list", "report", "RO", "ID", "GC", "Recur", "Ctxt", "Filt", "Most"]
    );
    assert_eq!(row("modify")[..6], ["modify", "operation", "RW", "Filt", "Mods", "Modifies"]);
    assert!(out.starts_with("\nCommand "), "{out}");
    assert!(out.ends_with("\n\n"), "{out}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn commands_table_alternate_row_colour() {
    let env = Env::new("defaultwidth=200\n");
    let output = env.task(&["commands", "rc._forcecolor:on"]);
    let out = stdout(&output);
    assert!(out.contains("\x1b[48;5;234m"), "{out}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn completion_commands_sorted_names() {
    let env = Env::new("");
    let output = env.task(&["_commands"]);
    let out = stdout(&output);
    let names: Vec<&str> = out.lines().collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    assert_eq!(names[0], "_aliases");
    // Built-ins and custom reports.
    assert!(names.contains(&"add") && names.contains(&"next"));
    assert_eq!(stderr(&output), "");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn zsh_commands_by_category() {
    let env = Env::new("");
    let output = env.task(&["_zshcommands"]);
    let out = stdout(&output);
    assert!(out.contains("\nadd:operation:Adds a new task\n"), "{out}");
    assert!(out.contains("\nnext:report:Most urgent tasks\n"), "{out}");
    let first = out.lines().next().unwrap();
    assert!(first.split(':').nth(1) == Some("metadata"), "{first}");
    let last = out.lines().last().unwrap();
    assert!(last.split(':').nth(1) == Some("internal"), "{last}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn help_shows_usage_and_help_text() {
    let env = Env::new("alias.zork=list\ndefaultwidth=200\n");
    let output = env.task(&["help"]);
    let out = stdout(&output);
    assert!(out.starts_with("\n"), "{out}");
    assert!(out.contains("\nUsage: task "), "{out}");
    assert!(out.contains("Runs rc.default.command, if specified."));
    assert!(out.contains("Aliased to 'list'"), "{out}");
    assert!(out.contains(
        "\nDocumentation for Taskwarrior can be found using 'man task', 'man taskrc', \
         'man task-color', 'man task-sync' or at https://taskwarrior.org\n"
    ));
    assert!(out.ends_with("  $ ! ' \" ( ) ; \\ ` * ? { } [ ] < > | & % # ~\n\n"));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn help_usage_shows_only_usage() {
    let env = Env::new("");
    let full = stdout(&env.task(&["help"]));
    let output = env.task(&["help", "usage"]);
    let usage = stdout(&output);
    assert!(usage.contains("Usage: task "), "{usage}");
    assert!(!usage.contains("Documentation for Taskwarrior"));
    assert!(full.starts_with(&usage), "usage table is the head of help");
    // An abbreviation of `usage` also works (closeEnough).
    assert_eq!(stdout(&env.task(&["help", "us"])), usage);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn execute_runs_the_command_through_the_shell() {
    let env = Env::new("verbose=nothing\n");
    let output = env.task(&["execute", "echo", "hello"]);
    assert_eq!(stdout(&output), "hello\n");
    assert_eq!(output.status.code(), Some(0));
    // exec.test.py: abbreviation, aliases and default.command.
    assert_eq!(stdout(&env.task(&["exec", "echo", "hi"])), "hi\n");
    let env = Env::new(
        "verbose=nothing\nalias.asdf=execute echo aliased\ndefault.command=exec echo default\n",
    );
    assert_eq!(stdout(&env.task(&["asdf"])), "aliased\n");
    assert_eq!(stdout(&env.task(&[])), "default\n");
}

#[test]
fn execute_returns_the_raw_wait_status() {
    // C++ returns `system`'s wait status (3 << 8); the exit code is its low
    // byte.
    let env = Env::new("verbose=nothing\n");
    let output = env.task(&["execute", "exit", "3"]);
    assert_eq!(output.status.code(), Some(0));
    let output = env.task(&["execute", "kill", "-9", "$$"]);
    assert_eq!(output.status.code(), Some(9));
}

#[test]
fn execute_without_a_command() {
    let env = Env::new("");
    let output = env.task(&["execute"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "Cannot execute an empty command.\n"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn logo_with_colour() {
    let env = Env::new("");
    let output = env.task(&["logo", "rc._forcecolor:on"]);
    let out = stdout(&output);
    assert!(out.starts_with("\n                  \x1b[48;5;232m  \x1b[0m"), "{out:?}");
    assert_eq!(out.lines().count(), 30);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn logo_requires_colour() {
    let env = Env::new("");
    let output = env.task(&["logo"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "The logo command requires that color support is enabled.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}
