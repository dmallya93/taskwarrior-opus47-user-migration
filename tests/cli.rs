//! End-to-end tests of the `task` binary: the `version` family and
//! configuration loading (`TASKRC`, `rc:`, `rc.x=y` / `rc.x:y` overrides,
//! `include`), plus `main.cpp`'s exit statuses.

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
    let output = env.task(&["add", "x"]);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        env.override_headers() + "Command 'add' is not yet implemented.\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn no_override_headers_without_taskrc_taskdata() {
    let env = Env::new("");
    std::fs::copy(env.rc(), env.home().join(".taskrc")).unwrap();
    let output = env.command(&["add", "x"]).output().unwrap();
    assert_eq!(stderr(&output), "Command 'add' is not yet implemented.\n");
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
