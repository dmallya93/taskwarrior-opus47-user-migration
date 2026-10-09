//! Rust translation of `src/CLI2.cpp`.
//!
//! # What is here now (milestone 1, foundation)
//!
//! - The static early-override helpers, ported exactly:
//!   [`get_override`] (C++ `CLI2::getOverride`), [`get_data_location`]
//!   (`CLI2::getDataLocation`) and [`apply_overrides`]
//!   (`CLI2::applyOverrides`), plus their shared private helper `get_value`
//!   (the C++ file-static `getValue`). All three only look at arguments
//!   before the first `--`.
//! - The `CLI2::minimumMatchLength` static ([`minimum_match_length`] /
//!   [`set_minimum_match_length`]).
//! - A *bootstrap* [`Cli2`] object: the C++ member functions that are pure
//!   bookkeeping are ported exactly (`entity`, `alias`, `add`, `getBinary`,
//!   `getCommand`, `exactMatch`, `canonicalize`, `handleArg0`, `findCommand`),
//!   a bootstrap [`Cli2::get_words`] (`getWords` for miscellaneous-only
//!   commands such as `config`), and [`Cli2::analyze`] runs the C++ `analyze` steps that select the
//!   command (arg0 handling, `--` termination, alias expansion, command
//!   lookup, `rc.default.command` / assumed `information`).
//!
//! # What milestone 5 (CLI2 and dispatch) replaces
//!
//! The bootstrap does not lex arguments: every argument is treated as a
//! single `Lexer::Type::word`, alias values and `rc.default.command` are split
//! on whitespace instead of being lexed, and "is this an ID/UUID" is a simple
//! digits-or-UUID check (see [`Cli2::analyze`]). Demotion, canonical names,
//! FILTER/MODIFICATION/MISCELLANEOUS categorisation, `prepareFilter` and the
//! real `dump` all arrive with the full parser port. [`A2`] is a minimal
//! version of the C++ `A2` (raw text, attributes and tags only).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::context::Context;
use crate::error::{Result, TwError};
use crate::lexer::{Lexer, Type as LexerType};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::fs::{File, Path};
use crate::shared::shared::auto_complete;

/// C++ `int CLI2::minimumMatchLength = 3;` (set from `abbreviation.minimum`
/// by `Context::staticInitialization`).
static MINIMUM_MATCH_LENGTH: AtomicI32 = AtomicI32::new(3);

/// Reads C++ `CLI2::minimumMatchLength`.
pub fn minimum_match_length() -> i32 {
    MINIMUM_MATCH_LENGTH.load(Ordering::Relaxed)
}

/// Writes C++ `CLI2::minimumMatchLength`.
pub fn set_minimum_match_length(length: i32) {
    MINIMUM_MATCH_LENGTH.store(length, Ordering::Relaxed);
}

////////////////////////////////////////////////////////////////////////////////
/// C++ file-static `getValue (argc, argv, arg)`: the text after the `:` or `=`
/// of the *last* argument before the first `--` that is `arg` immediately
/// followed by `:` or `=` and at least one more character. `None` if there is
/// no such argument.
///
/// As in C++, `argv[0]` is part of the search.
fn get_value<'a>(args: &'a [String], arg: &str) -> Option<&'a str> {
    let is_arg = |s: &str| {
        let s = s.as_bytes();
        s.len() > arg.len() + 1
            && (s[arg.len()] == b':' || s[arg.len()] == b'=')
            && s.starts_with(arg.as_bytes())
    };

    // find last argument before --
    let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
    args[..end]
        .iter()
        .rev()
        .find(|s| is_arg(s))
        // return the string after : or =
        .map(|s| &s[arg.len() + 1..])
}

////////////////////////////////////////////////////////////////////////////////
/// C++ static `CLI2::getOverride`: if the command line carries `rc:<file>`
/// (or `rc=<file>`) before any `--`, replaces `rc` with that file (the last
/// one wins) and returns `true`.
pub fn get_override(args: &[String], rc: &mut File) -> bool {
    match get_value(args, "rc") {
        None => false,
        Some(value) => {
            *rc = File::new(value);
            true
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ static `CLI2::getDataLocation`: looks for `rc.data.location:<dir>` (or
/// `=<dir>`) before any `--`. If found, `data` becomes that directory and the
/// result is `true`. Otherwise `data` is set from the configured
/// `data.location` (when non-empty) and the result is `false`.
///
/// The C++ reads `Context::getContext().config`; the configuration is passed
/// explicitly here (spec Design Decision 1).
pub fn get_data_location(args: &[String], data: &mut Path, config: &Configuration) -> bool {
    match get_value(args, "rc.data.location") {
        None => {
            let location = config.get("data.location");
            if !location.is_empty() {
                *data = Path::new(&location);
            }
            false
        }
        Some(value) => {
            // C++ `data = Directory (value)`: the Directory slices to its Path.
            *data = Path::new(value);
            true
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ static `CLI2::applyOverrides`: for every argument before the first
/// `--` that starts with `rc.` and has a `:` or `=` after that prefix, sets
/// the configuration variable between `rc.` and the first separator to the
/// text after it (later arguments win). Then, if the `override` verbosity
/// token is active, adds a `Configuration override <arg>` footnote for every
/// argument (before `--`) that starts with `rc.`, separator or not.
pub fn apply_overrides(args: &[String], context: &mut Context) {
    let last = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let is_override = |s: &str| s.starts_with("rc.");

    for raw in &args[..last] {
        if !is_override(raw) {
            continue;
        }
        // s.find_first_of (":=", 3)
        if let Some(offset) = raw[3..].find([':', '=']) {
            let sep = offset + 3;
            let name = &raw[3..sep];
            let value = &raw[sep + 1..];
            context.config.set(name, value);
        }
    }

    if context.verbose("override") {
        for raw in &args[..last] {
            if is_override(raw) {
                context.footnote(&format("Configuration override {1}", &[raw]));
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// Minimal C++ `A2`: one command-line argument, with string attributes
/// (`raw`, `canonical`, `basename`, …) and tags (`ORIGINAL`, `BINARY`, `CMD`,
/// `DEFAULT`, `ASSUMED`, `TERMINATOR`, `TERMINATED`, …).
///
/// Milestone 5 adds the lexer type and the remaining `A2` members.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct A2 {
    attributes: BTreeMap<String, String>,
    tags: Vec<String>,
}

impl A2 {
    /// C++ `A2::A2 (raw, lextype)` (lextype not yet modelled).
    pub fn new(raw: &str) -> Self {
        let mut arg = A2::default();
        arg.set_attribute("raw", raw);
        arg
    }

    /// C++ `A2::attribute (name, value)`.
    pub fn set_attribute(&mut self, name: &str, value: &str) {
        self.attributes.insert(name.to_string(), value.to_string());
    }

    /// C++ `A2::attribute (name)`: the attribute value or `""`.
    pub fn attribute(&self, name: &str) -> String {
        self.attributes.get(name).cloned().unwrap_or_default()
    }

    /// C++ `A2::tag`: adds a tag (no duplicates).
    pub fn tag(&mut self, tag: &str) {
        if !self.has_tag(tag) {
            self.tags.push(tag.to_string());
        }
    }

    /// C++ `A2::hasTag`.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// The `CLI2` object held by `Context` (`Context::cli2`).
///
/// Bootstrap version; see the module documentation for what milestone 5
/// (CLI2 and dispatch) completes.
#[derive(Debug, Clone, Default)]
pub struct Cli2 {
    /// C++ `_aliases`.
    pub aliases: BTreeMap<String, String>,
    /// C++ `_entities` (`std::multimap<category, name>`): per category, the
    /// names in insertion order.
    pub entities: BTreeMap<String, Vec<String>>,
    /// C++ `_original_args`.
    pub original_args: Vec<A2>,
    /// C++ `_args`.
    pub args: Vec<A2>,
    /// C++ `_command`: the cached canonical command, once analysed.
    pub command: String,
}

/// C++ `safetyValveDefault` used by `aliasExpansion`.
const SAFETY_VALVE_DEFAULT: i32 = 10;

impl Cli2 {
    /// C++ `CLI2::CLI2`.
    pub fn new() -> Self {
        Cli2::default()
    }

    /// C++ `CLI2::alias`.
    pub fn alias(&mut self, name: &str, value: &str) {
        self.aliases.insert(name.to_string(), value.to_string());
    }

    /// C++ `CLI2::entity`: records `name` under `category` unless present.
    pub fn entity(&mut self, category: &str, name: &str) {
        let names = self.entities.entry(category.to_string()).or_default();
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }

    /// C++ `CLI2::add`: captures one argument (trimmed of spaces by
    /// [`Lexer::trim`]) and invalidates prior analysis.
    pub fn add(&mut self, argument: &str) {
        let mut arg = A2::new(&Lexer::trim(argument));
        arg.tag("ORIGINAL");
        self.original_args.push(arg);

        // Adding a new argument invalidates prior analysis.
        self.args.clear();
    }

    /// C++ `CLI2::getBinary`.
    pub fn get_binary(&self) -> String {
        self.args.first().map(|a| a.attribute("raw")).unwrap_or_default()
    }

    /// C++ `CLI2::getCommand (canonical = true)`.
    pub fn get_command(&self) -> String {
        self.get_command_form(true)
    }

    /// C++ `CLI2::getCommand (canonical)`.
    pub fn get_command_form(&self, canonical: bool) -> String {
        // Shortcut if analysis has been finalized
        if !self.command.is_empty() {
            return self.command.clone();
        }

        for a in &self.args {
            if a.has_tag("CMD") {
                return a.attribute(if canonical { "canonical" } else { "raw" });
            }
        }

        String::new()
    }

    /// Bootstrap C++ `CLI2::getWords`: the raw text of every MISCELLANEOUS
    /// argument. With `debug.parser` >= 2 a `CLI2::getWords` debug message
    /// lists them.
    ///
    /// The bootstrap does not categorise arguments, so this applies the C++
    /// `categorizeArgs` rules for a command that accepts only miscellaneous
    /// arguments (such as `config`): every argument except the binary
    /// (`BINARY`), the command (`CMD`), the first `--` (a
    /// `Lexer::Type::separator`) and, before that `--`, the `rc:<file>` /
    /// `rc.<name>=<value>` pairs (tagged `RC` / `CONFIG` in C++). Arguments
    /// after `--` are words, so they are always included, as are later
    /// `--`s. Milestone 5 replaces this with the real categorisation.
    ///
    /// `debug_messages` is the Context debug sink (C++
    /// `Context::getContext().debug`).
    pub fn get_words(&self, config: &Configuration, debug_messages: &mut Vec<String>) -> Vec<String> {
        let words: Vec<String> = self
            .args
            .iter()
            .filter(|a| {
                !(a.has_tag("BINARY")
                    || a.has_tag("CMD")
                    || a.has_tag("TERMINATOR")
                    || (!a.has_tag("TERMINATED") && is_rc_pair(&a.attribute("raw"))))
            })
            .map(|a| a.attribute("raw"))
            .collect();

        if config.get_integer("debug.parser") >= 2 {
            let color_orig_args = Color::parse("gray10 on gray4").unwrap_or_default();
            let mut message = " ".to_string();
            for word in &words {
                message += &color_orig_args.colorize(word);
                message.push(' ');
            }
            // C++ `Context::debug` drops empty messages; this one never is.
            debug_messages.push(format!("CLI2::getWords{message}"));
        }

        words
    }

    /// C++ `CLI2::exactMatch`.
    pub fn exact_match(&self, category: &str, value: &str) -> bool {
        self.entities
            .get(category)
            .is_some_and(|names| names.iter().any(|n| n == value))
    }

    /// C++ `CLI2::canonicalize` (without the hash cache, which only affects
    /// speed): an exact entity match, or the single entity that `value`
    /// abbreviates by at least [`minimum_match_length`] characters.
    pub fn canonicalize(&self, category: &str, value: &str) -> Option<String> {
        let names = self.entities.get(category).map(Vec::as_slice).unwrap_or(&[]);
        if names.iter().any(|n| n == value) {
            return Some(value.to_string());
        }

        let matches = auto_complete(value, names, minimum_match_length());
        if matches.len() == 1 {
            return matches.into_iter().next();
        }

        None
    }

    /// Bootstrap C++ `CLI2::analyze`: builds `args` from `original_args`
    /// and finds the command, applying `rc.default.command` or assuming
    /// `information` like C++ `defaultCommand`. Fails with the C++ message
    /// `You must specify a command or a task to modify.` when no command can
    /// be determined.
    ///
    /// Milestone 5 hook: lexing, demotion, canonical names, categorisation
    /// and filter parenthesisation are not yet performed.
    ///
    /// `debug_messages` is the Context debug sink (C++
    /// `Context::getContext().debug`).
    pub fn analyze(&mut self, config: &Configuration, debug_messages: &mut Vec<String>) -> Result<()> {
        // Process _original_args.
        self.args.clear();
        self.handle_arg0();
        self.lex_arguments();

        // Process _args.
        self.alias_expansion(debug_messages);
        if !self.find_command() {
            self.default_command(config);
            if !self.find_command() {
                return Err(TwError::Message(
                    "You must specify a command or a task to modify.".to_string(),
                ));
            }
        }

        // Cache frequently looked up items
        self.command = self.get_command();
        Ok(())
    }

    /// Bootstrap C++ `CLI2::dump`: the bold title and the raw arguments.
    /// Milestone 5 hook: the full colourised parse-tree dump.
    pub fn dump(&self, title: &str) -> String {
        let mut out = format!("\x1b[1m{title}\x1b[0m\n  _original_args\n    ");
        let raws: Vec<String> = self.original_args.iter().map(|a| a.attribute("raw")).collect();
        out.push_str(&raws.join(" "));
        out.push('\n');
        out
    }

    /// C++ `CLI2::prepareFilter`. Milestone 5 hook: no-op until the filter
    /// parser exists.
    pub fn prepare_filter(&mut self) {}

    /// C++ `CLI2::handleArg0`: arg0 is tagged `BINARY`; invoked as `cal` or
    /// `calendar` (via a symlink), a `calendar` argument is added.
    fn handle_arg0(&mut self) {
        let Some(first) = self.original_args.first() else {
            return;
        };
        let raw = first.attribute("raw");
        let mut a = A2::new(&raw);
        a.tag("BINARY");

        let basename = match raw.rfind('/') {
            Some(slash) => raw[slash + 1..].to_string(),
            None => "task".to_string(),
        };

        a.set_attribute("basename", &basename);
        self.args.push(a);
        if basename == "cal" || basename == "calendar" {
            self.args.push(A2::new("calendar"));
        }
    }

    /// Bootstrap C++ `CLI2::lexArguments`: copies arguments 1.. as single
    /// words; the first `--` is tagged `TERMINATOR` and every later argument
    /// `TERMINATED`. Milestone 5 hook: real lexing.
    fn lex_arguments(&mut self) {
        let mut terminated = false;
        for original in self.original_args.iter().skip(1) {
            let raw = original.attribute("raw");
            let mut a = A2::new(&raw);
            if !terminated && raw == "--" {
                terminated = true;
                a.tag("TERMINATOR");
            } else if terminated {
                a.tag("TERMINATED");
            }
            self.args.push(a);
        }
    }

    /// C++ `CLI2::aliasExpansion`: replaces every (non-terminated) argument
    /// that names an alias by the alias value's tokens, repeatedly, at most
    /// `safetyValveDefault` times. Bootstrap: alias values are split on
    /// whitespace instead of lexed (milestone 5 hook).
    fn alias_expansion(&mut self, debug_messages: &mut Vec<String>) {
        let mut counter = 0;
        loop {
            let mut action = false;
            let mut reconstructed = Vec::new();
            for i in &self.args {
                let raw = i.attribute("raw");
                if i.has_tag("TERMINATED") {
                    reconstructed.push(i.clone());
                } else if let Some(value) = self.aliases.get(&raw) {
                    reconstructed.extend(value.split_whitespace().map(A2::new));
                    action = true;
                } else {
                    reconstructed.push(i.clone());
                }
            }
            self.args = reconstructed;

            let mut reconstructed_originals = Vec::new();
            let mut terminated = false;
            for i in &self.original_args {
                let raw = i.attribute("raw");
                if raw == "--" {
                    terminated = true;
                }
                if terminated {
                    reconstructed_originals.push(i.clone());
                } else if let Some(value) = self.aliases.get(&raw) {
                    reconstructed_originals.extend(value.split_whitespace().map(A2::new));
                    action = true;
                } else {
                    reconstructed_originals.push(i.clone());
                }
            }
            self.original_args = reconstructed_originals;

            // C++ `while (action && counter++ < safetyValveDefault)`.
            let again = action && {
                let before = counter;
                counter += 1;
                before < SAFETY_VALVE_DEFAULT
            };
            if !again {
                break;
            }
        }

        if counter >= SAFETY_VALVE_DEFAULT {
            debug_messages.push(format(
                "Nested alias limit of {1} reached.",
                &[&SAFETY_VALVE_DEFAULT],
            ));
        }
    }

    /// C++ `CLI2::findCommand`: tags the first argument that is (or
    /// abbreviates) a command as `CMD`, unless it is exactly an attribute
    /// name. Command DNA tags (`READONLY`, …) are milestone 5.
    fn find_command(&mut self) -> bool {
        for index in 0..self.args.len() {
            let raw = self.args[index].attribute("raw");
            let canonical = if self.exact_match("cmd", &raw) {
                raw.clone()
            } else if self.exact_match("attribute", &raw) {
                continue;
            } else if let Some(canonical) = self.canonicalize("cmd", &raw) {
                canonical
            } else {
                continue;
            };

            let a = &mut self.args[index];
            a.set_attribute("canonical", &canonical);
            a.tag("CMD");
            return true;
        }

        false
    }

    /// C++ `CLI2::defaultCommand`: with no command and no ID/UUID argument,
    /// injects `rc.default.command` (tagged `DEFAULT`) after arg0; with an
    /// ID/UUID, appends an `information` argument tagged `ASSUMED`.
    ///
    /// Bootstrap: an ID/UUID is an all-digit word or a canonical 36-character
    /// UUID; the default command is split on whitespace (milestone 5 hook:
    /// Lexer types).
    fn default_command(&mut self, config: &Configuration) {
        let found_command = self.args.iter().any(|a| a.has_tag("CMD"));
        let found_sequence = self.args.iter().any(|a| looks_like_sequence(&a.attribute("raw")));

        // If no command was specified, then a command will be inserted.
        if !found_command {
            // Default command.
            if !found_sequence {
                // Apply overrides, if any.
                let default_command = config.get("default.command");
                if !default_command.is_empty() && !self.args.is_empty() {
                    // Modify _args, _original_args to be:
                    //   <args0> [<def0> ...] <args1> [...]
                    let mut reconstructed_originals = vec![self.original_args[0].clone()];
                    let mut reconstructed = vec![self.args[0].clone()];

                    for lexeme in default_command.split_whitespace() {
                        reconstructed_originals.push(A2::new(lexeme));
                        let mut cmd = A2::new(lexeme);
                        cmd.tag("DEFAULT");
                        reconstructed.push(cmd);
                    }

                    reconstructed_originals.extend(self.original_args.iter().skip(1).cloned());
                    reconstructed.extend(self.args.iter().skip(1).cloned());

                    self.original_args = reconstructed_originals;
                    self.args = reconstructed;
                }
            } else {
                let mut info = A2::new("information");
                info.tag("ASSUMED");
                self.args.push(info);
            }
        }
    }
}

/// Bootstrap stand-in for "becomes a `Lexer::Type::pair` argument whose
/// `Lexer::decomposePair` name is `rc`" (the C++ `A2` constructor tags those
/// `RC`, or `CONFIG` when there is a modifier).
///
/// C++ `lexArguments` keeps an argument as a pair when its first token is a
/// pair that reaches the end of the argument, or when the argument
/// [`Lexer::was_quoted`] (contains one of ` \t()<>&~`). The first token is a
/// pair when the argument starts with an identifier ([`Lexer::is_identifier`],
/// which stops at `:`, `=`, whitespace and single-character operators)
/// immediately followed by `:`, `=`, `::` or `:=`; an unquoted value then
/// always reaches the end or stops at whitespace or a parenthesis (making the
/// argument "quoted"), so only a value that is a quoted string ending before
/// the argument does falls through to a plain word. The name is the
/// identifier's text before its first `.` (or the whole identifier).
///
/// The identifier scan and the quoting test use the real Lexer (milestone
/// 2). Milestone 5 hook: the pair recognition itself (`Lexer::isPair`,
/// `Lexer::decomposePair`) moves into the full `lexArguments`.
fn is_rc_pair(raw: &str) -> bool {
    let mut identifier = String::new();
    let mut type_ = LexerType::identifier;
    if !Lexer::new(raw).is_identifier(&mut identifier, &mut type_) {
        return false;
    }
    let end = identifier.len();
    let name = raw[..end].split('.').next().unwrap_or("");
    let rest = &raw[end..];
    if name != "rc" || !(rest.starts_with(':') || rest.starts_with('=')) {
        return false;
    }

    let separator = if rest.starts_with("::") || rest.starts_with(":=") { 2 } else { 1 };
    let value = &rest[separator..];
    let short_quoted = value
        .chars()
        .next()
        .filter(|&q| q == '\'' || q == '"')
        .and_then(|q| value[1..].find(q))
        .is_some_and(|close| close + 2 < value.len());

    !short_quoted || Lexer::was_quoted(raw)
}

/// Bootstrap stand-in for "lexes as `Lexer::Type::number` or `uuid`".
fn looks_like_sequence(raw: &str) -> bool {
    let is_number = !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit());
    let is_uuid = raw.len() == 36
        && raw.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    is_number || is_uuid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::env_lock;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn get_value_last_before_terminator() {
        let a = args(&["task", "rc:one", "rc=two", "--", "rc:three"]);
        assert_eq!(get_value(&a, "rc"), Some("two"));
        let a = args(&["task", "rc:", "rc.foo=1", "rcx:1"]);
        assert_eq!(get_value(&a, "rc"), None, "rc: alone needs a value");
        let a = args(&["task", "--", "rc:x"]);
        assert_eq!(get_value(&a, "rc"), None);
        let a = args(&["rc:argv0"]);
        assert_eq!(get_value(&a, "rc"), Some("argv0"), "argv[0] is scanned");
    }

    #[test]
    fn get_override_replaces_file() {
        let mut rc = File::new("/orig/rc");
        assert!(!get_override(&args(&["task", "list"]), &mut rc));
        assert_eq!(rc.data, "/orig/rc");
        assert!(get_override(&args(&["task", "rc:/x/y", "list"]), &mut rc));
        assert_eq!(rc.data, "/x/y");
        assert_eq!(rc.original, "/x/y");
    }

    #[test]
    fn get_data_location_override_and_config() {
        let mut config = Configuration::new();
        let mut data = Path::new("/default");
        assert!(!get_data_location(&args(&["task"]), &mut data, &config));
        assert_eq!(data.data, "/default", "empty data.location keeps data");

        config.set("data.location", "/from/config");
        assert!(!get_data_location(&args(&["task"]), &mut data, &config));
        assert_eq!(data.data, "/from/config");

        let a = args(&["task", "rc.data.location:/a", "rc.data.location=/b", "--", "rc.data.location:/c"]);
        assert!(get_data_location(&a, &mut data, &config));
        assert_eq!(data.data, "/b");
    }

    #[test]
    fn minimum_match_length_static() {
        let _lock = env_lock();
        let old = minimum_match_length();
        set_minimum_match_length(5);
        assert_eq!(minimum_match_length(), 5);
        set_minimum_match_length(old);
    }

    #[test]
    fn entities_aliases_and_command_selection() {
        // Holds the lock because abbreviation depends on the process-wide
        // minimum match length.
        let _lock = env_lock();
        let mut cli = Cli2::new();
        for cmd in ["list", "add", "information", "next", "calendar", "calc"] {
            cli.entity("cmd", cmd);
        }
        cli.entity("cmd", "list");
        assert_eq!(cli.entities["cmd"].len(), 6, "no duplicate entities");
        cli.entity("attribute", "project");

        cli.alias("ls", "list rc.x=1");
        for a in ["/usr/bin/task", " ls ", "+tag"] {
            cli.add(a);
        }
        let mut config = Configuration::new();
        cli.analyze(&config, &mut Vec::new()).unwrap();
        assert_eq!(cli.get_command(), "list");
        assert_eq!(cli.get_binary(), "/usr/bin/task");
        let raws: Vec<String> = cli.args.iter().map(|a| a.attribute("raw")).collect();
        assert_eq!(raws, ["/usr/bin/task", "list", "rc.x=1", "+tag"]);

        // Abbreviation (minimum 3) and ambiguity.
        let mut cli2 = cli.clone();
        cli2.original_args.clear();
        cli2.command.clear();
        cli2.add("task");
        cli2.add("inf");
        cli2.analyze(&config, &mut Vec::new()).unwrap();
        assert_eq!(cli2.get_command(), "information");
        assert_eq!(cli2.get_command_form(true), "information");

        // No command: default.command applies, tagged DEFAULT.
        let mut cli3 = Cli2::new();
        cli3.entities = cli.entities.clone();
        cli3.add("task");
        cli3.add("cal");
        config.set("default.command", "next limit:3");
        cli3.analyze(&config, &mut Vec::new()).unwrap();
        assert_eq!(cli3.get_command(), "next");
        assert!(cli3.args[1].has_tag("DEFAULT"));
        let raws: Vec<String> = cli3.args.iter().map(|a| a.attribute("raw")).collect();
        assert_eq!(raws, ["task", "next", "limit:3", "cal"]);

        // An ID assumes 'information'.
        let mut cli4 = Cli2::new();
        cli4.entities = cli.entities.clone();
        cli4.add("task");
        cli4.add("12");
        cli4.analyze(&config, &mut Vec::new()).unwrap();
        assert_eq!(cli4.get_command(), "information");
        assert!(cli4.args.last().unwrap().has_tag("ASSUMED"));

        // Nothing at all.
        let mut cli5 = Cli2::new();
        cli5.entities = cli.entities.clone();
        cli5.add("task");
        config.set("default.command", "");
        let err = cli5.analyze(&config, &mut Vec::new()).unwrap_err();
        assert_eq!(err.to_string(), "You must specify a command or a task to modify.");

        // Invoked as 'cal'.
        let mut cli6 = Cli2::new();
        cli6.entities = cli.entities.clone();
        cli6.add("/bin/cal");
        cli6.analyze(&config, &mut Vec::new()).unwrap();
        assert_eq!(cli6.get_command(), "calendar");

        // Exact attribute names are not commands.
        let mut cli7 = Cli2::new();
        cli7.entity("cmd", "projects");
        cli7.entity("attribute", "pro");
        cli7.add("task");
        cli7.add("pro");
        cli7.add("proj");
        cli7.analyze(&config, &mut Vec::new()).unwrap();
        assert!(!cli7.args[1].has_tag("CMD"));
        assert!(cli7.args[2].has_tag("CMD"));
        assert_eq!(cli7.args[2].attribute("raw"), "proj");
        // As in C++, the cached _command shortcut ignores `canonical`.
        assert_eq!(cli7.get_command_form(false), "projects");
        assert_eq!(cli7.get_command(), "projects");
    }

    #[test]
    fn terminated_args_are_not_alias_expanded() {
        let mut cli = Cli2::new();
        cli.entity("cmd", "list");
        cli.alias("x", "list");
        for a in ["task", "list", "--", "x"] {
            cli.add(a);
        }
        cli.analyze(&Configuration::new(), &mut Vec::new()).unwrap();
        assert!(cli.args[2].has_tag("TERMINATOR"));
        assert!(cli.args[3].has_tag("TERMINATED"));
        assert_eq!(cli.args[3].attribute("raw"), "x");
    }

    #[test]
    fn nested_alias_limit() {
        let mut cli = Cli2::new();
        cli.entity("cmd", "list");
        cli.alias("a", "a");
        cli.add("task");
        cli.add("a");
        cli.add("list");
        let mut debug = Vec::new();
        cli.analyze(&Configuration::new(), &mut debug).unwrap();
        assert_eq!(debug, ["Nested alias limit of 10 reached."]);
    }

    #[test]
    fn get_words_skips_binary_command_overrides_and_terminator() {
        let words = |list: &[&str]| {
            let mut cli = Cli2::new();
            cli.entity("cmd", "config");
            for a in list {
                cli.add(a);
            }
            cli.analyze(&Configuration::new(), &mut Vec::new()).unwrap();
            cli.get_words(&Configuration::new(), &mut Vec::new())
        };

        assert_eq!(words(&["task", "config", "a", "b c", ""]), ["a", "b c", ""]);
        assert_eq!(
            words(&["task", "rc:/x/rc", "rc.confirmation=off", "config", "rc.a:1", "name", "v"]),
            ["name", "v"]
        );
        assert_eq!(words(&["task", "config", "--", "rc.a=1", "--", "x"]), ["rc.a=1", "--", "x"]);
        assert_eq!(words(&["task", "pre", "config", "post"]), ["pre", "post"], "before the command too");
        assert_eq!(words(&["task", "config", "config"]), ["config"], "only the first is the command");
        // Not single `rc` pairs: they lex as words.
        assert_eq!(
            words(&["task", "config", "rcx=1", "rc", "rc.a-b=1", "rc.a='b'c", "rcfile"]),
            ["rcx=1", "rc", "rc.a-b=1", "rc.a='b'c", "rcfile"]
        );
        // `rc=`, `rc::`, `rc.:`, empty, quoted and "quoted" values are rc pairs.
        assert!(words(&["task", "config", "rc=x", "rc::x", "rc.:x", "rc.a=", "rc.a='b'", "rc.a=b c", "rc.a='b' c"])
            .is_empty());
    }

    #[test]
    fn get_words_debug_parser() {
        let mut cli = Cli2::new();
        cli.entity("cmd", "config");
        for a in ["task", "config", "a", "b"] {
            cli.add(a);
        }
        let mut config = Configuration::new();
        cli.analyze(&config, &mut Vec::new()).unwrap();
        let mut debug = Vec::new();
        cli.get_words(&config, &mut debug);
        assert!(debug.is_empty());

        config.set("debug.parser", 2);
        cli.get_words(&config, &mut debug);
        let color = Color::parse("gray10 on gray4").unwrap();
        assert_eq!(debug, [format!("CLI2::getWords {} {} ", color.colorize("a"), color.colorize("b"))]);
    }

    #[test]
    fn sequences() {
        assert!(looks_like_sequence("1"));
        assert!(looks_like_sequence("a1b2c3d4-0000-1111-2222-333344445555"));
        assert!(!looks_like_sequence("1-2x"));
        assert!(!looks_like_sequence(""));
    }
}
