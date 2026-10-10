//! Rust translation of `src/CLI2.h` / `src/CLI2.cpp`: the hand-written
//! command-line grammar (spec Design Decision 5: clap is not used).
//!
//! # Shape (milestone 5 Design Decision 1)
//!
//! [`A2`] mirrors the C++ `A2` member for member: the Lexer type
//! (`_lextype`), the tag list (`std::vector<std::string> _tags`, kept in
//! insertion order) and the attribute map (`std::map`, here a `BTreeMap`).
//! [`Cli2`] mirrors `class CLI2`; every C++ pass (`handleArg0`,
//! `lexArguments`, `aliasExpansion`, `findCommand`, `defaultCommand`,
//! `demotion`, `canonicalizeNames`, `categorizeArgs`,
//! `parenthesizeOriginalFilter`, and the `prepareFilter` passes) is a
//! separate function with the C++ name, called in the C++ order, so the
//! `rc.debug.parser=2` dumps match pass for pass.
//!
//! # Borrowing (milestone 5 Design Decision 2)
//!
//! The C++ passes reach `Context::getContext ()` for the configuration, the
//! debug/footnote sinks, verbosity, the command registry (command DNA,
//! `categorizeArgs`) and `getTaskContext`. Those passes take
//! `ctx: &mut Context` explicitly, and run on a [`Cli2`] that is *detached*
//! from the Context: [`with_cli2`] moves `ctx.cli2` out
//! (`std::mem::take`), runs the closure with the detached parser and the
//! Context, and puts the parser back (also on error). While detached,
//! `ctx.cli2` is an empty placeholder; none of the CLI2 code reads it.
//!
//! Read-only helpers whose C++ only needs the configuration and the debug
//! sink ([`Cli2::get_words`]) take those narrow inputs instead, so commands
//! can call them on `ctx.cli2` directly.
//!
//! The static early-override helpers ([`get_override`],
//! [`get_data_location`], [`apply_overrides`]) scan argv before full parsing
//! and only look at arguments before the first `--`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use crate::context::{strtol, Context};
use crate::error::{Result, TwError};
use crate::lexer::{Lexer, Type as LexerType};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::fs::{File, Path};
use crate::shared::shared::{auto_complete, split};
use crate::shared::utf8::{utf8_character, utf8_next_char};

thread_local! {
    /// C++ `int CLI2::minimumMatchLength = 3;` (set from `abbreviation.minimum`
    /// by `Context::staticInitialization`). Thread-local, like the Lexer and
    /// Datetime statics, so parallel unit tests cannot observe each other's
    /// values.
    static MINIMUM_MATCH_LENGTH: Cell<i32> = const { Cell::new(3) };
}

/// Reads C++ `CLI2::minimumMatchLength`.
pub fn minimum_match_length() -> i32 {
    MINIMUM_MATCH_LENGTH.with(Cell::get)
}

/// Writes C++ `CLI2::minimumMatchLength`.
pub fn set_minimum_match_length(length: i32) {
    MINIMUM_MATCH_LENGTH.with(|c| c.set(length));
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
/// Runs `f` on the Context's CLI2 parser, detached from the Context so that
/// `f` can also borrow the Context mutably (see the module documentation,
/// "Borrowing"). The parser is put back afterwards, whatever `f` returns.
///
/// ```ignore
/// cli2::with_cli2(ctx, |cli2, ctx| cli2.analyze(ctx))?;
/// ```
pub fn with_cli2<R>(context: &mut Context, f: impl FnOnce(&mut Cli2, &mut Context) -> R) -> R {
    let mut cli2 = std::mem::take(&mut context.cli2);
    let result = f(&mut cli2, context);
    context.cli2 = cli2;
    result
}

/// C++ `Context::getContext ().config.getInteger ("debug.parser") >= 2`, the
/// guard of every intermediate parse-tree dump.
fn debug_parser(context: &Context) -> bool {
    context.config.get_integer("debug.parser") >= 2
}

/// C++ `safetyValveDefault`: alias expansion limit. Any more indicates some
/// kind of error.
const SAFETY_VALVE_DEFAULT: i32 = 10;

////////////////////////////////////////////////////////////////////////////////
/// C++ `class A2`: represents a single argument.
#[derive(Debug, Clone, PartialEq)]
pub struct A2 {
    /// C++ `_lextype` (default `Lexer::Type::word`).
    pub lextype: LexerType,
    /// C++ `_tags`, in insertion order, without duplicates.
    pub tags: Vec<String>,
    /// C++ `_attributes`.
    pub attributes: BTreeMap<String, String>,
}

impl A2 {
    /// C++ `A2::A2 (raw, lextype)`: records the type, then sets the `raw`
    /// attribute, which [`decompose`](Self::decompose)s it (tags get
    /// `name`/`sign`, pairs `name`/`modifier`/`separator`/`value` and the
    /// `RC`/`CONFIG` tag, substitutions and patterns their parts).
    pub fn new(raw: &str, lextype: LexerType) -> Self {
        let mut a = A2 {
            lextype,
            tags: Vec::new(),
            attributes: BTreeMap::new(),
        };
        a.set_attribute("raw", raw);
        a
    }

    /// C++ `A2::hasTag`.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// C++ `A2::tag`: adds a tag unless already present.
    pub fn tag(&mut self, tag: &str) {
        if !self.has_tag(tag) {
            self.tags.push(tag.to_string());
        }
    }

    /// C++ `A2::unTag`: removes the first occurrence of a tag.
    pub fn un_tag(&mut self, tag: &str) {
        if let Some(i) = self.tags.iter().position(|t| t == tag) {
            self.tags.remove(i);
        }
    }

    /// C++ `A2::attribute (name, value)`: sets an attribute; setting `raw`
    /// re-runs [`decompose`](Self::decompose).
    pub fn set_attribute(&mut self, name: &str, value: &str) {
        self.attributes.insert(name.to_string(), value.to_string());

        if name == "raw" {
            self.decompose();
        }
    }

    /// C++ `A2::attribute (name)`: the attribute value, or `""` (no
    /// autovivification).
    pub fn attribute(&self, name: &str) -> String {
        self.attributes.get(name).cloned().unwrap_or_default()
    }

    /// C++ `A2::getToken`: the `canonical` attribute if present, else `raw`.
    ///
    /// Deviation: C++ dereferences the end iterator when neither exists
    /// (undefined behaviour; never happens, `raw` is always set); this
    /// returns `""`.
    pub fn get_token(&self) -> String {
        self.attributes
            .get("canonical")
            .or_else(|| self.attributes.get("raw"))
            .cloned()
            .unwrap_or_default()
    }

    /// C++ `A2::decompose`: splits `raw` into its parts according to the
    /// Lexer type.
    pub fn decompose(&mut self) {
        let raw = self.attribute("raw");
        match self.lextype {
            LexerType::tag => {
                // C++ `raw.substr (1)` / `raw.substr (0, 1)`; Lexer tags always
                // start with an ASCII '+' or '-'.
                let name = raw.get(1..).unwrap_or("").to_string();
                let sign = raw.get(..1).unwrap_or("").to_string();
                self.set_attribute("name", &name);
                self.set_attribute("sign", &sign);
            }

            LexerType::substitution => {
                // if (Directory (raw).exists ())
                //   return;

                let mut from = String::new();
                let mut to = String::new();
                let mut flags = String::new();
                if Lexer::decompose_substitution(&raw, &mut from, &mut to, &mut flags) {
                    self.set_attribute("from", &from);
                    self.set_attribute("to", &to);
                    self.set_attribute("flags", &flags);
                }
            }

            LexerType::pair => {
                let mut name = String::new();
                let mut modifier = String::new();
                let mut sep = String::new();
                let mut value = String::new();
                if Lexer::decompose_pair(&raw, &mut name, &mut modifier, &mut sep, &mut value) {
                    self.set_attribute("name", &name);
                    self.set_attribute("modifier", &modifier);
                    self.set_attribute("separator", &sep);
                    self.set_attribute("value", &value);

                    if name == "rc" {
                        if !modifier.is_empty() {
                            self.tag("CONFIG");
                        } else {
                            self.tag("RC");
                        }
                    }
                }
            }

            LexerType::pattern => {
                // if (Directory (raw).exists ())
                //   return;

                let mut pattern = String::new();
                let mut flags = String::new();
                if Lexer::decompose_pattern(&raw, &mut pattern, &mut flags) {
                    self.set_attribute("pattern", &pattern);
                    self.set_attribute("flags", &flags);
                }
            }

            _ => {}
        }
    }

    /// C++ `A2::dump`: the colourised Lexer type, then every attribute as
    /// `name='<yellow>value</yellow>' `, then every tag in its colour
    /// (`BINARY`, `CMD`, `FILTER`, `MODIFICATION`, `MISCELLANEOUS`, `RC`,
    /// `CONFIG`, `?` have their own; the rest are green), each followed by a
    /// space.
    pub fn dump(&self) -> String {
        let output = Lexer::type_to_string(self.lextype);

        // Dump attributes.
        let mut atts = String::new();
        for (name, value) in &self.attributes {
            atts += &format!("{name}='\x1b[33m{value}\x1b[0m' ");
        }

        // Dump tags.
        let mut tags = String::new();
        for tag in &self.tags {
            let color = match tag.as_str() {
                "BINARY" => "\x1b[1;37;44m",
                "CMD" => "\x1b[1;37;46m",
                "FILTER" => "\x1b[1;37;42m",
                "MODIFICATION" => "\x1b[1;37;43m",
                "MISCELLANEOUS" => "\x1b[1;37;45m",
                "RC" => "\x1b[1;37;41m",
                "CONFIG" => "\x1b[1;37;101m",
                "?" => "\x1b[38;5;255;48;5;232m",
                _ => "\x1b[32m",
            };
            tags += &format!("{color}{tag}\x1b[0m ");
        }

        format!("{output} {atts}{tags}")
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class CLI2`: represents the command line. Held by `Context`
/// (`Context::cli2`).
#[derive(Debug, Clone, Default)]
pub struct Cli2 {
    /// C++ `_entities` (`std::multimap<category, name>`): per category, the
    /// names in insertion order (the multimap's iteration order).
    pub entities: BTreeMap<String, Vec<String>>,
    /// C++ `_aliases`.
    pub aliases: BTreeMap<String, String>,
    /// C++ `_canonical_cache`: memoised [`canonicalize`](Self::canonicalize)
    /// results (interior mutability because `canonicalize` is called while
    /// arguments are borrowed).
    ///
    /// Deviation: C++ keys the cache by the `int` truncation of
    /// `31 * hash (category) + hash (value)`, so colliding keys could return
    /// a wrong cached value; this keys by the `(category, value)` pair.
    pub canonical_cache: RefCell<HashMap<(String, String), String>>,
    /// C++ `_original_args`.
    pub original_args: Vec<A2>,
    /// C++ `_args`.
    pub args: Vec<A2>,
    /// C++ `_id_ranges`: `(low, high)` ID ranges found by `findIDs`.
    pub id_ranges: Vec<(String, String)>,
    /// C++ `_uuid_list`: UUIDs found by `findUUIDs`.
    pub uuid_list: Vec<String>,
    /// C++ `_command`: the cached canonical command, once analysed.
    pub command: String,
    /// C++ `_context_added`: recursion block for [`add_context`](Self::add_context).
    pub context_added: bool,
}

impl Cli2 {
    /// C++ `CLI2::CLI2 () = default`.
    pub fn new() -> Self {
        Cli2::default()
    }

    /// C++ `CLI2::alias`.
    pub fn alias(&mut self, name: &str, value: &str) {
        self.aliases.insert(name.to_string(), value.to_string());
    }

    /// C++ `CLI2::entity`: records `name` under `category` unless the pair is
    /// already present.
    pub fn entity(&mut self, category: &str, name: &str) {
        // Walk the list of entities for category.
        let names = self.entities.entry(category.to_string()).or_default();
        if names.iter().any(|n| n == name) {
            return;
        }

        // The category/name pair was not found, therefore add it.
        names.push(name.to_string());
    }

    /// C++ `CLI2::add (const std::string&)`: captures a single argument
    /// (trimmed by [`Lexer::trim`], tagged `ORIGINAL`).
    pub fn add(&mut self, argument: &str) {
        let mut arg = A2::new(&Lexer::trim(argument), LexerType::word);
        arg.tag("ORIGINAL");
        self.original_args.push(arg);

        // Adding a new argument invalidates prior analysis.
        self.args.clear();
    }

    /// C++ `CLI2::add (const std::vector<std::string>&, int offset = 0)`:
    /// captures a set of arguments (as untagged words), inserted immediately
    /// after `offset` arguments after the binary.
    ///
    /// Deviation: C++ has undefined behaviour when `offset + 1` exceeds the
    /// number of original arguments; here the arguments are appended.
    pub fn add_args(&mut self, arguments: &[String], offset: usize) {
        let keep = (offset + 1).min(self.original_args.len());
        let mut replacement: Vec<A2> = self.original_args[..keep].to_vec();

        for arg in arguments {
            replacement.push(A2::new(arg, LexerType::word));
        }

        for i in (1 + offset)..self.original_args.len() {
            replacement.push(self.original_args[i].clone());
        }

        self.original_args = replacement;

        // Adding a new argument invalidates prior analysis.
        self.args.clear();
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::analyze`: intended to be called after [`add`](Self::add) to
    /// perform the final analysis. Fails with `You must specify a command or
    /// a task to modify.` when no command can be determined, and with the
    /// `categorizeArgs` errors.
    pub fn analyze(&mut self, ctx: &mut Context) -> Result<()> {
        if debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::analyze"));
        }

        // Process _original_args.
        self.args.clear();
        self.handle_arg0();
        self.lex_arguments(ctx);

        // Process _args.
        self.alias_expansion(ctx);
        if !self.find_command(ctx) {
            self.default_command(ctx);
            if !self.find_command(ctx) {
                return Err(TwError::Message(
                    "You must specify a command or a task to modify.".to_string(),
                ));
            }
        }

        self.demotion(ctx);
        self.canonicalize_names(ctx);

        // Determine arg types: FILTER, MODIFICATION, MISCELLANEOUS.
        self.categorize_args(ctx)?;
        self.parenthesize_original_filter(ctx);

        // Cache frequently looked up items
        self.command = self.get_command();
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::addFilter`: processes a raw filter string, inserting its
    /// lexed tokens (wrapped in parentheses) immediately after the binary,
    /// then re-analyses.
    pub fn add_filter(&mut self, ctx: &mut Context, arg: &str) -> Result<()> {
        if !arg.is_empty() {
            let mut filter = vec!["(".to_string()];

            let mut lexeme = String::new();
            let mut type_ = LexerType::word;
            let mut lex = Lexer::new(arg);

            while lex.token(&mut lexeme, &mut type_) {
                filter.push(lexeme.clone());
            }

            filter.push(")".to_string());
            self.add_args(&filter, 0);
            self.analyze(ctx)?;
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::addModifications`: processes a raw modification string,
    /// inserting its lexed tokens immediately after the command (i.e. `add`),
    /// then re-analyses.
    ///
    /// As in C++, the command's index in `_args` is used as the offset into
    /// `_original_args`.
    pub fn add_modifications(&mut self, ctx: &mut Context, arg: &str) -> Result<()> {
        if !arg.is_empty() {
            let mut mods = Vec::new();

            let mut lexeme = String::new();
            let mut type_ = LexerType::word;
            let mut lex = Lexer::new(arg);

            while lex.token(&mut lexeme, &mut type_) {
                mods.push(lexeme.clone());
            }

            // Determine at which argument index does the task modification command
            // reside
            let cmd_index = self.args.iter().position(|a| a.has_tag("CMD")).unwrap_or(self.args.len());

            // Insert modifications after the command.
            self.add_args(&mods, cmd_index);
            self.analyze(ctx)?;
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::addContext`: there are situations where a context filter is
    /// applied. This method determines whether one applies, and if so,
    /// applies it (`context.<name>.read` as a filter for `readable`
    /// commands, else `context.<name>.write` as modifications for
    /// `writeable` ones; see [`Context::get_task_context`]).
    /// Disqualifiers include:
    ///   - filter contains ID or UUID
    pub fn add_context(&mut self, ctx: &mut Context, readable: bool, writeable: bool) -> Result<()> {
        // Recursion block.
        if self.context_added {
            return Ok(());
        }

        // Detect if any context is set, and bail out if not
        let context_string = if readable {
            // Empty string is treated as "currently selected context"
            ctx.get_task_context("read", "", true)
        } else if writeable {
            ctx.get_task_context("write", "", true)
        } else {
            return Ok(());
        };

        // If context is empty, bail out too
        if context_string.is_empty() {
            return Ok(());
        }

        // For readable contexts: Detect if UUID or ID is set, and bail out
        if readable {
            for a in &self.args {
                if matches!(a.lextype, LexerType::uuid | LexerType::number | LexerType::set) {
                    ctx.debug(&format(
                        "UUID/ID argument found '{1}', not applying context.",
                        &[&a.attribute("raw")],
                    ));
                    return Ok(());
                }
            }
        }

        // Apply the context. Readable (filtering) takes precedence. Also set the
        // block now, since addFilter calls analyze(), which calls addContext().
        self.context_added = true;
        if readable {
            self.add_filter(ctx, &context_string)?;
        } else if writeable {
            self.add_modifications(ctx, &context_string)?;
        }

        // Inform the user about the application of context
        if ctx.verbose("context") {
            let name = ctx.config.get("context");
            ctx.footnote(&format(
                "Context '{1}' set. Use 'task context none' to remove.",
                &[&name],
            ));
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::prepareFilter`: parses the command line, identifying filter
    /// components and expanding syntactic sugar as necessary. With the
    /// `filter` verbosity token, footnotes `Filter: <raw FILTER args>`.
    ///
    /// Fails with the `desugarFilterAttributes` errors.
    pub fn prepare_filter(&mut self, ctx: &mut Context) -> Result<()> {
        // Clear and re-populate.
        self.id_ranges.clear();
        self.uuid_list.clear();
        self.context_added = false;

        // Remove all the syntactic sugar for FILTERs.
        self.lex_filter_args(ctx);
        self.find_ids(ctx);
        self.find_uuids(ctx);
        self.insert_id_expr(ctx);
        self.desugar_filter_plain_args(ctx);
        self.find_stray_modifications(ctx);
        self.desugar_filter_tags(ctx);
        self.desugar_filter_attributes(ctx)?;
        self.desugar_filter_patterns(ctx);
        self.insert_junctions(ctx); // Deliberately after all desugar calls.

        if ctx.verbose("filter") {
            let mut combined = String::new();
            for a in &self.args {
                if a.has_tag("FILTER") {
                    if !combined.is_empty() {
                        combined.push(' ');
                    }

                    combined += &a.attribute("raw");
                }
            }

            if !combined.is_empty() {
                ctx.footnote(&format!("Filter: {combined}"));
            }
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::getWords`: the raw text of every `MISCELLANEOUS` argument.
    /// With `debug.parser` >= 2 a `CLI2::getWords` debug message lists them.
    ///
    /// Takes the configuration and the Context debug sink (C++
    /// `Context::getContext ().debug`) rather than the Context, so commands
    /// can call it on `ctx.cli2` directly.
    pub fn get_words(&self, config: &Configuration, debug_messages: &mut Vec<String>) -> Vec<String> {
        let words: Vec<String> = self
            .args
            .iter()
            .filter(|a| a.has_tag("MISCELLANEOUS"))
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

    /// C++ `CLI2::getMiscellaneous`: all the `MISCELLANEOUS` args.
    pub fn get_miscellaneous(&self) -> Vec<A2> {
        self.args.iter().filter(|a| a.has_tag("MISCELLANEOUS")).cloned().collect()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::canonicalize`: searches for `value` in the `category`
    /// entities; returns the exact match, or the single entity that `value`
    /// abbreviates by at least [`minimum_match_length`] characters.
    pub fn canonicalize(&self, category: &str, value: &str) -> Option<String> {
        // Utilize a cache mapping of (category, value) -> canonicalized value.
        // This cache does not need to be invalidated, because entities are defined
        // only once per initialization of the Context object.
        let cache_key = (category.to_string(), value.to_string());
        if let Some(cached) = self.canonical_cache.borrow().get(&cache_key) {
            return Some(cached.clone());
        }

        // Extract a list of entities for category.
        let mut options: Vec<&String> = Vec::new();
        for e in self.entities.get(category).map(Vec::as_slice).unwrap_or(&[]) {
            // Shortcut: if an exact match is found, success.
            if value == e {
                self.canonical_cache.borrow_mut().insert(cache_key, value.to_string());
                return Some(value.to_string());
            }

            options.push(e);
        }

        // Match against the options, throw away results.
        let matches = auto_complete(value, &options, minimum_match_length());
        if matches.len() == 1 {
            let canonical = matches[0].clone();
            self.canonical_cache.borrow_mut().insert(cache_key, canonical.clone());
            return Some(canonical);
        }

        None
    }

    /// C++ `CLI2::getBinary`.
    pub fn get_binary(&self) -> String {
        self.args.first().map(|a| a.attribute("raw")).unwrap_or_default()
    }

    /// C++ `CLI2::getCommand ()` (`canonical = true`).
    pub fn get_command(&self) -> String {
        self.get_command_form(true)
    }

    /// C++ `CLI2::getCommand (canonical)`. As in C++, once analysis has been
    /// finalised the cached `_command` is returned whatever `canonical` is.
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

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::dump (title = "CLI2 Parser")`: the full parse-tree dump used
    /// for `rc.debug.parser`: the bold title, `_original_args` (ORIGINAL ones
    /// in `gray10 on gray4`, inserted ones in `black on rgb311`), every
    /// `_args` entry ([`A2::dump`]), and `_id_ranges` / `_uuid_list` when
    /// present.
    pub fn dump(&self, title: &str) -> String {
        let mut out = format!("\x1b[1m{title}\x1b[0m\n  _original_args\n    ");

        let color_args = Color::parse("gray10 on gray4").unwrap_or_default();
        let color_filter = Color::parse("black on rgb311").unwrap_or_default();
        for (i, a) in self.original_args.iter().enumerate() {
            if i != 0 {
                out.push(' ');
            }

            if a.has_tag("ORIGINAL") {
                out += &color_args.colorize(&a.attribute("raw"));
            } else {
                out += &color_filter.colorize(&a.attribute("raw"));
            }
        }

        out.push('\n');

        if !self.args.is_empty() {
            out += "  _args\n";
            for a in &self.args {
                out += "    ";
                out += &a.dump();
                out.push('\n');
            }
        }

        if !self.id_ranges.is_empty() {
            out += "  _id_ranges\n    ";
            for (first, second) in &self.id_ranges {
                if first != second {
                    out += &color_args.colorize(&format!("{first}-{second}"));
                } else {
                    out += &color_args.colorize(first);
                }
                out.push(' ');
            }

            out.push('\n');
        }

        if !self.uuid_list.is_empty() {
            out += "  _uuid_list\n    ";
            for uuid in &self.uuid_list {
                out += &color_args.colorize(uuid);
                out.push(' ');
            }

            out.push('\n');
        }

        out
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::handleArg0`: arg0 is the first argument, which is the name
    /// and potentially a relative or absolute path to the invoked binary.
    ///
    /// The binary name is 'task', but if the binary is reported as 'cal' or
    /// 'calendar' then it was invoked via symbolic link, in which case capture
    /// the first argument as 'calendar'.
    ///
    /// Deviation: C++ indexes `_original_args[0]` unconditionally (undefined
    /// behaviour when nothing was added); here nothing happens.
    fn handle_arg0(&mut self) {
        // Capture arg0 separately, because it is the command that was run, and could
        // need special handling.
        let Some(first) = self.original_args.first() else {
            return;
        };
        let raw = first.attribute("raw");
        let mut a = A2::new(&raw, LexerType::word);
        a.tag("BINARY");

        let mut basename = "task".to_string();
        if let Some(slash) = raw.rfind('/') {
            basename = raw[slash + 1..].to_string();
        }

        a.set_attribute("basename", &basename);
        if basename == "cal" || basename == "calendar" {
            self.args.push(a);

            let cal = A2::new("calendar", LexerType::word);
            self.args.push(cal);
        } else {
            self.args.push(a);
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::lexArguments`: all arguments must be individually and
    /// wholly recognized by the Lexer. Any argument not recognized is
    /// considered a `Lexer::Type::word` (with unescaped single quotes
    /// escaped, then read and dequoted as one quoted word).
    ///
    /// As a side effect, tags all arguments after a terminator (`--`), and the
    /// terminator itself, with `TERMINATED`; they become words.
    fn lex_arguments(&mut self, ctx: &mut Context) {
        // Note: Starts iterating at index 1, because ::handleArg0 has already
        //       processed it.
        let mut terminated = false;
        for i in 1..self.original_args.len() {
            let raw = self.original_args[i].attribute("raw");
            let is_original = self.original_args[i].has_tag("ORIGINAL");
            let quoted = Lexer::was_quoted(&raw);

            // Process single-token arguments.
            let mut lexeme = String::new();
            let mut type_ = LexerType::word;
            let mut lex = Lexer::new(&raw);
            // Token goes to EOS, or quoted pairs automatically go to EOS.
            if lex.token(&mut lexeme, &mut type_) && (lex.is_eos() || (quoted && type_ == LexerType::pair)) {
                if !terminated && type_ == LexerType::separator {
                    terminated = true;
                } else if terminated {
                    type_ = LexerType::word;
                }

                let mut a = A2::new(&raw, type_);
                if terminated {
                    a.tag("TERMINATED");
                }
                if quoted {
                    a.tag("QUOTED");
                }

                if is_original {
                    a.tag("ORIGINAL");
                }

                self.args.push(a);
            }
            // Process multiple-token arguments.
            else {
                let quote = "'";

                // Escape unescaped single quotes
                let mut escaped = String::with_capacity(raw.len());

                let mut cursor = 0usize;
                let mut next_escaped = false;
                loop {
                    let num = utf8_next_char(&raw, &mut cursor);
                    if num == 0 {
                        break;
                    }
                    let character = utf8_character(num);
                    if !next_escaped && character == "\\" {
                        next_escaped = true;
                    } else {
                        if character == quote && !next_escaped {
                            escaped.push('\\');
                        }
                        next_escaped = false;
                    }
                    escaped += &character;
                }

                cursor = 0;
                let mut word = String::new();
                if Lexer::read_word_quoted(&format!("{quote}{escaped}{quote}"), quote, &mut cursor, &mut word) {
                    Lexer::dequote(&mut word);
                    let mut unknown = A2::new(&word, LexerType::word);
                    if Lexer::was_quoted(&raw) {
                        unknown.tag("QUOTED");
                    }

                    if is_original {
                        unknown.tag("ORIGINAL");
                    }

                    self.args.push(unknown);
                }
                // This branch may have no use-case.
                else {
                    let mut unknown = A2::new(&raw, LexerType::word);
                    unknown.tag("UNKNOWN");

                    if Lexer::was_quoted(&raw) {
                        unknown.tag("QUOTED");
                    }

                    if is_original {
                        unknown.tag("ORIGINAL");
                    }

                    self.args.push(unknown);
                }
            }
        }

        if debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::analyze lexArguments"));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::demotion`:
    /// [1] Scan all args for the 'add' and 'log' commands, and demote any
    ///     `Lexer::Type::tag` args with sign '-' to `Lexer::Type::word`.
    /// [2] Convert any pseudo args name:value into config settings, and erase.
    ///
    /// As in C++, the "erase" of [2] has no effect: the C++ builds the
    /// `replacement` list without the pseudo args but never assigns it to
    /// `_args`, so the pseudo args stay in the argument list (only the
    /// configuration changes). The `-tag` demotion happens in place.
    fn demotion(&mut self, ctx: &mut Context) {
        let mut changes = false;
        let mut replacement: Vec<A2> = Vec::new();

        for i in 0..self.args.len() {
            if self.args[i].lextype == LexerType::tag && self.args[i].attribute("sign") == "-" {
                let command = self.get_command();
                if command == "add" || command == "log" {
                    self.args[i].lextype = LexerType::word;
                    changes = true;
                }
            } else if self.args[i].lextype == LexerType::pair {
                if let Some(canonical) = self.canonicalize("pseudo", &self.args[i].attribute("name")) {
                    ctx.config.set(&canonical, self.args[i].attribute("value"));
                    changes = true;

                    // Equivalent to erasing 'a'.
                    continue;
                }
            }

            replacement.push(self.args[i].clone());
        }
        // C++ quirk preserved: `replacement` is discarded (see above).
        drop(replacement);

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::analyze demotion"));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::aliasExpansion`: if any aliases are found in un-TERMINATED
    /// arguments, replace the alias with a set of Lexed tokens from the
    /// configuration. Repeats (for nested aliases) while anything was
    /// expanded, at most `safetyValveDefault` extra times; reaching the limit
    /// queues `Nested alias limit of 10 reached.` as a debug message.
    ///
    /// `_original_args` are expanded the same way, up to the first `--`.
    fn alias_expansion(&mut self, ctx: &mut Context) {
        let mut changes = false;
        let mut counter = 0;
        loop {
            let mut action = false;
            let mut reconstructed = Vec::new();

            for i in &self.args {
                let raw = i.attribute("raw");
                if i.has_tag("TERMINATED") {
                    reconstructed.push(i.clone());
                } else if let Some(value) = self.aliases.get(&raw) {
                    lex_into(value, &mut reconstructed);

                    action = true;
                    changes = true;
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
                    lex_into(value, &mut reconstructed_originals);

                    action = true;
                    changes = true;
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
            ctx.debug(&format("Nested alias limit of {1} reached.", &[&SAFETY_VALVE_DEFAULT]));
        }

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::analyze aliasExpansion"));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::canonicalizeNames`: scan all arguments and canonicalize
    /// names that need it. Every pair not starting with `rc:` / `rc.` gets a
    /// `canonical` attribute (a pseudo or attribute name), or is demoted to
    /// a word when its name is neither.
    fn canonicalize_names(&mut self, ctx: &mut Context) {
        let mut changes = false;
        for i in 0..self.args.len() {
            if self.args[i].lextype == LexerType::pair {
                let raw = self.args[i].attribute("raw");
                if !raw.starts_with("rc:") && !raw.starts_with("rc.") {
                    let name = self.args[i].attribute("name");
                    if let Some(canonical) = self
                        .canonicalize("pseudo", &name)
                        .or_else(|| self.canonicalize("attribute", &name))
                    {
                        self.args[i].set_attribute("canonical", &canonical);
                    } else {
                        self.args[i].lextype = LexerType::word;
                    }

                    changes = true;
                }
            }
        }

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::analyze canonicalizeNames"));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::categorizeArgs`: categorize FILTER, MODIFICATION and
    /// MISCELLANEOUS args, based on CMD DNA. Applies the context first
    /// ([`add_context`](Self::add_context)) for commands that use it.
    ///
    /// Fails with `The '<cmd>' command does not allow '<arg>'.` for an
    /// argument to a command that accepts none, and `Unknown error. Please
    /// report.` (a C++ `throw std::string`) for internally inconsistent DNA.
    ///
    /// The C++ `dynamic_cast` special cases for `CmdCustom` / `CmdTimesheet`
    /// are the [`Command::uses_context`](crate::commands::command::Command::uses_context)
    /// overrides here.
    fn categorize_args(&mut self, ctx: &mut Context) -> Result<()> {
        // Context is only applied for commands that request it.
        let command = self.get_command();
        let cmd = ctx.commands.get(&command).cloned();

        // Determine if the command uses Context. CmdCustom and CmdTimesheet need to
        // be handled separately, as they override the parent Command::use_context
        // method, and this is a pointer to Command class.
        //
        // All Command classes overriding uses_context () getter need to be specified
        // here.
        let uses_context = cmd.as_ref().is_some_and(|c| c.uses_context(&ctx.config));

        // Apply the context, if applicable
        if let Some(c) = &cmd {
            if uses_context {
                self.add_context(ctx, c.accepts_filter(), c.accepts_modifications())?;
            }
        }

        let mut changes = false;
        let mut after_command = false;
        for a in self.args.iter_mut() {
            if a.lextype == LexerType::separator {
                continue;
            }

            // Record that the command has been found, it affects behavior.
            if a.has_tag("CMD") {
                after_command = true;
                continue;
            }

            // Skip admin args.
            if a.has_tag("BINARY") || a.has_tag("RC") || a.has_tag("CONFIG") {
                // NOP.
                continue;
            }

            let Some(c) = &cmd else {
                continue;
            };

            // All combinations, with all 8 cases handled below.:
            //
            //   -- -- --   Error: found an arg, but none expected
            //   -- -- Mi   task [Mi] <cmd> [Mi]
            //   -- Mo --   task [Mo] <cmd> [Mo]
            //   -- Mo Mi   Internally inconsistent
            //   Fi -- --   task [Fi] <cmd> [Fi]
            //   Fi -- Mi   task [Fi] <cmd> [Mi]
            //   Fi Mo --   task [Fi] <cmd> [Mo]
            //   Fi Mo Mi   Internally inconsistent
            //
            match (c.accepts_filter(), c.accepts_modifications(), c.accepts_miscellaneous()) {
                (false, false, false) => {
                    // No commands were expected --> error.
                    return Err(TwError::Message(format(
                        "The '{1}' command does not allow '{2}'.",
                        &[&command, &a.attribute("raw")],
                    )));
                }
                (false, false, true) => {
                    a.tag("MISCELLANEOUS");
                    changes = true;
                }
                (false, true, false) => {
                    a.tag("MODIFICATION");
                    changes = true;
                }
                (false, true, true) | (true, true, true) => {
                    // Error: internally inconsistent.
                    return Err(TwError::Message("Unknown error. Please report.".to_string()));
                }
                (true, false, false) => {
                    a.tag("FILTER");
                    changes = true;
                }
                (true, false, true) => {
                    if !after_command {
                        a.tag("FILTER");
                    } else {
                        a.tag("MISCELLANEOUS");
                    }

                    changes = true;
                }
                (true, true, false) => {
                    if !after_command {
                        a.tag("FILTER");
                    } else {
                        a.tag("MODIFICATION");
                    }

                    changes = true;
                }
            }
        }

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::analyze categorizeArgs"));
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::parenthesizeOriginalFilter`. The following command:
    ///
    /// ```text
    ///    task +home or +work list
    /// ```
    ///
    /// Is reasonable, and does not work unless the filter is parenthesized.
    /// Ignoring context, the 'list' report has a filter, which is inserted at
    /// the beginning like this:
    ///
    /// ```text
    ///   task ( status:pending ) +home or +work list
    /// ```
    ///
    /// Parenthesizing the user-provided (original) filter yields this:
    ///
    /// ```text
    ///   task ( status:pending ) ( +home or +work ) list
    /// ```
    ///
    /// And when the conjunction is added:
    ///
    /// ```text
    ///   task ( status:pending ) and ( +home or +work ) list
    /// ```
    ///
    /// the query is correct.
    fn parenthesize_original_filter(&mut self, ctx: &mut Context) {
        // Locate the first and last ORIGINAL FILTER args.
        let mut first_original_filter = 0usize;
        let mut last_original_filter = 0usize;
        for i in 1..self.args.len() {
            if self.args[i].has_tag("FILTER") && self.args[i].has_tag("ORIGINAL") {
                if first_original_filter == 0 {
                    first_original_filter = i;
                }

                last_original_filter = i;
            }
        }

        // If found, parenthesize the arg list accordingly.
        if first_original_filter != 0 && last_original_filter != 0 {
            let mut reconstructed = Vec::new();
            for (i, a) in self.args.iter().enumerate() {
                if i == first_original_filter {
                    let mut open_paren = A2::new("(", LexerType::op);
                    open_paren.tag("ORIGINAL");
                    open_paren.tag("FILTER");
                    reconstructed.push(open_paren);
                }

                reconstructed.push(a.clone());

                if i == last_original_filter {
                    let mut close_paren = A2::new(")", LexerType::op);
                    close_paren.tag("ORIGINAL");
                    close_paren.tag("FILTER");
                    reconstructed.push(close_paren);
                }
            }

            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::analyze parenthesizeOriginalFilter"));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::findCommand`: scan all arguments and if any are an exact
    /// match for a command name, then tag as CMD. If an argument is an exact
    /// match for an attribute, despite being an inexact match for a command,
    /// then it is not a command. The command's DNA is applied as tags
    /// (`READONLY`, `SHOWSID`, `RUNSGC`, `USESCONTEXT`, `ALLOWSFILTER`,
    /// `ALLOWSMODIFICATIONS`, `ALLOWSMISC`).
    ///
    /// Deviation: C++ dereferences a null `Command*` when a `cmd` entity has
    /// no registered command (a crash; `Context::initialize` registers the
    /// entities from the registry, so it never happens there); here the DNA
    /// tags are skipped.
    fn find_command(&mut self, ctx: &mut Context) -> bool {
        for i in 0..self.args.len() {
            let raw = self.args[i].attribute("raw");

            // If the arg canonicalized to a 'cmd', but is also not an exact match
            // for an 'attribute', proceed. Example:
            //   task project=foo list
            //        ^cmd        ^cmd
            //        ^attribute
            let canonical = if self.exact_match("cmd", &raw) {
                raw
            } else if self.exact_match("attribute", &raw) {
                continue;
            } else if let Some(canonical) = self.canonicalize("cmd", &raw) {
                canonical
            } else {
                continue;
            };

            let a = &mut self.args[i];
            a.set_attribute("canonical", &canonical);
            a.tag("CMD");

            // Apply command DNA as tags.
            if let Some(command) = ctx.commands.get(&canonical).cloned() {
                if command.read_only() {
                    a.tag("READONLY");
                }
                if command.displays_id() {
                    a.tag("SHOWSID");
                }
                if command.needs_gc() {
                    a.tag("RUNSGC");
                }
                if command.uses_context(&ctx.config) {
                    a.tag("USESCONTEXT");
                }
                if command.accepts_filter() {
                    a.tag("ALLOWSFILTER");
                }
                if command.accepts_modifications() {
                    a.tag("ALLOWSMODIFICATIONS");
                }
                if command.accepts_miscellaneous() {
                    a.tag("ALLOWSMISC");
                }
            }

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::analyze findCommand"));
            }

            // Stop and indicate command found.
            return true;
        }

        // Indicate command not found.
        false
    }

    /// C++ `CLI2::exactMatch`: searches for exact `value` in the `category`
    /// entities.
    pub fn exact_match(&self, category: &str, value: &str) -> bool {
        // Extract a list of entities for category.
        self.entities
            .get(category)
            .is_some_and(|names| names.iter().any(|n| n == value))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::desugarFilterTags`:
    ///
    /// ```text
    /// +tag --> tags _hastag_ tag
    /// -tag --> tags _notag_ tag
    /// ```
    fn desugar_filter_tags(&mut self, ctx: &mut Context) {
        let mut changes = false;
        let mut reconstructed = Vec::new();
        for a in &self.args {
            if a.lextype == LexerType::tag && a.has_tag("FILTER") {
                changes = true;

                let mut left = A2::new("tags", LexerType::dom);
                left.tag("FILTER");
                reconstructed.push(left);

                let raw = a.attribute("raw");

                let mut op = A2::new(
                    if raw.starts_with('+') { "_hastag_" } else { "_notag_" },
                    LexerType::op,
                );
                op.tag("FILTER");
                reconstructed.push(op);

                // C++ `raw.substr (1)`; Lexer tags start with an ASCII sign.
                let mut right = A2::new(raw.get(1..).unwrap_or(""), LexerType::string);
                right.tag("FILTER");
                reconstructed.push(right);
            } else {
                reconstructed.push(a.clone());
            }
        }

        if changes {
            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::prepareFilter desugarFilterTags"));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::findStrayModifications`: for `add` and `log`, every
    /// `FILTER` argument is re-tagged `MODIFICATION`.
    fn find_stray_modifications(&mut self, ctx: &mut Context) {
        let mut changes = false;

        let command = self.get_command();
        if command == "add" || command == "log" {
            for a in self.args.iter_mut() {
                if a.has_tag("FILTER") {
                    a.un_tag("FILTER");
                    a.tag("MODIFICATION");
                    changes = true;
                }
            }
        }

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::prepareFilter findStrayModifications"));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::desugarFilterAttributes`:
    ///
    /// ```text
    /// <name>[.<mod>]:['"][<value>]['"] --> name <op> value
    /// ```
    ///
    /// Modifiers map to operators as: (none) `=`, `before`/`under`/`below`
    /// `<`, `after`/`over`/`above` `>`, `by` `<=`, `none` `== ''`, `any`
    /// `!== ''`, `is`/`equals` `==`, `not` `!=`, `isnt` `!==`,
    /// `has`/`contains` `~`, `hasnt` `!~`, `startswith`/`left` `~ ^value`,
    /// `endswith`/`right` `~ value$`, `word` `~ \bvalue\b`, `noword`
    /// `!~ \bvalue\b` (macOS: the bare value; Solaris: `\<value\>`).
    ///
    /// Fails with `Error: unrecognized attribute modifier '<mod>'.` (a C++
    /// `throw std::string`). As in C++, the in-place demotion of earlier
    /// unrecognised pairs to words survives the error; the reconstruction
    /// does not.
    ///
    /// Deviations:
    /// - C++ `Context::getContext ().columns[canonical]` autovivifies a null
    ///   entry for a name without a column; the Rust registry only holds
    ///   columns, so it is looked up without insertion.
    /// - C++ reads `values[0]` even when the value lexes to no tokens (e.g.
    ///   whitespace only; undefined behaviour); here the type is then
    ///   `Lexer::Type::string`.
    fn desugar_filter_attributes(&mut self, ctx: &mut Context) -> Result<()> {
        let mut changes = false;
        let mut reconstructed = Vec::new();
        for i in 0..self.args.len() {
            if self.args[i].lextype == LexerType::pair && self.args[i].has_tag("FILTER") {
                let a = &self.args[i];
                let name = a.attribute("name");
                let mod_ = a.attribute("modifier");
                let mut value = a.attribute("value");

                // An unquoted string, while equivalent to an empty string, doesn't cause
                // an operand shortage in eval.
                if value.is_empty() {
                    value = "''".to_string();
                }

                // Some values are expressions, which need to be lexed. The best way to
                // determine whether an expression is either a single value, or needs to
                // be lexed, is to lex it and count the tokens. For example:
                //    now+1d
                // This should be lexed and surrounded by parentheses:
                //    (
                //    now
                //    +
                //    1d
                //    )
                // Use this sequence in place of a single value.
                let values = self.lex_expression(&value);
                if debug_parser(ctx) {
                    ctx.debug(&format!("CLI2::lexExpression {name}:{value}"));
                    for v in &values {
                        ctx.debug(&format!("  {}", v.dump()));
                    }
                    ctx.debug(" ");
                }

                let mut found = false;
                if let Some(canonical) = self.canonicalize("attribute", &name) {
                    // Certain attribute types do not suport math.
                    //   string   --> no
                    //   numeric  --> yes
                    //   date     --> yes
                    //   duration --> yes
                    let mut eval_supported = true;
                    if ctx.columns.get(&canonical).is_some_and(|col| col.type_() == "string") {
                        eval_supported = false;
                    }

                    let mut lhs = A2::new(&name, LexerType::dom);
                    lhs.tag("FILTER");
                    lhs.set_attribute("canonical", &canonical);
                    lhs.set_attribute("modifier", &mod_);

                    let mut op = A2::new("", LexerType::op);
                    op.tag("FILTER");

                    // Attribute types that do not support evaluation should be interpreted
                    // as strings (currently this means that string attributes are not evaluated)
                    let rhs_type = if eval_supported {
                        values.first().map_or(LexerType::string, |v| v.lextype)
                    } else {
                        LexerType::string
                    };
                    // C++ `A2 rhs ("", Lexer::Type::tag)` decomposes the empty
                    // raw value, and `raw.substr (1)` throws
                    // `std::out_of_range`, which only `catch (...)` handles
                    // ("Unknown error. Please report."). This happens when an
                    // eval-supporting attribute's value lexes to a tag first,
                    // e.g. `due:+foo`.
                    if rhs_type == LexerType::tag {
                        return Err(TwError::Runtime("basic_string::substr".to_string()));
                    }
                    let mut rhs = A2::new("", rhs_type);
                    rhs.tag("FILTER");

                    // Special case for '<name>:<value>'.
                    let (op_raw, rhs_raw) = match mod_.as_str() {
                        "" => ("=", value.clone()),
                        "before" | "under" | "below" => ("<", value.clone()),
                        "after" | "over" | "above" => (">", value.clone()),
                        "by" => ("<=", value.clone()),
                        "none" => ("==", "''".to_string()),
                        "any" => ("!==", "''".to_string()),
                        "is" | "equals" => ("==", value.clone()),
                        "not" => ("!=", value.clone()),
                        "isnt" => ("!==", value.clone()),
                        "has" | "contains" => ("~", value.clone()),
                        "hasnt" => ("!~", value.clone()),
                        "startswith" | "left" => ("~", format!("^{value}")),
                        "endswith" | "right" => ("~", format!("{value}$")),
                        "word" => ("~", word_boundary(&value)),
                        "noword" => ("!~", word_boundary(&value)),
                        _ => {
                            return Err(TwError::Message(format(
                                "Error: unrecognized attribute modifier '{1}'.",
                                &[&mod_],
                            )));
                        }
                    };
                    op.set_attribute("raw", op_raw);
                    rhs.set_attribute("raw", &rhs_raw);

                    reconstructed.push(lhs);
                    reconstructed.push(op);

                    // Do not modify this construct without full understanding.
                    // Getting this wrong breaks a whole lot of filtering tests.
                    if eval_supported {
                        reconstructed.extend(values);
                    } else if Lexer::is_dom_str(&rhs.attribute("raw")) {
                        rhs.lextype = LexerType::dom;
                        reconstructed.push(rhs);
                    } else {
                        reconstructed.push(rhs);
                    }

                    found = true;
                }
                // If the name does not canonicalize to either an attribute or a UDA
                // then it is not a recognized Lexer::Type::pair, so downgrade it to
                // Lexer::Type::word.
                else {
                    self.args[i].lextype = LexerType::word;
                }

                if found {
                    changes = true;
                } else {
                    reconstructed.push(self.args[i].clone());
                }
            }
            // Not a FILTER pair.
            else {
                reconstructed.push(self.args[i].clone());
            }
        }

        if changes {
            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::prepareFilter desugarFilterAttributes"));
            }
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::desugarFilterPatterns`:
    ///
    /// ```text
    /// /pattern/ --> description ~ 'pattern'
    /// ```
    fn desugar_filter_patterns(&mut self, ctx: &mut Context) {
        let mut changes = false;
        let mut reconstructed = Vec::new();
        for a in &self.args {
            if a.lextype == LexerType::pattern && a.has_tag("FILTER") {
                changes = true;

                let mut lhs = A2::new("description", LexerType::dom);
                lhs.tag("FILTER");
                reconstructed.push(lhs);

                let mut op = A2::new("~", LexerType::op);
                op.tag("FILTER");
                reconstructed.push(op);

                let mut rhs = A2::new(&a.attribute("pattern"), LexerType::string);
                rhs.set_attribute("flags", &a.attribute("flags"));
                rhs.tag("FILTER");
                reconstructed.push(rhs);
            } else {
                reconstructed.push(a.clone());
            }
        }

        if changes {
            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::prepareFilter desugarFilterPatterns"));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::findIDs`. An ID sequence can be:
    ///
    /// ```text
    ///   a single ID:          1
    ///   a list of IDs:        1,3,5
    ///   a list of IDs:        1 3 5
    ///   a range:              5-10
    ///   or a combination:     1,3,5-10 12
    /// ```
    ///
    /// With `rc.sugar` the ranges are collected into `id_ranges` (a number
    /// preceded by an operator other than `(`/`)` is skipped; when there is
    /// no FILTER argument at all, plain-integer numbers and sets tagged
    /// `MODIFICATION` become FILTER IDs, except for `add`/`log`). Without
    /// sugar, every FILTER number becomes an `id:<n>` pair.
    fn find_ids(&mut self, ctx: &mut Context) {
        let mut changes = false;

        if ctx.config.get_boolean("sugar") {
            let mut previous_filter_arg_was_an_operator = false;
            let mut filter_count = 0;

            for a in &self.args {
                if a.has_tag("FILTER") {
                    filter_count += 1;

                    if a.lextype == LexerType::number {
                        // Skip any number that was preceded by an operator.
                        if !previous_filter_arg_was_an_operator {
                            changes = true;
                            let number = a.attribute("raw");
                            self.id_ranges.push((number.clone(), number));
                        }
                    } else if a.lextype == LexerType::set {
                        // Split the ID list into elements.
                        let elements = split(&a.attribute("raw"), ',');

                        for element in &elements {
                            changes = true;
                            push_id_range(&mut self.id_ranges, element);
                        }
                    }

                    let raw = a.attribute("raw");
                    previous_filter_arg_was_an_operator = a.lextype == LexerType::op && raw != "(" && raw != ")";
                }
            }

            // If no IDs were found, and no filter was specified, look for number/set
            // listed as a MODIFICATION.
            let command = self.get_command();

            if self.id_ranges.is_empty() && filter_count == 0 && command != "add" && command != "log" {
                for a in self.args.iter_mut() {
                    if a.has_tag("MODIFICATION") {
                        let raw = a.attribute("raw");

                        // For a number to be an ID, it must not contain any sign or floating
                        // point elements.
                        if a.lextype == LexerType::number && !raw.contains('.') && !raw.contains('e') && !raw.contains('-')
                        {
                            changes = true;
                            a.un_tag("MODIFICATION");
                            a.tag("FILTER");
                            self.id_ranges.push((raw.clone(), raw));
                        } else if a.lextype == LexerType::set {
                            a.un_tag("MODIFICATION");
                            a.tag("FILTER");

                            // Split the ID list into elements.
                            let elements = split(&raw, ',');

                            for element in &elements {
                                changes = true;
                                push_id_range(&mut self.id_ranges, element);
                            }
                        }
                    }
                }
            }
        }
        // Sugar-free.
        else {
            let mut reconstructed = Vec::new();
            for a in &self.args {
                if a.has_tag("FILTER") && a.lextype == LexerType::number {
                    changes = true;
                    let mut pair = A2::new(&format!("id:{}", a.attribute("raw")), LexerType::pair);
                    pair.tag("FILTER");
                    pair.decompose();
                    reconstructed.push(pair);
                } else {
                    reconstructed.push(a.clone());
                }
            }

            if changes {
                self.args = reconstructed;
            }
        }

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::prepareFilter findIDs"));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::findUUIDs`: with `rc.sugar`, collects FILTER UUIDs into
    /// `uuid_list` (or, when there are none, the `MODIFICATION` ones, which
    /// become FILTER). Without sugar, every FILTER UUID becomes a
    /// `uuid:<uuid>` pair.
    fn find_uuids(&mut self, ctx: &mut Context) {
        let mut changes = false;

        if ctx.config.get_boolean("sugar") {
            for a in &self.args {
                if a.lextype == LexerType::uuid && a.has_tag("FILTER") {
                    changes = true;
                    self.uuid_list.push(a.attribute("raw"));
                }
            }

            if self.uuid_list.is_empty() {
                for a in self.args.iter_mut() {
                    if a.lextype == LexerType::uuid && a.has_tag("MODIFICATION") {
                        changes = true;
                        a.un_tag("MODIFICATION");
                        a.tag("FILTER");
                        self.uuid_list.push(a.attribute("raw"));
                    }
                }
            }
        }
        // Sugar-free.
        else {
            let mut reconstructed = Vec::new();
            for a in &self.args {
                if a.has_tag("FILTER") && a.lextype == LexerType::uuid {
                    changes = true;
                    let mut pair = A2::new(&format!("uuid:{}", a.attribute("raw")), LexerType::pair);
                    pair.tag("FILTER");
                    pair.decompose();
                    reconstructed.push(pair);
                } else {
                    reconstructed.push(a.clone());
                }
            }

            if changes {
                self.args = reconstructed;
            }
        }

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::prepareFilter findUUIDs"));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::insertIDExpr`: replaces the *first* FILTER set/number/uuid
    /// argument with one expression covering all of `id_ranges` and
    /// `uuid_list`; all other such arguments are eaten.
    fn insert_id_expr(&mut self, ctx: &mut Context) {
        // Skip completely if no ID/UUID was found. This is because below, '(' and ')'
        // are inserted regardless of list size.
        if self.id_ranges.is_empty() && self.uuid_list.is_empty() {
            return;
        }

        // Find the *first* occurence of lexer type set/number/uuid, and replace it
        // with a synthesized expression. All other occurences are eaten.
        let mut changes = false;
        let mut found_id = false;
        let mut reconstructed = Vec::new();
        for a in &self.args {
            if matches!(a.lextype, LexerType::set | LexerType::number | LexerType::uuid) && a.has_tag("FILTER") {
                if !found_id {
                    found_id = true;
                    changes = true;

                    // Construct a single sequence that represents all _id_ranges and
                    // _uuid_list in one clause. This is essentially converting this:
                    //
                    //   1,2-3 uuid,uuid uuid 4
                    //
                    // into:
                    //
                    //   (
                    //        ( id == 1 )
                    //     or ( ( id >= 2 ) and ( id <= 3 ) )
                    //     or ( id == 4 )
                    //     or ( uuid = $UUID )
                    //     or ( uuid = $UUID )
                    //   )

                    // Building block operators.
                    let filter_arg = |raw: &str, lextype: LexerType| {
                        let mut a = A2::new(raw, lextype);
                        a.tag("FILTER");
                        a
                    };
                    let open_paren = filter_arg("(", LexerType::op);
                    let close_paren = filter_arg(")", LexerType::op);
                    let op_or = filter_arg("or", LexerType::op);
                    let op_and = filter_arg("and", LexerType::op);
                    let op_similar = filter_arg("=", LexerType::op);
                    let op_equal = filter_arg("==", LexerType::op);
                    let op_gte = filter_arg(">=", LexerType::op);
                    let op_lte = filter_arg("<=", LexerType::op);

                    // Building block attributes.
                    let arg_id = filter_arg("id", LexerType::dom);
                    let arg_uuid = filter_arg("uuid", LexerType::dom);

                    reconstructed.push(open_paren.clone());

                    // Add all ID ranges.
                    for (r, (first, second)) in self.id_ranges.iter().enumerate() {
                        if r != 0 {
                            reconstructed.push(op_or.clone());
                        }

                        if first == second {
                            reconstructed.push(open_paren.clone());
                            reconstructed.push(arg_id.clone());
                            reconstructed.push(op_equal.clone());

                            reconstructed.push(filter_arg(first, LexerType::number));

                            reconstructed.push(close_paren.clone());
                        } else {
                            let low = strtol(first);
                            let high = strtol(second);
                            let ascending = low <= high;

                            reconstructed.push(open_paren.clone());
                            reconstructed.push(arg_id.clone());
                            reconstructed.push(op_gte.clone());

                            reconstructed.push(filter_arg(if ascending { first } else { second }, LexerType::number));

                            reconstructed.push(op_and.clone());
                            reconstructed.push(arg_id.clone());
                            reconstructed.push(op_lte.clone());

                            reconstructed.push(filter_arg(if ascending { second } else { first }, LexerType::number));

                            reconstructed.push(close_paren.clone());
                        }
                    }

                    // Combine the ID and UUID sections with 'or'.
                    if !self.id_ranges.is_empty() && !self.uuid_list.is_empty() {
                        reconstructed.push(op_or.clone());
                    }

                    // Add all UUID list items.
                    for (u, uuid) in self.uuid_list.iter().enumerate() {
                        if u != 0 {
                            reconstructed.push(op_or.clone());
                        }

                        reconstructed.push(open_paren.clone());
                        reconstructed.push(arg_uuid.clone());
                        reconstructed.push(op_similar.clone());

                        reconstructed.push(filter_arg(uuid, LexerType::string));

                        reconstructed.push(close_paren.clone());
                    }

                    reconstructed.push(close_paren);
                }

                // No 'else' because all set/number/uuid args but the first are removed.
            } else {
                reconstructed.push(a.clone());
            }
        }

        if changes {
            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::prepareFilter insertIDExpr"));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::lexFilterArgs`: FILTER `Lexer::Type::word` args will
    /// become part of an expression, and so they need to be Lexed.
    fn lex_filter_args(&mut self, ctx: &mut Context) {
        let mut changes = false;
        let mut reconstructed = Vec::new();
        for a in &self.args {
            if a.lextype == LexerType::word && a.has_tag("FILTER") {
                changes = true;

                let mut lexeme = String::new();
                let mut type_ = LexerType::word;
                let mut lex = Lexer::new(&a.attribute("raw"));
                while lex.token(&mut lexeme, &mut type_) {
                    let mut extra = A2::new(&lexeme, type_);
                    extra.tag("FILTER");
                    reconstructed.push(extra);
                }
            } else {
                reconstructed.push(a.clone());
            }
        }

        if changes {
            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::prepareFilter lexFilterArgs"));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::isEmptyParenExpression (iterator it, bool forward = true)`:
    /// detects if the bracket at `index` is a start or end of an empty paren
    /// expression. Examples:
    ///
    /// ```text
    /// ( status = pending ) ( )
    ///                      ^
    ///              it -----|         => true
    ///
    /// ( status = pending ) ( project = Home )
    ///                      ^
    ///              it -----|         => false
    /// ```
    ///
    /// As in C++, the backward scan stops before reaching `args[0]`.
    fn is_empty_paren_expression(&self, index: usize, forward: bool) -> bool {
        let mut open = 0;
        let mut closed = 0;

        let mut a = index;
        while if forward { a < self.args.len() } else { a != 0 } {
            let raw = self.args[a].attribute("raw");
            if raw == "(" {
                open += 1;
            } else if raw == ")" {
                closed += 1;
            } else {
                // Encountering a non-paren token means there is something between parenthees
                return false;
            }

            // Getting balanced parentheses means we have an empty paren expression
            if open == closed && open != 0 {
                return true;
            }

            if forward {
                a += 1;
            } else {
                a -= 1;
            }
        }

        // Should not end here.
        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::desugarFilterPlainArgs`: FILTER, `Lexer::Type::word` args
    /// are treated as search terms.
    ///
    /// Algorithm:
    ///
    /// ```text
    ///   Given:
    ///     - task ... argX candidate argY
    ///   Where:
    ///     - neither argX nor argY are an operator, except (, ), and, or, xor
    ///     - candidate is one of: Lexer::Type::word
    ///                            Lexer::Type::identifier
    ///                            Lexer::Type::date
    /// ```
    ///
    /// Each candidate becomes `description ~ <dequoted word>` (all three
    /// tagged `FILTER` and `PLAIN`). As in C++, a *last* argument that is a
    /// date is not a candidate.
    ///
    /// Deviation: C++ indexes `_args[0]` and `_args[size - 2]`
    /// unconditionally (undefined behaviour with fewer than two arguments,
    /// which cannot happen after `analyze`); here such lists are left alone.
    fn desugar_filter_plain_args(&mut self, ctx: &mut Context) {
        if self.args.len() < 2 {
            return;
        }

        let is_junction = |raw: &str| matches!(raw, "(" | ")" | "and" | "or" | "xor");

        // First walk the arg list looking for plain words that are not part of an
        // existing expression.
        let mut prevprev = 0usize;
        let mut prev = 0usize;
        for i in 0..self.args.len() {
            let a = &self.args[i];
            let raw = a.attribute("raw");
            let ppraw = self.args[prevprev].attribute("raw");

            let p = &self.args[prev];
            if (self.args[prevprev].lextype != LexerType::op || is_junction(&ppraw)) // argX
                && matches!(p.lextype, LexerType::identifier | LexerType::date | LexerType::word) // candidate
                && p.has_tag("FILTER") // candidate
                && (a.lextype != LexerType::op || is_junction(&raw))
            // argY
            {
                self.args[prev].tag("PLAIN");
            }

            prevprev = prev;
            prev = i;
        }

        // Cover the case where the *last* argument is a plain arg.
        let len = self.args.len();
        let penultimate = &self.args[len - 2];
        let praw = penultimate.attribute("raw");
        let last = &self.args[len - 1];
        if (penultimate.lextype != LexerType::op || is_junction(&praw)) // argX
            && matches!(last.lextype, LexerType::identifier | LexerType::word) // candidate
            && last.has_tag("FILTER")
        // candidate
        {
            self.args[len - 1].tag("PLAIN");
        }

        // Walk the list again, upgrading PLAIN args.
        let mut changes = false;
        let mut reconstructed = Vec::new();
        for a in &self.args {
            if a.has_tag("PLAIN") {
                changes = true;

                let mut lhs = A2::new("description", LexerType::dom);
                lhs.set_attribute("canonical", "description");
                lhs.tag("FILTER");
                lhs.tag("PLAIN");
                reconstructed.push(lhs);

                let mut op = A2::new("~", LexerType::op);
                op.tag("FILTER");
                op.tag("PLAIN");
                reconstructed.push(op);

                let mut word = a.attribute("raw");
                Lexer::dequote(&mut word);
                let mut rhs = A2::new(&word, LexerType::string);
                rhs.tag("FILTER");
                rhs.tag("PLAIN");
                reconstructed.push(rhs);
            } else {
                reconstructed.push(a.clone());
            }
        }

        if changes {
            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::prepareFilter desugarFilterPlainArgs"));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::insertJunctions`: two consecutive FILTER, non-OP arguments
    /// that are not "(" or ")" need an "and" operator inserted between them.
    ///
    /// ```text
    ///   ) <non-op>         -->  ) and <non-op>
    ///   <non-op> (         -->  <non-op> <and> (
    ///   ) (                -->  ) and (
    ///   <non-op> <non-op>  -->  <non-op> and <non-op>
    /// ```
    ///
    /// Empty paren expressions (`( )`) do not get a junction.
    fn insert_junctions(&mut self, ctx: &mut Context) {
        let mut changes = false;
        let mut reconstructed = Vec::new();
        // C++ `auto prev = _args.begin ()`: index 0 means "no FILTER arg yet".
        let mut prev = 0usize;

        for a in 0..self.args.len() {
            if self.args[a].has_tag("FILTER") {
                // The prev iterator should be the first FILTER arg. (C++
                // `if (prev == _args.begin ()) prev = a;`, which the
                // assignment below repeats.)
                //
                // Insert AND between terms.
                if prev != 0 && a != prev {
                    let p = &self.args[prev];
                    let c = &self.args[a];
                    let p_raw = p.attribute("raw");
                    let c_raw = c.attribute("raw");
                    if (p.lextype != LexerType::op && c_raw == "(" && !self.is_empty_paren_expression(a, true))
                        || (p_raw == ")" && c.lextype != LexerType::op && !self.is_empty_paren_expression(prev, false))
                        || (p_raw == ")"
                            && c_raw == "("
                            && !self.is_empty_paren_expression(a, true)
                            && !self.is_empty_paren_expression(prev, false))
                        || (p.lextype != LexerType::op && c.lextype != LexerType::op)
                    {
                        let mut op_or = A2::new("and", LexerType::op);
                        op_or.tag("FILTER");
                        reconstructed.push(op_or);
                        changes = true;
                    }
                }

                // Previous FILTER arg.
                prev = a;
            }

            reconstructed.push(self.args[a].clone());
        }

        if changes {
            self.args = reconstructed;

            if debug_parser(ctx) {
                ctx.debug(&self.dump("CLI2::prepareFilter insertJunctions"));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::lexExpression`: lexes an attribute value into FILTER
    /// tokens; if there were multiple tokens, they are parenthesized,
    /// because the expression will be used as a value:
    ///
    /// ```text
    ///    now+1d --> ( now + 1d )
    /// ```
    fn lex_expression(&self, expression: &str) -> Vec<A2> {
        let mut lexed = Vec::new();
        let mut lexeme = String::new();
        let mut type_ = LexerType::word;
        let mut lex = Lexer::new(expression);
        while lex.token(&mut lexeme, &mut type_) {
            let mut token = A2::new(&lexeme, type_);
            token.tag("FILTER");
            lexed.push(token);
        }

        // If there were multiple tokens, parenthesize, because this expression will
        // be used as a value.
        if lexed.len() > 1 {
            let mut open_paren = A2::new("(", LexerType::op);
            open_paren.tag("FILTER");
            let mut close_paren = A2::new(")", LexerType::op);
            close_paren.tag("FILTER");

            lexed.insert(0, open_paren);
            lexed.push(close_paren);
        }

        lexed
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CLI2::defaultCommand`: look for situations that require defaults:
    ///
    /// 1. If no command was found, and no ID/UUID, and if rc.default.command
    ///    is configured, inject the lexed tokens from rc.default.command
    ///    (tagged `DEFAULT` in `_args`) after arg0.
    ///
    /// 2. If no command was found, but an ID/UUID (a `Lexer::Type::number` or
    ///    `uuid` argument) was found, then assume a command of 'information'
    ///    (tagged `ASSUMED`).
    fn default_command(&mut self, ctx: &mut Context) {
        // Scan the top-level branches for evidence of ID, UUID, overrides and other
        // arguments.
        let mut changes = false;
        let mut found_command = false;
        let mut found_sequence = false;

        for a in &self.args {
            if a.has_tag("CMD") {
                found_command = true;
            }

            if a.lextype == LexerType::uuid || a.lextype == LexerType::number {
                found_sequence = true;
            }
        }

        // If no command was specified, then a command will be inserted.
        if !found_command {
            // Default command.
            if !found_sequence {
                // Apply overrides, if any.
                let default_command = ctx.config.get("default.command");
                // Deviation: C++ indexes `_original_args[0]` / `_args[0]`
                // unconditionally; here an empty list is left alone.
                if !default_command.is_empty() && !self.args.is_empty() && !self.original_args.is_empty() {
                    // Modify _args, _original_args to be:
                    //   <args0> [<def0> ...] <args1> [...]

                    let mut reconstructed_originals = vec![self.original_args[0].clone()];
                    let mut reconstructed = vec![self.args[0].clone()];

                    let mut lexeme = String::new();
                    let mut type_ = LexerType::word;
                    let mut lex = Lexer::new(&default_command);

                    while lex.token(&mut lexeme, &mut type_) {
                        reconstructed_originals.push(A2::new(&lexeme, type_));

                        let mut cmd = A2::new(&lexeme, type_);
                        cmd.tag("DEFAULT");
                        reconstructed.push(cmd);
                    }

                    reconstructed_originals.extend(self.original_args.iter().skip(1).cloned());
                    reconstructed.extend(self.args.iter().skip(1).cloned());

                    self.original_args = reconstructed_originals;
                    self.args = reconstructed;
                    changes = true;
                }
            } else {
                let mut info = A2::new("information", LexerType::word);
                info.tag("ASSUMED");
                self.args.push(info);
                changes = true;
            }
        }

        if changes && debug_parser(ctx) {
            ctx.debug(&self.dump("CLI2::analyze defaultCommand"));
        }
    }
}

/// The C++ alias-expansion idiom `Lexer lex (value); while (lex.token
/// (lexeme, type)) list.emplace_back (lexeme, type);`.
fn lex_into(value: &str, list: &mut Vec<A2>) {
    let mut lexeme = String::new();
    let mut type_ = LexerType::word;
    let mut lex = Lexer::new(value);
    while lex.token(&mut lexeme, &mut type_) {
        list.push(A2::new(&lexeme, type_));
    }
}

/// The C++ `findIDs` element idiom: `element` (from a split ID set) with a
/// `-` becomes the range `(before, after)` of the first hyphen, otherwise
/// the single ID `(element, element)`.
fn push_id_range(id_ranges: &mut Vec<(String, String)>, element: &str) {
    match element.find('-') {
        Some(hyphen) => id_ranges.push((element[..hyphen].to_string(), element[hyphen + 1..].to_string())),
        None => id_ranges.push((element.to_string(), element.to_string())),
    }
}

/// The `word`/`noword` modifier value in `desugarFilterAttributes`: the C++
/// `#if defined(DARWIN)` bare value, `#elif defined(SOLARIS)` `\<value\>`,
/// else `\bvalue\b`.
fn word_boundary(value: &str) -> String {
    if cfg!(target_os = "macos") {
        value.to_string()
    } else if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        format!("\\<{value}\\>")
    } else {
        format!("\\b{value}\\b")
    }
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

    use crate::commands::command::factory;

    /// A Context with the real command registry (`Command::factory`) and its
    /// `cmd` entities, a few `attribute` entities and the `limit` pseudo, as
    /// `Context::initialize` steps [3]-[4] set them up.
    fn context() -> Context {
        let mut ctx = Context::new();
        factory(&mut ctx.commands, &ctx.config).unwrap();
        let keywords: Vec<String> = ctx.commands.keys().cloned().collect();
        for keyword in keywords {
            ctx.cli2.entity("cmd", &keyword);
        }
        for name in ["description", "due", "project", "status", "tags"] {
            ctx.cli2.entity("attribute", name);
        }
        ctx.cli2.entity("pseudo", "limit");
        ctx
    }

    /// `task <list...>` analysed on `ctx` (fresh arguments each time).
    fn parse(ctx: &mut Context, list: &[&str]) -> Result<()> {
        ctx.cli2.original_args.clear();
        ctx.cli2.args.clear();
        ctx.cli2.command.clear();
        ctx.cli2.context_added = false;
        for a in list {
            ctx.cli2.add(a);
        }
        with_cli2(ctx, |cli2, ctx| cli2.analyze(ctx))
    }

    fn raws(args: &[A2]) -> Vec<String> {
        args.iter().map(|a| a.attribute("raw")).collect()
    }

    fn find<'a>(ctx: &'a Context, raw: &str) -> &'a A2 {
        ctx.cli2.args.iter().find(|a| a.attribute("raw") == raw).expect(raw)
    }

    #[test]
    fn a2_decompose_tags_and_token() {
        let tag = A2::new("+home", LexerType::tag);
        assert_eq!(tag.attribute("name"), "home");
        assert_eq!(tag.attribute("sign"), "+");

        let rc = A2::new("rc:/x/rc", LexerType::pair);
        assert!(rc.has_tag("RC") && !rc.has_tag("CONFIG"));
        let config = A2::new("rc.verbose=nothing", LexerType::pair);
        assert!(config.has_tag("CONFIG") && !config.has_tag("RC"));
        assert_eq!(config.attribute("modifier"), "verbose");
        assert_eq!(config.attribute("value"), "nothing");

        let pair = A2::new("pro:home", LexerType::pair);
        assert_eq!(
            (pair.attribute("name"), pair.attribute("separator"), pair.attribute("value")),
            ("pro".to_string(), ":".to_string(), "home".to_string())
        );

        let sub = A2::new("/a/b/g", LexerType::substitution);
        assert_eq!((sub.attribute("from"), sub.attribute("to"), sub.attribute("flags")), ("a".into(), "b".into(), "g".into()));
        let pat = A2::new("/x/", LexerType::pattern);
        assert_eq!(pat.attribute("pattern"), "x");

        // A word is not decomposed; tags are ordered and unique.
        let mut word = A2::new("+home", LexerType::word);
        assert_eq!(word.attribute("name"), "");
        word.tag("B");
        word.tag("A");
        word.tag("B");
        assert_eq!(word.tags, ["B", "A"]);
        word.un_tag("B");
        assert_eq!(word.tags, ["A"]);
        word.un_tag("missing");

        assert_eq!(word.get_token(), "+home");
        word.set_attribute("canonical", "canon");
        assert_eq!(word.get_token(), "canon");
    }

    #[test]
    fn a2_dump_format() {
        let mut a = A2::new("+x", LexerType::tag);
        a.tag("FILTER");
        a.tag("ORIGINAL");
        assert_eq!(
            a.dump(),
            "\x1b[37;45mtag\x1b[0m name='\x1b[33mx\x1b[0m' raw='\x1b[33m+x\x1b[0m' sign='\x1b[33m+\x1b[0m' \
             \x1b[1;37;42mFILTER\x1b[0m \x1b[32mORIGINAL\x1b[0m "
        );

        let mut b = A2::new("w", LexerType::word);
        for tag in ["BINARY", "CMD", "MODIFICATION", "MISCELLANEOUS", "RC", "CONFIG", "?"] {
            b.tag(tag);
        }
        assert_eq!(
            b.dump(),
            "\x1b[38;5;15m\x1b[48;5;236mword\x1b[0m raw='\x1b[33mw\x1b[0m' \x1b[1;37;44mBINARY\x1b[0m \
             \x1b[1;37;46mCMD\x1b[0m \x1b[1;37;43mMODIFICATION\x1b[0m \x1b[1;37;45mMISCELLANEOUS\x1b[0m \
             \x1b[1;37;41mRC\x1b[0m \x1b[1;37;101mCONFIG\x1b[0m \x1b[38;5;255;48;5;232m?\x1b[0m "
        );
    }

    #[test]
    fn cli2_dump_format() {
        let mut cli = Cli2::new();
        cli.add("task");
        cli.original_args.push(A2::new("inserted", LexerType::word));
        let gray = Color::parse("gray10 on gray4").unwrap();
        let filter = Color::parse("black on rgb311").unwrap();
        assert_eq!(
            cli.dump("T"),
            format!("\x1b[1mT\x1b[0m\n  _original_args\n    {} {}\n", gray.colorize("task"), filter.colorize("inserted"))
        );

        cli.args.push(A2::new("task", LexerType::word));
        cli.id_ranges = vec![("1".into(), "3".into()), ("5".into(), "5".into())];
        cli.uuid_list = vec!["abc".into()];
        assert_eq!(
            cli.dump("T"),
            format!(
                "\x1b[1mT\x1b[0m\n  _original_args\n    {} {}\n  _args\n    {}\n  _id_ranges\n    {} {} \n  _uuid_list\n    {} \n",
                gray.colorize("task"),
                filter.colorize("inserted"),
                cli.args[0].dump(),
                gray.colorize("1-3"),
                gray.colorize("5"),
                gray.colorize("abc"),
            )
        );
    }

    #[test]
    fn add_args_inserts_after_offset() {
        let mut cli = Cli2::new();
        for a in ["task", "a", "b"] {
            cli.add(a);
        }
        cli.add_args(&["x".to_string(), "y".to_string()], 0);
        assert_eq!(raws(&cli.original_args), ["task", "x", "y", "a", "b"]);
        assert!(!cli.original_args[1].has_tag("ORIGINAL"));
        assert!(cli.original_args[3].has_tag("ORIGINAL"));
        cli.add_args(&["z".to_string()], 4);
        assert_eq!(raws(&cli.original_args), ["task", "x", "y", "a", "b", "z"]);
        cli.add_args(&["end".to_string()], 99);
        assert_eq!(raws(&cli.original_args).last().unwrap(), "end");
    }

    #[test]
    fn lexing_types_and_quoting() {
        let mut ctx = context();
        parse(&mut ctx, &["task", "add", "foo bar", "don't stop", "it's", "pro:home", "+tag", "rc.x=1", "12"]).unwrap();
        assert_eq!(ctx.cli2.get_command(), "add");

        let binary = &ctx.cli2.args[0];
        assert!(binary.has_tag("BINARY"));
        assert_eq!(binary.attribute("basename"), "task");

        // Multi-token arguments become one QUOTED word.
        let quoted = find(&ctx, "foo bar");
        assert_eq!(quoted.lextype, LexerType::word);
        assert!(quoted.has_tag("QUOTED") && quoted.has_tag("ORIGINAL"));
        // Unescaped single quotes are escaped, then dequoted.
        let apostrophe = find(&ctx, "don't stop");
        assert_eq!(apostrophe.lextype, LexerType::word);
        assert!(apostrophe.has_tag("QUOTED"));
        // A single token is kept with its Lexer type.
        assert_eq!(find(&ctx, "it's").lextype, LexerType::identifier);

        assert_eq!(find(&ctx, "pro:home").lextype, LexerType::pair);
        assert_eq!(find(&ctx, "pro:home").attribute("canonical"), "project");
        assert_eq!(find(&ctx, "+tag").lextype, LexerType::tag);
        assert!(find(&ctx, "rc.x=1").has_tag("CONFIG"));
        assert_eq!(find(&ctx, "12").lextype, LexerType::number);

        // 'add' takes modifications only: everything but admin args.
        for raw in ["foo bar", "don't stop", "it's", "pro:home", "+tag", "12"] {
            assert!(find(&ctx, raw).has_tag("MODIFICATION"), "{raw}");
        }
        assert!(!find(&ctx, "rc.x=1").has_tag("MODIFICATION"));

        // Command DNA.
        let cmd = find(&ctx, "add");
        for tag in ["CMD", "USESCONTEXT", "ALLOWSMODIFICATIONS"] {
            assert!(cmd.has_tag(tag), "{tag}");
        }
        for tag in ["READONLY", "SHOWSID", "RUNSGC", "ALLOWSFILTER", "ALLOWSMISC"] {
            assert!(!cmd.has_tag(tag), "{tag}");
        }

        // Quoted pairs go to EOS.
        parse(&mut ctx, &["task", "add", "pro:a b"]).unwrap();
        let pair = find(&ctx, "pro:a b");
        assert_eq!(pair.lextype, LexerType::pair);
        assert!(pair.has_tag("QUOTED"));
        assert_eq!(pair.attribute("value"), "a b");
    }

    #[test]
    fn terminator_and_terminated_args() {
        let mut ctx = context();
        ctx.cli2.alias("x", "list");
        parse(&mut ctx, &["task", "add", "--", "+tag", "x", "pro:a", "--"]).unwrap();
        assert_eq!(ctx.cli2.get_command(), "add");
        let args = &ctx.cli2.args;
        assert_eq!(raws(args), ["task", "add", "--", "+tag", "x", "pro:a", "--"]);
        assert_eq!(args[2].lextype, LexerType::separator);
        assert!(args[2].has_tag("TERMINATED"));
        assert!(!args[2].has_tag("MODIFICATION"), "the separator is not categorised");
        for a in &args[3..] {
            assert_eq!(a.lextype, LexerType::word, "{}", a.attribute("raw"));
            assert!(a.has_tag("TERMINATED") && a.has_tag("MODIFICATION"));
        }
    }

    #[test]
    fn alias_expansion_lexes_and_nests() {
        let mut ctx = context();
        ctx.cli2.alias("ls", "count rc.x=1");
        ctx.cli2.alias("ll", "ls +tag");
        parse(&mut ctx, &["task", "ll", "pro:a"]).unwrap();
        assert_eq!(ctx.cli2.get_command(), "count");
        // Only the ORIGINAL filter argument is parenthesized.
        assert_eq!(raws(&ctx.cli2.args), ["task", "count", "rc.x=1", "+tag", "(", "pro:a", ")"]);
        assert!(find(&ctx, "+tag").has_tag("FILTER"));
        assert!(find(&ctx, "rc.x=1").has_tag("CONFIG"));
        assert_eq!(find(&ctx, "+tag").lextype, LexerType::tag);
        assert!(!find(&ctx, "+tag").has_tag("ORIGINAL"), "alias tokens are not ORIGINAL");
        assert_eq!(raws(&ctx.cli2.original_args), ["task", "count", "rc.x=1", "+tag", "pro:a"]);
        // 'count' uses context; none is set.
        assert_eq!(ctx.debug_messages, ["No context set"]);

        // Recursive aliases hit the safety valve.
        let mut ctx = context();
        ctx.cli2.alias("a", "a");
        parse(&mut ctx, &["task", "a", "count"]).unwrap();
        assert_eq!(ctx.debug_messages, ["Nested alias limit of 10 reached.", "No context set"]);

        // Original args after '--' are not expanded.
        let mut ctx = context();
        ctx.cli2.alias("x", "y z");
        parse(&mut ctx, &["task", "add", "--", "x"]).unwrap();
        assert_eq!(raws(&ctx.cli2.original_args), ["task", "add", "--", "x"]);
    }

    #[test]
    fn command_abbreviation_and_minimum() {
        // Abbreviation depends on the process-wide minimum match length.
        let _lock = crate::test_util::env_lock();
        let mut ctx = context();
        parse(&mut ctx, &["task", "vers"]).unwrap();
        assert_eq!(ctx.cli2.get_command(), "version");
        assert_eq!(find(&ctx, "vers").attribute("canonical"), "version");

        // Ambiguous ('con' is config/context): falls back to default.command.
        ctx.config.set("default.command", "");
        let err = parse(&mut ctx, &["task", "con"]).unwrap_err();
        assert_eq!(err.to_string(), "You must specify a command or a task to modify.");

        // Shorter than abbreviation.minimum.
        let old = minimum_match_length();
        set_minimum_match_length(5);
        let mut ctx = context();
        ctx.config.set("default.command", "");
        let result = parse(&mut ctx, &["task", "vers"]);
        set_minimum_match_length(old);
        assert!(result.is_err());

        // An exact attribute name is not a command abbreviation.
        let mut ctx = context();
        ctx.cli2.entity("attribute", "proj");
        parse(&mut ctx, &["task", "proj", "projects"]).unwrap();
        assert!(!find(&ctx, "proj").has_tag("CMD"));
        assert_eq!(ctx.cli2.get_command(), "projects");
    }

    #[test]
    fn default_and_assumed_commands() {
        let mut ctx = context();
        ctx.config.set("default.command", "next limit:3");
        ctx.cli2.entity("cmd", "next");
        let result = parse(&mut ctx, &["task", "pro:x"]);
        // 'next' is a custom report, not registered without its config.
        assert!(result.is_ok());
        assert_eq!(ctx.cli2.get_command(), "next");
        assert_eq!(raws(&ctx.cli2.args), ["task", "next", "limit:3", "pro:x"]);
        assert!(ctx.cli2.args[1].has_tag("DEFAULT") && ctx.cli2.args[2].has_tag("DEFAULT"));
        assert_eq!(raws(&ctx.cli2.original_args), ["task", "next", "limit:3", "pro:x"]);
        // The pseudo attribute is applied (and, as in C++, not erased).
        assert_eq!(ctx.config.get("limit"), "3");

        // An ID (or UUID) assumes 'information'.
        let mut ctx = context();
        parse(&mut ctx, &["task", "12"]).unwrap();
        assert_eq!(ctx.cli2.get_command(), "information");
        assert!(ctx.cli2.args.last().unwrap().has_tag("ASSUMED"));
        let mut ctx = context();
        parse(&mut ctx, &["task", "a1b2c3d4-0000-1111-2222-333344445555"]).unwrap();
        assert_eq!(ctx.cli2.get_command(), "information");

        // Invoked as 'cal'.
        let mut ctx = context();
        parse(&mut ctx, &["/usr/bin/cal"]).unwrap();
        assert_eq!(ctx.cli2.get_command(), "calendar");
        assert_eq!(ctx.cli2.get_binary(), "/usr/bin/cal");
        assert_eq!(ctx.cli2.args[0].attribute("basename"), "cal");
    }

    #[test]
    fn demotion_and_canonical_names() {
        let mut ctx = context();
        parse(&mut ctx, &["task", "add", "-tag", "+keep", "foo:bar", "due:tomorrow", "rc:x"]).unwrap();
        assert_eq!(find(&ctx, "-tag").lextype, LexerType::word, "demoted for add");
        assert_eq!(find(&ctx, "+keep").lextype, LexerType::tag);
        assert_eq!(find(&ctx, "foo:bar").lextype, LexerType::word, "unknown name");
        assert_eq!(find(&ctx, "due:tomorrow").attribute("canonical"), "due");
        assert_eq!(find(&ctx, "rc:x").attribute("canonical"), "", "rc: is left alone");

        let mut ctx = context();
        parse(&mut ctx, &["task", "-tag", "count"]).unwrap();
        assert_eq!(find(&ctx, "-tag").lextype, LexerType::tag, "not demoted for count");
    }

    #[test]
    fn categories_and_parenthesized_filter() {
        let mut ctx = context();
        // modify: filter before the command, modifications after.
        parse(&mut ctx, &["task", "+a", "or", "+b", "modify", "pro:x"]).unwrap();
        assert_eq!(raws(&ctx.cli2.args), ["task", "(", "+a", "or", "+b", ")", "modify", "pro:x"]);
        for raw in ["(", "+a", "or", "+b", ")"] {
            assert!(find(&ctx, raw).has_tag("FILTER"), "{raw}");
        }
        assert!(find(&ctx, "pro:x").has_tag("MODIFICATION"));

        // config: miscellaneous only; getWords returns them.
        parse(&mut ctx, &["task", "rc:/x/rc", "config", "rc.a:1", "name", "b c", ""]).unwrap();
        let mut debug = Vec::new();
        assert_eq!(ctx.cli2.get_words(&ctx.config, &mut debug), ["name", "b c", ""]);
        assert!(debug.is_empty());
        assert_eq!(raws(&ctx.cli2.get_miscellaneous()), ["name", "b c", ""]);

        parse(&mut ctx, &["task", "config", "--", "rc.a=1", "--", "x"]).unwrap();
        assert_eq!(ctx.cli2.get_words(&ctx.config, &mut debug), ["rc.a=1", "--", "x"]);

        // A command that takes no arguments.
        let err = parse(&mut ctx, &["task", "version", "extra"]).unwrap_err();
        assert_eq!(err.to_string(), "The 'version' command does not allow 'extra'.");
    }

    #[test]
    fn get_words_debug_parser() {
        let mut ctx = context();
        parse(&mut ctx, &["task", "config", "a", "b"]).unwrap();
        let mut config = Configuration::new();
        config.set("debug.parser", 2);
        let mut debug = Vec::new();
        ctx.cli2.get_words(&config, &mut debug);
        let color = Color::parse("gray10 on gray4").unwrap();
        assert_eq!(debug, [format!("CLI2::getWords {} {} ", color.colorize("a"), color.colorize("b"))]);
    }

    #[test]
    fn debug_parser_dumps_each_pass() {
        let mut ctx = context();
        ctx.config.set("debug.parser", 2);
        ctx.cli2.alias("al", "add");
        parse(&mut ctx, &["task", "al", "-x", "pro:a"]).unwrap();
        let titles: Vec<&str> = ctx
            .debug_messages
            .iter()
            .filter_map(|m| m.strip_prefix("\x1b[1m"))
            .map(|m| m.split('\x1b').next().unwrap())
            .collect();
        assert_eq!(
            titles,
            [
                "CLI2::analyze",
                "CLI2::analyze lexArguments",
                "CLI2::analyze aliasExpansion",
                "CLI2::analyze findCommand",
                "CLI2::analyze demotion",
                "CLI2::analyze canonicalizeNames",
                "CLI2::analyze categorizeArgs",
            ]
        );
    }

    #[test]
    fn context_filter_is_injected() {
        let mut ctx = context();
        ctx.config.set("context", "work");
        ctx.config.set("context.work.read", "+work or +w");
        ctx.config.set("context.work.write", "+work");
        ctx.config.set("verbose", "context");
        ctx.verbosity = ["context".to_string()].into_iter().collect();
        // 'count' reads with context.
        parse(&mut ctx, &["task", "pro:a", "count"]).unwrap();
        assert!(ctx.cli2.context_added);
        assert_eq!(raws(&ctx.cli2.original_args), ["task", "(", "+work", "or", "+w", ")", "pro:a", "count"]);
        assert!(ctx.cli2.args.iter().filter(|a| a.has_tag("FILTER")).count() >= 6);
        assert_eq!(ctx.footnotes, ["Context 'work' set. Use 'task context none' to remove."]);
        assert!(ctx.debug_messages.iter().any(|m| m == "Applying context 'work'"));

        // An ID disqualifies the read context.
        let mut ctx2 = context();
        ctx2.config = ctx.config.clone();
        parse(&mut ctx2, &["task", "1", "count"]).unwrap();
        assert!(!ctx2.cli2.context_added);
        assert!(ctx2.debug_messages.iter().any(|m| m == "UUID/ID argument found '1', not applying context."));

        // 'add' takes the write context as modifications after the command.
        let mut ctx3 = context();
        ctx3.config = ctx.config.clone();
        parse(&mut ctx3, &["task", "add", "x"]).unwrap();
        assert_eq!(raws(&ctx3.cli2.original_args), ["task", "add", "+work", "x"]);
        assert!(find(&ctx3, "+work").has_tag("MODIFICATION"));
    }

    #[test]
    fn prepare_filter_footnote() {
        let mut ctx = context();
        ctx.verbosity = ["filter".to_string()].into_iter().collect();
        parse(&mut ctx, &["task", "+a", "export"]).unwrap();
        with_cli2(&mut ctx, |cli2, ctx| cli2.prepare_filter(ctx)).unwrap();
        assert_eq!(ctx.footnotes, ["Filter: ( tags _hastag_ a )"]);
    }

    ////////////////////////////////////////////////////////////////////////////
    // prepareFilter passes (CLI2 part B).

    /// [`context`] plus the configuration defaults (`rc.sugar=1`, ...), the
    /// full column registry and an `attribute` entity per column, as
    /// `Context::initialize` sets them up.
    fn filter_context() -> Context {
        let mut ctx = context();
        ctx.config
            .parse(crate::context::CONFIGURATION_DEFAULTS, 1, &[crate::context::task_rcdir()], "")
            .unwrap();
        crate::columns::column::factory_all(&mut ctx.columns, &ctx.config).unwrap();
        let names: Vec<String> = ctx.columns.keys().cloned().collect();
        for name in names {
            ctx.cli2.entity("attribute", &name);
        }
        ctx
    }

    /// Analyses `list`, runs `prepareFilter` and returns the raw text of the
    /// FILTER arguments.
    fn prepared(ctx: &mut Context, list: &[&str]) -> Vec<String> {
        parse(ctx, list).unwrap();
        with_cli2(ctx, |cli2, ctx| cli2.prepare_filter(ctx)).unwrap();
        filter_raws(ctx)
    }

    fn filter_raws(ctx: &Context) -> Vec<String> {
        ctx.cli2
            .args
            .iter()
            .filter(|a| a.has_tag("FILTER"))
            .map(|a| a.attribute("raw"))
            .collect()
    }

    fn filter_types(ctx: &Context) -> Vec<LexerType> {
        ctx.cli2.args.iter().filter(|a| a.has_tag("FILTER")).map(|a| a.lextype).collect()
    }

    fn words(text: &str) -> Vec<String> {
        text.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn id_sequences() {
        let mut ctx = filter_context();
        // A set with a range and a single ID.
        assert_eq!(
            prepared(&mut ctx, &["task", "1-3,5", "count"]),
            words("( ( ( id >= 1 and id <= 3 ) or ( id == 5 ) ) )")
        );
        assert_eq!(ctx.cli2.id_ranges, [("1".into(), "3".into()), ("5".into(), "5".into())]);
        assert!(ctx.cli2.uuid_list.is_empty());
        let id = ctx.cli2.args.iter().find(|a| a.attribute("raw") == "id").unwrap();
        assert_eq!(id.lextype, LexerType::dom);
        let five = ctx.cli2.args.iter().find(|a| a.attribute("raw") == "5").unwrap();
        assert_eq!(five.lextype, LexerType::number);

        // A descending range is normalised; separate IDs: the first is
        // replaced, the others are eaten.
        assert_eq!(
            prepared(&mut ctx, &["task", "5-3", "7", "count"]),
            words("( ( ( id >= 3 and id <= 5 ) or ( id == 7 ) ) )")
        );
        assert_eq!(ctx.cli2.id_ranges, [("5".into(), "3".into()), ("7".into(), "7".into())]);

        // A number preceded by an operator other than '(' / ')' is not an ID.
        assert_eq!(prepared(&mut ctx, &["task", "urgency", ">", "5", "count"]), words("( urgency > 5 )"));
        assert!(ctx.cli2.id_ranges.is_empty());

        // With no filter at all, integer MODIFICATION numbers/sets are IDs ...
        assert_eq!(prepared(&mut ctx, &["task", "modify", "3"]), words("( ( id == 3 ) )"));
        assert_eq!(prepared(&mut ctx, &["task", "modify", "2,4"]), words("( ( id == 2 ) or ( id == 4 ) )"));
        // ... unless signed or floating point, or the command is add/log.
        assert!(prepared(&mut ctx, &["task", "modify", "3.5"]).is_empty());
        assert!(ctx.cli2.id_ranges.is_empty());
        assert!(prepared(&mut ctx, &["task", "add", "3"]).is_empty());
        assert!(ctx.cli2.id_ranges.is_empty());
        assert!(find(&ctx, "3").has_tag("MODIFICATION"));

        // Sugar-free: a FILTER number becomes the pair id:<n>, then 'id = n'.
        ctx.config.set("sugar", "0");
        assert_eq!(prepared(&mut ctx, &["task", "3", "count"]), words("( id = 3 )"));
        assert!(ctx.cli2.id_ranges.is_empty());
        let id = ctx.cli2.args.iter().find(|a| a.attribute("raw") == "id").unwrap();
        assert_eq!(id.attribute("canonical"), "id");
    }

    #[test]
    fn uuids_full_and_partial() {
        let mut ctx = filter_context();
        let full = "a3e5c7f0-1234-4abc-8def-0123456789ab";
        assert_eq!(
            prepared(&mut ctx, &["task", full, "count"]),
            words(&format!("( ( ( uuid = {full} ) ) )"))
        );
        assert_eq!(ctx.cli2.uuid_list, [full]);
        let value = ctx.cli2.args.iter().find(|a| a.attribute("raw") == full).unwrap();
        assert_eq!(value.lextype, LexerType::string);

        // Partial UUID, combined with IDs (IDs first, then 'or', then UUIDs).
        assert_eq!(
            prepared(&mut ctx, &["task", "abcdef01", "2", "abcdef02", "count"]),
            words("( ( ( id == 2 ) or ( uuid = abcdef01 ) or ( uuid = abcdef02 ) ) )")
        );
        assert_eq!(ctx.cli2.uuid_list, ["abcdef01", "abcdef02"]);

        // A MODIFICATION UUID is promoted when there is no FILTER UUID.
        assert_eq!(prepared(&mut ctx, &["task", "modify", "abcdef01"]), words("( ( uuid = abcdef01 ) )"));

        // Sugar-free: uuid:<uuid>, then 'uuid = <uuid>'.
        ctx.config.set("sugar", "0");
        assert_eq!(prepared(&mut ctx, &["task", "abcdef01", "count"]), words("( uuid = abcdef01 )"));
        assert!(ctx.cli2.uuid_list.is_empty());
    }

    #[test]
    fn tags_desugar() {
        let mut ctx = filter_context();
        assert_eq!(
            prepared(&mut ctx, &["task", "+home", "-work", "count"]),
            words("( tags _hastag_ home and tags _notag_ work )")
        );
        assert_eq!(
            filter_types(&ctx),
            [
                LexerType::op,
                LexerType::dom,
                LexerType::op,
                LexerType::string,
                LexerType::op,
                LexerType::dom,
                LexerType::op,
                LexerType::string,
                LexerType::op,
            ]
        );
    }

    #[test]
    fn attribute_modifiers() {
        let mut ctx = filter_context();
        // project is a string column: the value is not evaluated.
        let cases = [
            ("project:x", "project = x"),
            ("project.before:x", "project < x"),
            ("project.under:x", "project < x"),
            ("project.below:x", "project < x"),
            ("project.after:x", "project > x"),
            ("project.over:x", "project > x"),
            ("project.above:x", "project > x"),
            ("project.by:x", "project <= x"),
            ("project.none:", "project == ''"),
            ("project.any:", "project !== ''"),
            ("project.is:x", "project == x"),
            ("project.equals:x", "project == x"),
            ("project.not:x", "project != x"),
            ("project.isnt:x", "project !== x"),
            ("project.has:x", "project ~ x"),
            ("project.contains:x", "project ~ x"),
            ("project.hasnt:x", "project !~ x"),
            ("project.startswith:x", "project ~ ^x"),
            ("project.left:x", "project ~ ^x"),
            ("project.endswith:x", "project ~ x$"),
            ("project.right:x", "project ~ x$"),
            ("project.word:x", "project ~ \\bx\\b"),
            ("project.noword:x", "project !~ \\bx\\b"),
            // An empty value is "''".
            ("project:", "project = ''"),
            // A multi-token value of a string attribute stays one string.
            ("project:a+b", "project = a+b"),
        ];
        for (arg, expected) in cases {
            let raws = prepared(&mut ctx, &["task", arg, "count"]);
            assert_eq!(raws, words(&format!("( {expected} )")), "{arg}");
            let types = filter_types(&ctx);
            assert_eq!(types[1..4], [LexerType::dom, LexerType::op, LexerType::string], "{arg}");
            let lhs = &ctx.cli2.args.iter().filter(|a| a.has_tag("FILTER")).nth(1).unwrap();
            assert_eq!(lhs.attribute("canonical"), "project", "{arg}");
        }

        // Abbreviated names keep the raw name, with the canonical name for
        // the token Eval sees.
        prepared(&mut ctx, &["task", "pro.has:x", "count"]);
        let lhs = ctx.cli2.args.iter().find(|a| a.attribute("raw") == "pro").unwrap();
        assert_eq!((lhs.attribute("canonical").as_str(), lhs.get_token().as_str()), ("project", "project"));
        assert_eq!(lhs.attribute("modifier"), "has");

        // A string attribute whose value is a DOM reference compares to it.
        assert_eq!(prepared(&mut ctx, &["task", "project:rc.x", "count"]), words("( project = rc.x )"));
        assert_eq!(filter_types(&ctx)[3], LexerType::dom);

        // Date attributes evaluate their value: a single token keeps its type,
        // multiple tokens are parenthesized.
        assert_eq!(prepared(&mut ctx, &["task", "due.before:eom", "count"]), words("( due < eom )"));
        assert_ne!(filter_types(&ctx)[3], LexerType::string);
        assert_eq!(
            prepared(&mut ctx, &["task", "due.after:now+1d", "count"]),
            words("( due > ( now + 1d ) )")
        );
        assert_eq!(prepared(&mut ctx, &["task", "due.none:", "count"]), words("( due == '' )"));
        // Numeric and duration attributes also evaluate.
        assert_eq!(prepared(&mut ctx, &["task", "urgency.over:2", "count"]), words("( urgency > 2 )"));
        assert_eq!(filter_types(&ctx)[3], LexerType::number);
        // recur is a string column (C++ ColumnRecur : ColumnTypeString).
        assert_eq!(prepared(&mut ctx, &["task", "recur:1w+1d", "count"]), words("( recur = 1w+1d )"));
        {
            let mut ctx = context();
            ctx.config
                .parse(crate::context::CONFIGURATION_DEFAULTS, 1, &[crate::context::task_rcdir()], "")
                .unwrap();
            ctx.config.set("uda.est.type", "duration");
            crate::columns::column::factory_all(&mut ctx.columns, &ctx.config).unwrap();
            ctx.cli2.entity("attribute", "est");
            assert_eq!(ctx.columns.get("est").unwrap().type_(), "duration");
            assert_eq!(prepared(&mut ctx, &["task", "est.over:1w+1d", "count"]), words("( est > ( 1w + 1d ) )"));
            assert_eq!(filter_types(&ctx)[3..6], [LexerType::op, LexerType::duration, LexerType::op]);
        }

        // A name that is not an attribute is downgraded to a word.
        assert_eq!(prepared(&mut ctx, &["task", "foo:bar", "count"]), words("( foo:bar )"));
        assert_eq!(filter_types(&ctx)[1], LexerType::word);

        // Unknown modifier.
        parse(&mut ctx, &["task", "project.bogus:x", "count"]).unwrap();
        let err = with_cli2(&mut ctx, |cli2, ctx| cli2.prepare_filter(ctx)).unwrap_err();
        assert_eq!(err.to_string(), "Error: unrecognized attribute modifier 'bogus'.");

        // C++ std::out_of_range from A2 ("", Lexer::Type::tag).
        parse(&mut ctx, &["task", "due:+foo", "count"]).unwrap();
        let err = with_cli2(&mut ctx, |cli2, ctx| cli2.prepare_filter(ctx)).unwrap_err();
        assert!(matches!(err, TwError::Runtime(_)), "{err:?}");
    }

    #[test]
    fn patterns_and_plain_args() {
        let mut ctx = filter_context();
        assert_eq!(prepared(&mut ctx, &["task", "/foo/", "count"]), words("( description ~ foo )"));
        let rhs = ctx.cli2.args.iter().find(|a| a.attribute("raw") == "foo").unwrap();
        assert_eq!(rhs.lextype, LexerType::string);
        assert!(rhs.attributes.contains_key("flags"));

        // Plain words are description searches, dequoted, tagged PLAIN.
        // ("bar baz" is one multi-token argv: a word, re-lexed by lexFilterArgs.)
        assert_eq!(
            prepared(&mut ctx, &["task", "foo", "bar baz", "count"]),
            words("( description ~ foo and description ~ bar and description ~ baz )")
        );
        let plain: Vec<&A2> = ctx.cli2.args.iter().filter(|a| a.has_tag("PLAIN")).collect();
        assert_eq!(plain.len(), 9);
        assert_eq!(plain[0].attribute("canonical"), "description");

        // A quoted single token is a string, not a plain-arg candidate.
        assert_eq!(prepared(&mut ctx, &["task", "foo", "'bar baz'", "count"]), [
            "(", "description", "~", "foo", "and", "'bar baz'", ")"
        ]);

        // A word that is part of an expression is not plain.
        assert_eq!(prepared(&mut ctx, &["task", "description", "~", "x", "count"]), words("( description ~ x )"));
        assert!(!ctx.cli2.args.iter().any(|a| a.has_tag("PLAIN")));

        // A FILTER word containing an expression is lexed into tokens.
        assert_eq!(prepared(&mut ctx, &["task", "urgency>5", "count"]), words("( urgency > 5 )"));
    }

    #[test]
    fn junctions_and_empty_parens() {
        let mut ctx = filter_context();
        // ) (  -->  ) and (
        assert_eq!(
            prepared(&mut ctx, &["task", "(", "+a", ")", "(", "+b", ")", "count"]),
            words("( ( tags _hastag_ a ) and ( tags _hastag_ b ) )")
        );
        // ) <non-op>  and  <non-op> (
        assert_eq!(
            prepared(&mut ctx, &["task", "(", "+a", ")", "+b", "(", "+c", ")", "count"]),
            words("( ( tags _hastag_ a ) and tags _hastag_ b and ( tags _hastag_ c ) )")
        );
        // Explicit operators get no junction.
        assert_eq!(
            prepared(&mut ctx, &["task", "+a", "or", "+b", "count"]),
            words("( tags _hastag_ a or tags _hastag_ b )")
        );
        // Empty paren expressions get no junction either side.
        assert_eq!(
            prepared(&mut ctx, &["task", "+a", "(", ")", "count"]),
            words("( tags _hastag_ a ( ) )")
        );
        assert_eq!(
            prepared(&mut ctx, &["task", "(", ")", "+a", "count"]),
            words("( ( ) tags _hastag_ a )")
        );
        assert_eq!(
            prepared(&mut ctx, &["task", "(", "+a", ")", "(", ")", "count"]),
            words("( ( tags _hastag_ a ) ( ) )")
        );
    }

    #[test]
    fn is_empty_paren_expression_scans() {
        let mut cli2 = Cli2::new();
        let set = |cli2: &mut Cli2, list: &[&str]| {
            cli2.args = list.iter().map(|r| A2::new(r, LexerType::op)).collect();
        };
        set(&mut cli2, &["task", "(", "(", ")", ")", "x"]);
        assert!(cli2.is_empty_paren_expression(1, true));
        assert!(cli2.is_empty_paren_expression(4, false));
        set(&mut cli2, &["task", "(", "x", ")"]);
        assert!(!cli2.is_empty_paren_expression(1, true));
        assert!(!cli2.is_empty_paren_expression(3, false));
        // Unbalanced to the end.
        set(&mut cli2, &["task", "(", "("]);
        assert!(!cli2.is_empty_paren_expression(1, true));
        // As in C++, the backward scan never examines args[0].
        set(&mut cli2, &["(", ")"]);
        assert!(!cli2.is_empty_paren_expression(1, false));
    }

    #[test]
    fn stray_modifications_for_add() {
        // findUUIDs promotes a MODIFICATION UUID even for 'add'; the stray
        // FILTER args are then turned back into MODIFICATIONs.
        let mut ctx = filter_context();
        ctx.config.set("debug.parser", "2");
        assert!(prepared(&mut ctx, &["task", "add", "abcdef01"]).is_empty());
        let mods: Vec<String> = ctx
            .cli2
            .args
            .iter()
            .filter(|a| a.has_tag("MODIFICATION"))
            .map(|a| a.attribute("raw"))
            .collect();
        assert_eq!(mods, words("( ( uuid = abcdef01 ) )"));
        assert!(ctx
            .debug_messages
            .iter()
            .any(|m| m.contains("CLI2::prepareFilter findStrayModifications")));

        // 'log' too; other commands keep their FILTER.
        assert!(prepared(&mut ctx, &["task", "log", "abcdef01"]).is_empty());
        assert!(!prepared(&mut ctx, &["task", "modify", "abcdef01"]).is_empty());
    }

    #[test]
    fn prepare_filter_debug_dumps() {
        let mut ctx = filter_context();
        ctx.config.set("debug.parser", "2");
        parse(&mut ctx, &["task", "1-3", "+home", "project:x", "due.before:eom", "/foo/", "bar", "count"]).unwrap();
        ctx.debug_messages.clear();
        with_cli2(&mut ctx, |cli2, ctx| cli2.prepare_filter(ctx)).unwrap();
        let titles: Vec<&str> = ctx
            .debug_messages
            .iter()
            .filter_map(|m| m.strip_prefix("\x1b[1m"))
            .map(|m| m.split('\x1b').next().unwrap())
            .collect();
        assert_eq!(
            titles,
            [
                "CLI2::prepareFilter findIDs",
                "CLI2::prepareFilter insertIDExpr",
                "CLI2::prepareFilter desugarFilterPlainArgs",
                "CLI2::prepareFilter desugarFilterTags",
                "CLI2::prepareFilter desugarFilterAttributes",
                "CLI2::prepareFilter desugarFilterPatterns",
                "CLI2::prepareFilter insertJunctions",
            ]
        );
        // lexExpression output for each attribute.
        let i = ctx.debug_messages.iter().position(|m| m == "CLI2::lexExpression project:x").unwrap();
        assert!(ctx.debug_messages[i + 1].starts_with("  "));
        assert_eq!(ctx.debug_messages[i + 2], " ");
        assert!(ctx.debug_messages.iter().any(|m| m == "CLI2::lexExpression due:eom"));
        assert_eq!(
            filter_raws(&ctx),
            words(
                "( ( ( id >= 1 and id <= 3 ) ) and tags _hastag_ home and project = x and due < eom \
                 and description ~ foo and description ~ bar )"
            )
        );

        // A word FILTER arg triggers lexFilterArgs.
        parse(&mut ctx, &["task", "urgency>5", "count"]).unwrap();
        ctx.debug_messages.clear();
        with_cli2(&mut ctx, |cli2, ctx| cli2.prepare_filter(ctx)).unwrap();
        assert!(ctx.debug_messages[0].contains("CLI2::prepareFilter lexFilterArgs"));
    }
}
