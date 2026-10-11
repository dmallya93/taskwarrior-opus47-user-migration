//! End-to-end tests of the `task` binary: the `version` family and
//! configuration loading (`TASKRC`, `rc:`, `rc.x=y` / `rc.x:y` overrides,
//! `include`), `main.cpp`'s exit statuses, and `Context` dispatch through the
//! full CLI2 parser (`rc.debug.parser` dumps, default command, assumed
//! `information`, abbreviations, `--`, argument checks, message colours),
//! and the milestone 5 commands `add`, `log`, `count`, `_get`, `export`, the
//! `ids`/`uuids` family, `_unique` and `_zshattributes`, and the milestone 6
//! report stack (custom reports, `sort.cpp`, `setHeaderUnderline` and the
//! column factory).

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
        env.write("test.rc", &format!("data.location={}\nhooks=off\nnews.version=2.6.0\n{rc}", env.data().display()));
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
            let (key, value) = line.split_once('=').unwrap_or_else(|| panic!("bad line {line:?}"));
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
    let env = Env::new("");
    let output = env.task(&["--help"]);
    assert_eq!(
        stderr(&output),
        env.override_headers() + "Command 'help' is not yet implemented.\n"
    );
    assert_eq!(output.status.code(), Some(2));
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
fn stub_command_is_not_implemented() {
    let env = Env::new("");
    let output = env.task(&["prepend", "x"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "Command 'prepend' is not yet implemented.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn no_override_headers_without_taskrc_taskdata() {
    let env = Env::new("");
    std::fs::copy(env.rc(), env.home().join(".taskrc")).unwrap();
    let output = env.command(&["prepend", "x"]).output().unwrap();
    assert_eq!(stderr(&output), "Command 'prepend' is not yet implemented.\n");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn malformed_rc_file() {
    let env = Env::new("this is not a setting\n");
    let output = env.task(&["_version"]);
    assert_eq!(stderr(&output), "Malformed entry 'this is not a setting' in config file.\n");
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
        let hash = words[1].strip_prefix('(').and_then(|h| h.strip_suffix(')')).unwrap();
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
    assert!(out.starts_with("\n\x1b[1mtask\x1b[0m \x1b[1m3.5.0\x1b[0m built for "), "{out:?}");
    assert!(out.contains("\nTaskwarrior may be copied only under the\nterms of the MIT license, which may be\n"));
}

#[test]
fn version_copyright_follows_tz() {
    let env = Env::new("");
    let out = stdout(&env.command(&["version"]).env("TASKRC", env.rc()).env("TZ", "UTC").output().unwrap());
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
    assert!(out.contains("Copyright (C) 2006 - 2031 T. Babej, P. Beckingham, F. Hernandez."), "{out}");

    // The rc.debug Perf line stamps Datetime ().toISO ().
    let Some(output) = faketime_task(&env, "2031-06-15 12:34:56", &["rc.debug=1", "version"]) else {
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
    env.write("other.rc", &format!("data.location={}\nfoo=from-other\n", env.data().display()));
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
    let map = show(&env.command(&["_show"]).env("TASKRC", env.rc()).output().unwrap());
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
    parse_rc_text(taskwarrior::context::CONFIGURATION_DEFAULTS, &home, &mut expected);
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
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
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
    assert_eq!(stderr(&output), format!("TASKRC override: {}\n", env.rc().display()));
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rc_text(&env), format!("{base}foo=bar\n"));

    // Modify, joining the value words; the value is JSON-encoded.
    let output = config_task(&env, &["config", "foo", "one", "two/\"3\""], "");
    assert_eq!(stdout(&output), modified);
    assert_eq!(rc_text(&env), format!("{base}foo=one two\\/\\\"3\\\"\n"));
    let output = config_task(&env, &["_show"], "");
    assert_eq!(show(&output)["foo"], "one two/\"3\"", "decoded again on load");

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
    assert_eq!(stderr(&output), format!("{header}No entry named 'nosuch' found.\n"));
    assert_eq!(output.status.code(), Some(2));

    let output = config_task(&env, &["config"], "");
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), format!("{header}Specify the name of a config variable to modify.\n"));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rc_text(&env), base);
}

#[test]
fn config_keeps_comments_and_overrides_are_not_words() {
    let env = Env::new("confirmation=off\nfoo=old # why\n# foo=commented\n");
    let base = format!("data.location={}\nhooks=off\nnews.version=2.6.0\nconfirmation=off\n", env.data().display());

    let output = config_task(&env, &["rc.verbose=nothing", "config", "rc.x=1", "foo", "new"], "");
    assert_eq!(stdout(&output), format!("Config file {} modified.\n", env.rc().display()));
    assert_eq!(rc_text(&env), format!("{base}foo=new  # why\n# foo=commented\n"));
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
    assert_eq!(stdout(&output), "Are you sure you want to remove 'foo'? (yes/no) No changes made.\n");
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
    assert_eq!((stdout(&output).as_str(), stderr(&output).as_str()), ("0\n", ""));
    assert_eq!(output.status.code(), Some(0));

    // Bare numbers are not durations (Context::staticInitialization).
    let output = env.task(&["calc", "100", "*", "2"]);
    assert_eq!(stdout(&output), "200\n");

    let output = env.task(&["rc.expressions=postfix", "calc", "12 3600 * 34 60 * 56 + +"]);
    assert_eq!(stdout(&output), "45296\n");

    // Eval debug output goes to the Context debug sink (stderr).
    let output = env.task(&["rc.debug=1", "calc", "2- -3"]);
    assert_eq!(stdout(&output), "5\n");
    assert!(stderr(&output).contains("Eval _neg_ ↓'3' → ↑'-3'\n"), "{}", stderr(&output));

    // Evaluation errors are C++ `std::string` throws, caught by Context::run.
    let output = env.task(&["calc", "1", "+"]);
    assert_eq!(
        (stdout(&output).as_str(), stderr(&output).as_str()),
        ("", "The expression could not be evaluated.\n")
    );
    assert_eq!(output.status.code(), Some(2));
}

fn calc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_calc")).args(args).output().unwrap()
}

#[test]
fn calc_binary() {
    let output = calc(&["12 * 3600 + 34 * 60 + 56"]);
    assert_eq!((stdout(&output).as_str(), stderr(&output).as_str()), ("45296\n", ""));
    assert_eq!(output.status.code(), Some(0));

    let output = calc(&["100*2"]);
    assert_eq!(stdout(&output), "200\n");

    let output = calc(&["--debug", "--postfix", "12 3600 * 34 60 * 56 + +"]);
    let out = stdout(&output);
    assert!(out.contains("\nEval literal number ↑'3600'\n"), "{out}");
    assert!(out.ends_with("\n45296\n"), "{out}");

    let output = calc(&["--debug", "15min"]);
    assert!(stdout(&output).ends_with("Eval literal duration ↑'PT15M'\nPT15M\n"), "{}", stdout(&output));

    let output = calc(&["--help"]);
    let usage = format!("\nUsage: {} [options] '<expression>'\n\nOptions:\n", env!("CARGO_BIN_EXE_calc"));
    assert!(stdout(&output).starts_with(&usage), "{}", stdout(&output));
    assert_eq!(output.status.code(), Some(1));

    let output = calc(&["--version"]);
    assert!(stdout(&output).starts_with("\ncalc 3.5.0 built for "), "{}", stdout(&output));
    assert_eq!(output.status.code(), Some(1));

    let output = calc(&[]);
    assert_eq!((stdout(&output).as_str(), stderr(&output).as_str()), ("", "No expression to evaluate.\n"));
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
    assert!(err.starts_with(&format!("Timer Config::load ({}) ", env.rc().display())), "{err}");
    assert_eq!(err.matches(FINAL_PARSE_TREE).count(), 1, "{err}");
    assert!(err.contains(&format!("{ESC}[1m{FINAL_PARSE_TREE}{ESC}[0m\n  _original_args\n")), "{err}");
    assert!(err.contains("MISCELLANEOUS"), "{err}");
    assert!(err.contains("\nPerf task 3.5.0 "), "{err}");
    // No intermediate pass dumps at level 1.
    assert!(!err.contains("CLI2::analyze"), "{err}");
    assert!(err.ends_with(&(env.override_headers() + "Configuration override rc.debug.parser=1\n")), "{err}");
}

#[test]
fn debug_parser_2_dumps_every_pass_but_no_final_tree() {
    let env = Env::new("");
    let output = env.task(&["rc.debug.parser=2", "calc", "1"]);
    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    for title in ["CLI2::analyze", "CLI2::analyze lexArguments", "CLI2::analyze findCommand", "CLI2::analyze categorizeArgs"] {
        assert!(err.contains(&format!("{ESC}[1m{title}{ESC}[0m\n")), "{title}: {err}");
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
        format!("[{} version rc.verbose=default]\n", env!("CARGO_BIN_EXE_task"))
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
    for args in [&["version", "foo"][..], &["version", "--", "foo"][..], &["_version", "+tag"][..]] {
        let output = env.task(args);
        let word = args.last().unwrap();
        let command = args[0];
        let expected = format!("The '{command}' command does not allow '{word}'.\n");
        assert!(stderr(&output).ends_with(&expected), "{args:?}: {}", stderr(&output));
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
    for args in [&["1", "3"][..], &["a1b2c3d4-0000-4000-8000-000000000000"][..]] {
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
    assert_eq!((stdout(&output), stderr(&output)), (String::new(), String::new()));
    assert_eq!(output.status.code(), Some(1));
    let output = env.task(&["rc.verbose=nothing", "rc.abbreviation.minimum=5", "vers"]);
    assert_eq!((stdout(&output), stderr(&output)), (String::new(), String::new()));
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn attribute_abbreviation_end_to_end() {
    // abbreviation.test.py with count/_get in place of 'list'.
    let env = Env::new("verbose=nothing\nabbreviation.minimum=1\n");
    env.task(&["add", "project:home", "priority:H", "hasattributes"]);
    env.task(&["add", "noattributes"]);
    for arg in ["project:home", "projec:home", "proje:home", "proj:home", "pro:home"] {
        assert_eq!(stdout(&env.task(&[arg, "_ids"])), "1\n", "{arg}");
    }
    for arg in ["priority:H", "priorit:H", "priori:H", "prior:H", "prio:H", "pri:H"] {
        assert_eq!(stdout(&env.task(&[arg, "_ids"])), "1\n", "{arg}");
    }
    // Ambiguous (project/priority): a plain word, matching nothing.
    assert_eq!(stdout(&env.task(&["pr:home", "count"])), "0\n");
    // Abbreviated attribute names also apply to modifications.
    env.task(&["add", "pro:work", "pri:L", "third"]);
    assert_eq!(stdout(&env.task(&["_get", "3.project", "3.priority"])), "work L\n");
    // ...but an ambiguous one is part of the description.
    env.task(&["add", "pr:x", "fourth"]);
    assert_eq!(stdout(&env.task(&["_get", "4.description", "4.project"])), "pr:x fourth \n");
    // Below abbreviation.minimum the name is not recognized.
    assert_eq!(stdout(&env.task(&["rc.abbreviation.minimum=4", "pro:home", "count"])), "0\n");
    assert_eq!(stdout(&env.task(&["rc.abbreviation.minimum=4", "proj:home", "count"])), "1\n");
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
    assert_eq!(stdout(&env.task(&["rc.alias.zz=_ids", "zz", "+today"])), "5\n");
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
    assert_eq!(stderr(&output), env.override_headers() + "Configuration override rc.x=1\n");
    let output = env.task(&["calc", "--", "rc.verbose=nothing", "2"]);
    assert!(stderr(&output).starts_with(&env.override_headers()), "{}", stderr(&output));
    assert!(!stderr(&output).contains("Configuration override"), "{}", stderr(&output));
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
    assert_eq!(stderr(&output), format!("{ESC}[32mThe 'version' command does not allow 'foo'.{ESC}[0m\n"));
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
    assert!(stderr(&output).starts_with(&format!("{ESC}[34mTimer Config::load (")), "{}", stderr(&output));

    // An empty rule leaves the text alone; without a TTY or _forcecolor there
    // is no colour at all.
    let mismatch = "There are different numbers of columns and labels for report 'next'.\n";
    let output = env.task(&["rc._forcecolor=on", "rc.color.error=", "rc.verbose=nothing", "rc.report.next.labels=X"]);
    assert_eq!(stderr(&output), mismatch);
    let output = env.task(&["rc.color=on", "rc.color.error=red", "rc.verbose=nothing", "rc.report.next.labels=X"]);
    assert_eq!(stderr(&output), mismatch);
}

#[test]
fn bad_color_rule_is_reported() {
    let env = Env::new("");
    let output = env.task(&["rc._forcecolor=on", "rc.color.error=chartreuse", "rc.verbose=nothing", "_version"]);
    // initializeColorRules queues the error and initialization continues; the
    // error is printed by run.
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).starts_with("3.5.0"), "{}", stdout(&output));
    assert_eq!(stderr(&output), "The color 'chartreuse' is not recognized.\n");
}

////////////////////////////////////////////////////////////////////////////////
// CmdAdd.cpp, CmdLog.cpp, CmdCount.cpp, CmdGet.cpp, CmdExport.cpp

/// `task rc.verbose=nothing <args>`: stdout, stderr and exit status.
fn quiet(env: &Env, args: &[&str]) -> (String, String, i32) {
    let mut all = vec!["rc.verbose=nothing"];
    all.extend_from_slice(args);
    let output = env.task(&all);
    (stdout(&output), stderr(&output), output.status.code().unwrap())
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
    assert_eq!(quiet(&env, &["add", "four"]), (String::new(), String::new(), 0));
    let tasks = export(&env, &[]);
    let descriptions: Vec<&str> = tasks.iter().map(|t| t["description"].as_str().unwrap()).collect();
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
    let uuid = out.strip_prefix("Created task ").unwrap().strip_suffix(".\n").unwrap();
    assert!(is_uuid(uuid), "{out}");
    assert_eq!(export(&env, &[])[0]["uuid"], uuid);

    // A completed task has no ID, so new-id shows the UUID.
    let output = env.task(&["rc.verbose=new-id", "add", "done", "status:completed"]);
    let out = stdout(&output);
    let uuid = out.strip_prefix("Created task ").unwrap().strip_suffix(".\n").unwrap();
    assert!(is_uuid(uuid), "{out}");

    // Recurrence templates.
    let output = env.task(&["rc.verbose=new-id", "add", "rec", "recur:weekly", "due:tomorrow"]);
    assert_eq!(stdout(&output), "Created task 2 (recurrence template).\n");
    let output = env.task(&["rc.verbose=new-uuid", "add", "rec", "recur:weekly", "due:tomorrow"]);
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
        (String::new(), "Additional text must be provided.\n".into(), 2)
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
    assert_eq!(quiet(&env, &["add", "x", "due:2030-01-01", "scheduled:due"]).2, 0);
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
    let uuid = out.strip_prefix("Logged task ").unwrap().strip_suffix(".\n").unwrap();
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
        (String::new(), "Additional text must be provided.\n".into(), 2)
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
    assert_eq!(quiet(&env, &["count"]).0, "3\n");
    assert_eq!(quiet(&env, &["+tag", "count"]).0, "1\n");
    assert_eq!(quiet(&env, &["status:pending", "count"]).0, "2\n");
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
            &["_get", "1.description", "1.due.year", "rc.verbose", "system.version"]
        ),
        ("one 2030 nothing 3.5.0\n".into(), String::new(), 0)
    );
    // An unresolvable reference is an empty string.
    assert_eq!(quiet(&env, &["_get", "2.description"]), ("\n".into(), String::new(), 0));
    assert_eq!(
        quiet(&env, &["_get", "1.description", "2.description", "1.description"]),
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
    assert_eq!(quiet(&env, &["export"]), ("[\n]\n".into(), String::new(), 0));
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
    assert!(lines[1].starts_with("{\"id\":1,\"description\":\"one\",") && lines[1].ends_with("\"urgency\":0},"));
    assert!(lines[2].starts_with("{\"id\":2,\"description\":\"two\",") && lines[2].ends_with("\"urgency\":0}"));
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
    let uuid = export(&env, &["1"])[0]["uuid"].as_str().unwrap().to_string();
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
    assert_eq!(descriptions(export_report(&env, &[], Some("byrev"))), ["c", "a"]);
    assert_eq!(descriptions(export_report(&env, &[], Some("byr"))), ["c", "a"]);
    assert_eq!(descriptions(export_report(&env, &["/a/"], Some("byrev"))), ["a"]);
    // Without a report: `id,uuid`.
    assert_eq!(descriptions(export(&env, &[])), ["a", "b", "c"]);

    // sort=none: the command line's UUID order.
    let uuids: Vec<String> = export(&env, &[])
        .iter()
        .map(|t| t["uuid"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        descriptions(export_report(&env, &[&uuids[2], &uuids[0], &uuids[1]], Some("asgiven"))),
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
        (String::new(), "Unable to find report that matches 'count'.\n".into(), 2)
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
    assert_eq!((stdout(&output), stderr(&output)), ("1-5\n".into(), String::new()));
}

#[test]
fn completion_ids_list_and_zsh_format() {
    let env = ids_env();
    assert_eq!(quiet(&env, &["_ids"]), ("1\n2\n3\n4\n5\n".into(), String::new(), 0));
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
    assert_eq!(sorted_lines(&out), ["1:one", "2:two", "3:a,b", "4:w", "5:five"]);
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
    assert_eq!(quiet(&env, &["uuids"]), (uuids.join(" ") + "\n", String::new(), 0));
    assert_eq!(quiet(&env, &["_uuids"]), (uuids.join("\n") + "\n", String::new(), 0));
    let done = tasks.iter().find(|t| field(t, "description") == "done").unwrap();
    assert_eq!(quiet(&env, &["status:completed", "uuids"]).0, field(done, "uuid") + "\n");
    assert_eq!(quiet(&env, &["/nomatch/", "uuids"]).0, "\n");
    assert_eq!(quiet(&env, &["/nomatch/", "_uuids"]).0, "\n");
    assert_eq!(quiet(&env, &["/nomatch/", "_zshuuids"]).0, "");

    // uuid:description for every matching task (completed included).
    let mut expected: Vec<String> = tasks
        .iter()
        .map(|t| format!("{}:{}", field(t, "uuid"), field(t, "description").replace(':', ",")))
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
    assert_eq!(quiet(&env, &["_unique", "project"]), ("A\nB\nD\n".into(), String::new(), 0));
    assert_eq!(quiet(&env, &["status:pending", "_unique", "project"]).0, "A\nB\n");
    assert_eq!(quiet(&env, &["_unique", "status"]).0, "completed\npending\n");
    // Abbreviations and UDAs are canonicalized.
    assert_eq!(quiet(&env, &["_unique", "proj"]).0, "A\nB\nD\n");
    assert_eq!(quiet(&env, &["_unique", "size"]).0, "big\nsmall\n");
    // Tags as stored (comma-separated).
    assert_eq!(quiet(&env, &["_unique", "tags"]).0, "x\nx,y\n");
    // `id` is synthesized for non-completed tasks only.
    assert_eq!(quiet(&env, &["_unique", "id"]).0, "1\n2\n3\n");
    // No match: empty output.
    assert_eq!(quiet(&env, &["/nomatch/", "_unique", "project"]), (String::new(), String::new(), 0));
    // Headers are cleared.
    let output = env.task(&["_unique", "project"]);
    assert_eq!((stdout(&output), stderr(&output)), ("A\nB\nD\n".into(), String::new()));

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
        (String::new(), "You must specify an attribute or UDA.\n".into(), 2)
    );
}

////////////////////////////////////////////////////////////////////////////////
// CmdAttributes.cpp

#[test]
fn zsh_attributes_lists_sorted_columns() {
    let builtin = [
        "depends", "description", "due", "end", "entry", "id", "imask", "last", "mask", "modified", "parent",
        // `priority` is a UDA in the compiled-in defaults.
        "priority", "project", "recur", "rtype", "scheduled", "start", "status", "tags", "template", "until", "urgency",
        "uuid", "wait",
    ];
    let lines = |names: &[&str]| names.iter().map(|n| format!("{n}:{n}\n")).collect::<String>();

    let env = Env::new("");
    assert_eq!(quiet(&env, &["_zshattributes"]), (lines(&builtin), String::new(), 0));

    // UDAs are columns too, sorted in with the built-ins.
    let env = Env::new("uda.size.type=string\nuda.aaa.type=numeric\n");
    let mut all = builtin.to_vec();
    all.extend(["size", "aaa"]);
    all.sort();
    let output = env.task(&["_zshattributes"]);
    // Helper (`_`) commands run with verbosity `nothing`: no override headers.
    assert_eq!((stdout(&output), stderr(&output)), (lines(&all), String::new()));
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
    assert!(err.contains("The color 'bogus' is not recognized."), "{err:?}");

    // Legend (any prefix of "legend"), obeying rc overrides.
    let (out, _, rc) = quiet(&env, &["rc._forcecolor:on", "rc.color.debug:red", "colors", "leg"]);
    assert_eq!(rc, 0);
    assert!(out.starts_with("\nHere are the colors currently in use:\n"), "{out:?}");
    assert!(out.contains("color.debug              \x1b[0m \x1b[31mred"), "{out:?}");
    assert!(!out.contains("_forcecolor"));
    assert!(out.ends_with("\x1b[0m\n\n"), "{out:?}");

    // The full chart.
    let (out, _, rc) = quiet(&env, &["rc._forcecolor:on", "colors"]);
    assert_eq!(rc, 0);
    assert!(out.starts_with("\nBasic colors\n \x1b[30m black \x1b[0m \x1b[31m red \x1b[0m"), "{out:?}");
    assert!(out.contains("\n\ncolor0 - color15\n  0 1 2 . . .\n  \x1b[48;5;0m  \x1b[0m"));
    assert!(out.contains("Color cube rgb\x1b[1;31m0\x1b[0m"));
    assert!(out.contains("\x1b[48;5;231m  \x1b[0m \n\nGray ramp gray0 - gray23 (also color232 - color255)\n"));
    assert!(out.ends_with("\x1b[48;5;255m  \x1b[0m\n\nTry running 'task color white on red'.\n\n"), "{out:?}");
}

////////////////////////////////////////////////////////////////////////////////
// CmdUrgency.cpp

#[test]
fn urgency_per_task_and_no_match() {
    let env = Env::new("");
    assert_eq!(quiet(&env, &["add", "one"]).2, 0);
    assert_eq!(quiet(&env, &["add", "two", "+x", "project:P"]).2, 0);
    assert_eq!(quiet(&env, &["1", "_urgency"]), ("task 1 urgency 0\n".into(), String::new(), 0));
    assert_eq!(quiet(&env, &["2", "_urgency"]), ("task 2 urgency 1.8\n".into(), String::new(), 0));
    let (out, _, rc) = quiet(&env, &["_urgency"]);
    let mut lines: Vec<&str> = out.lines().collect();
    lines.sort_unstable();
    assert_eq!((lines, rc), (vec!["task 1 urgency 0", "task 2 urgency 1.8"], 0));
    // Nothing selected: footnote and exit status 1 (helper commands are quiet).
    let output = env.task(&["99", "_urgency"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!((stdout(&output), stderr(&output)), (String::new(), String::new()));
}

#[test]
fn information_tables_and_no_match() {
    let env = Env::new("");
    assert_eq!(quiet(&env, &["add", "one", "+x", "project:P", "due:2030-01-01"]).2, 0);
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
    assert!(lines.contains(&"Virtual tags          BLOCKING PENDING PROJECT READY TAGGED UNBLOCKED"), "{out}");
    // Urgency breakdown and journal.
    assert!(lines.contains(&"    project       1 *    1 =      1"), "{out}");
    assert!(lines.contains(&"    blocking      1 *    8 =      8"), "{out}");
    assert!(lines.contains(&"                               12.2"), "{out}");
    assert!(out.contains("Date       Modification\n---------- "), "{out}");
    assert!(out.contains("Description set to 'one'."), "{out}");
    assert!(out.contains("Due set to '2030-01-01'."), "{out}");
    assert!(out.contains("Tag 'x' added."), "{out}");
    // rc.journal.info=off drops the journal table.
    let (out, _, _) = quiet(&env, &["rc.journal.info=off", "1", "information"]);
    assert!(!out.contains("Modification"), "{out}");
    // Abbreviated keyword; nothing selected: footnote and exit status 1.
    let output = env.task(&["rc.verbose=footnote", "99", "info"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!((stdout(&output), stderr(&output)), (String::new(), "No matches.\n".to_string()));
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
    assert!(out.starts_with("\nColumns     Type   Modifiable"), "{out:?}");
    assert!(out.contains("description string Modifiable combined*") && !out.contains("uuid") && !out.contains("project"));
    // fontunderline (default on) without colour underlines the labels instead of dashes.
    let (out, _, _) = quiet(&env, &["rc.fontunderline=on", "columns", "proj"]);
    assert!(out.starts_with("\x1b[4mColumns\x1b[0m \x1b[4mType  \x1b[0m"), "{out:?}");
    // More than one search word.
    let (out, err, rc) = quiet(&env, &["columns", "a", "b"]);
    assert_eq!((out.as_str(), err.as_str(), rc), ("", "You can only specify one search string.\n", 2));
}

#[test]
fn completion_columns_include_udas() {
    let env = Env::new("uda.zzz.type=string\n");
    let (out, err, rc) = quiet(&env, &["_columns"]);
    assert_eq!((err.as_str(), rc), ("", 0));
    let names: Vec<&str> = out.lines().collect();
    assert_eq!(names.first(), Some(&"depends"));
    assert_eq!(names.last(), Some(&"zzz"));
    assert!(names.contains(&"description") && names.contains(&"urgency") && names.contains(&"uuid"), "{out}");
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
    assert_eq!(lines[2], "---------------- --------------------------------------------------");
    assert_eq!(lines[3], "active           Active tasks");
    assert!(lines.contains(&"mine             Mine"), "{out}");
    assert!(lines.contains(&"information      Shows all data and metadata"), "{out}");
    assert!(out.ends_with("waiting          Waiting (hidden) tasks\n\n30 reports\n"), "{out:?}");
    // Forced colour: the header takes color.label, alternate rows color.alternate.
    let (out, _, rc) = quiet(&env, &["rc._forcecolor=on", "rc.color.alternate=on gray2", "rc.color.label=bold", "reports"]);
    assert_eq!(rc, 0);
    assert!(out.starts_with("\x1b[1mReport          \x1b[0m"), "{out:?}");
    assert!(out.contains("\x1b[48;5;234mactive          \x1b[0m"), "{out:?}");
    assert!(out.ends_with("\n30 reports\n") && !out.contains("\n\n30"), "{out:?}");
}

////////////////////////////////////////////////////////////////////////////////
// CmdCustom: report.* reports

/// The `news` nag footnote CmdCustom adds (`news.version=2.6.0` in the rc).
const NEWS_NAG: &str = "Recently upgraded to 3.5.0. Please run 'task news' to read highlights about the new release.\n";

/// Three tasks with fixed entry dates, for reports run at 2031-06-15 12:00 UTC.
fn report_env(rc: &str) -> Option<Env> {
    let env = Env::new(rc);
    for args in [
        &["rc.verbose=nothing", "add", "one", "entry:2031-06-01", "project:Home", "+x", "due:2031-06-20"][..],
        &["rc.verbose=nothing", "add", "two", "entry:2031-06-14", "priority:H"][..],
        &["rc.verbose=nothing", "add", "three", "entry:2031-06-15T11:00:00"][..],
    ] {
        let output = faketime_task(&env, "2031-06-15 12:00:00", args)?;
        assert_eq!(output.status.code(), Some(0), "{args:?}: {}", stderr(&output));
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
    let output = faketime_task(&env, "2031-06-15 12:00:00", &["rc.verbose=affected,label", "next", "limit:2"]).unwrap();
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
    assert_eq!(stderr(&output), env.override_headers() + "No matches.\n" + NEWS_NAG);
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
        &["rc._forcecolor=on", "rc.fontunderline=off", "rc.color.label=bold", "rc.color.label.sort=red", "mine", "limit:1"],
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
    assert_eq!(stderr(&output), "There are different numbers of columns and labels for report 'mine'.\n");
    assert_eq!(output.status.code(), Some(2));

    // An unknown column.
    let output = env.task(&["rc.verbose=nothing", "rc.report.mine.columns=id,bogus", "rc.report.mine.labels=", "mine"]);
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
        &[&["b1", "project:B"], &["a2", "project:A"], &["b2", "project:B"], &["a1", "project:A"]],
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
        &[&["b1", "project:B"], &["none"], &["a1", "project:A"], &["b2", "project:B"]],
    );
    // `/` after the direction: a blank line wherever the project changes.
    assert_eq!(
        sorted_report(&env, "id,project,description", "project+/,id+", &[]),
        " 2         none\n\n 3 A       a1\n\n 1 B       b1\n 4 B       b2\n"
    );
    // A break column without a direction (`project/`) keeps the slash in
    // the key name (C++ Context::decomposeSortField), so it is rejected.
    let (out, err, rc) = quiet(&env, &["rc.report.r.columns=id", "rc.report.r.sort=project/", "r"]);
    assert_eq!((out.as_str(), err.as_str(), rc), ("", "The 'project/' column is not a valid sort field.\n", 2));
}

#[test]
fn sort_uda_custom_values_order() {
    let env = tasks_env(
        "uda.size.type=string\nuda.size.label=Size\nuda.size.values=S,M,,L\n",
        &[&["large", "size:L"], &["none"], &["small", "size:S"], &["medium", "size:M"]],
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
        sorted_report(&env, "id,size,description", "size+", &["rc.uda.size.values="]),
        " 1 L    large\n 4 M    medium\n 3 S    small\n 2      none\n"
    );
    assert_eq!(
        sorted_report(&env, "id,size,description", "size-", &["rc.uda.size.values="]),
        " 3 S    small\n 4 M    medium\n 1 L    large\n 2      none\n"
    );
}

#[test]
fn sort_by_depends() {
    let env = tasks_env(
        "",
        &[&["first"], &["second"], &["needs2", "depends:2"], &["needs1", "depends:1"], &["free"]],
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
        &[&["low", "priority:L"], &["high", "priority:H"], &["none"], &["medium", "priority:M"]],
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
    let tasks: Vec<Vec<&str>> = tasks.iter().map(|t| t.iter().map(String::as_str).collect()).collect();
    let tasks: Vec<&[&str]> = tasks.iter().map(Vec::as_slice).collect();
    let env = tasks_env("", &tasks);
    // Natural (numeric) ID order: 1, 2, 10, 11 rather than 1, 10, 11, 2.
    assert_eq!(sorted_report(&env, "id,description", "id+", &["1,2,10,11"]), " 1 t1\n 2 t2\n10 t10\n11 t11\n");
    assert_eq!(sorted_report(&env, "id,description", "id-", &["1,2,10,11"]), "11 t11\n10 t10\n 2 t2\n 1 t1\n");
    // Descriptions sort as strings.
    assert_eq!(sorted_report(&env, "id,description", "description+", &["1,2,10,11"]), " 1 t1\n10 t10\n11 t11\n 2 t2\n");
}

#[test]
fn sort_projects_with_hierarchy() {
    let env = tasks_env(
        "",
        &[&["work", "project:Work"], &["garden", "project:Home.Garden"], &["home", "project:Home"], &["hw", "project:Homework"], &["none"]],
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
    let env = tasks_env("color.label=bold\ncolor.label.sort=red\ncolor.alternate=on gray2\n", &[&["one"], &["two"]]);
    let header = |color: &str, underline: &str| {
        let color = format!("rc._forcecolor={color}");
        let underline = format!("rc.fontunderline={underline}");
        sorted_report(&env, "id,description", "id+", &["rc.verbose=label", &color, &underline])
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
    let env = Env::new("defaultwidth=80\ndetection=off\ncolor.label=bold\ncolor.alternate=on gray2\n");
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
        format!("Report           Description\n{} {}\nactive           Active tasks", "-".repeat(16), "-".repeat(50))
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
        format!("\x1b[1;4m{label}\x1b[0m \x1b[1;4m{desc}\x1b[0m\n{alt}\nall              All tasks")
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
            &["one", "size:big", "cost:3", "when:2030-01-02", "span:2d", "ref:a0b1c2d3-e4f5-4789-abcd-0123456789ab"],
            &["two", "cost:12.5", "span:10d"],
        ],
    );
    // Durations render right-aligned in ISO form; dates in rc.dateformat.
    let columns = "id,size,cost,when,span,ref,ref.short,description";
    assert_eq!(
        sorted_report(&env, columns, "id+", &["rc.verbose=label", "rc.defaultwidth=120"]),
        "ID Size Cost When       Span Ref                                  Ref      Description\n\
         -- ---- ---- ---------- ---- ------------------------------------ -------- -----------\n \
         1 big     3 2030-01-02  P2D a0b1c2d3-e4f5-4789-abcd-0123456789ab a0b1c2d3 one\n \
         2      12.5            P10D                                               two\n"
    );
    // UDA styles: indicators (right-aligned, default "U"; widths include
    // the labels), and an unknown style.
    assert_eq!(
        sorted_report(&env, "id,size.indicator,when.indicator,description", "id+", &["rc.uda.when.indicator=W"]),
        " 1    U    W one\n 2           two\n"
    );
    let (_, err, rc) = quiet(&env, &["rc.report.r.columns=id,cost.donkey", "r"]);
    assert_eq!((err.as_str(), rc), ("Unrecognized column format 'cost.donkey'\n", 2));
}

#[test]
fn report_column_factory_errors() {
    let env = tasks_env("", &[&["one"], &["two"]]);
    let (out, err, rc) = quiet(&env, &["rc.report.r.columns=id,bogus", "r"]);
    assert_eq!((out.as_str(), err.as_str(), rc), ("", "Unrecognized column name 'bogus'.\n", 2));
    let (out, err, rc) = quiet(&env, &["rc.report.r.columns=id,description.donkey", "r"]);
    assert_eq!((out.as_str(), err.as_str(), rc), ("", "Unrecognized column format 'description.donkey'\n", 2));
    // An unknown sort key fails in the comparator (C++ validateSortColumns
    // only maps legacy names), so it needs two tasks to compare.
    let (out, err, rc) = quiet(&env, &["rc.report.r.columns=id", "rc.report.r.sort=bogus+", "r"]);
    assert_eq!((out.as_str(), err.as_str(), rc), ("", "The 'bogus' column is not a valid sort field.\n", 2));
    let (out, err, rc) = quiet(&env, &["rc.report.r.columns=id", "rc.report.r.sort=bogus+", "r", "1"]);
    assert_eq!((out.as_str(), err.as_str(), rc), (" 1\n", "", 0));
}

#[test]
fn report_without_labels_uses_column_labels() {
    let env = tasks_env("uda.size.type=string\nuda.size.label=Size\nuda.nolabel.type=string\n", &[&["one", "project:P", "+t", "size:x", "nolabel:y"]]);
    // Each column's default label (Column metadata); a UDA without
    // uda.X.label has an empty one, rendered as blanks (C++ ViewTask::render).
    assert_eq!(
        sorted_report(&env, "id,project,tags,status,size,nolabel,description", "id+", &["rc.verbose=label"]),
        "ID Project Tags Status  Size   Description\n\
         -- ------- ---- ------- ----   -----------\n \
         1 P       t    Pending x    y one\n"
    );
}
