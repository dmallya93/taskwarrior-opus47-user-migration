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
