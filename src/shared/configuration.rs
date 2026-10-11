//! Rust translation of `libshared src/Configuration.cpp` (everything declared
//! in `Configuration.h`): the [`Configuration`] map and the free functions
//! [`set_variable_in_file`] / [`unset_variable_in_file`].
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.
//!
//! # Shape (spec Design Decision 2)
//!
//! C++ `class Configuration : public std::map<std::string, std::string>` is a
//! struct wrapping a [`BTreeMap<String, String>`] (same sorted iteration order
//! as `std::map`) plus the dirty flag and the original file. The inherited
//! `std::map` API (`find`, `size`, `erase`, iteration, …) is reached through
//! [`Deref`]/[`DerefMut`] to the map and `IntoIterator for &Configuration`;
//! the inherent methods below (`get`, `set`, …) shadow the map's methods of the
//! same name, exactly as the C++ members hide nothing but add to the map.
//!
//! | C++                                        | Rust                                              |
//! |--------------------------------------------|---------------------------------------------------|
//! | `Configuration ()`                         | [`Configuration::new`] / [`Default`]              |
//! | `load (file, nest = 1, search_paths = {})` | [`Configuration::load`]                           |
//! | `save ()`                                  | [`Configuration::save`]                           |
//! | `parse (input, nest = 1, search_paths = {}, file_path = {})` | [`Configuration::parse`]        |
//! | `has (key)`                                | [`Configuration::has`]                            |
//! | `get (key, getFromContext = true)`         | [`Configuration::get`], [`Configuration::get_from_context`] |
//! | `getInteger (key, getFromContext = true)`  | [`Configuration::get_integer`], [`Configuration::get_integer_from_context`] |
//! | `getReal (key, getFromContext = true)`     | [`Configuration::get_real`], [`Configuration::get_real_from_context`] |
//! | `getBoolean (key, getFromContext = true)`  | [`Configuration::get_boolean`], [`Configuration::get_boolean_from_context`] |
//! | `set (key, int)` / `set (key, double)` / `set (key, string)` | [`Configuration::set`] (overloaded through [`ConfigValue`]) |
//! | `setIfBlank (key, value)`                  | [`Configuration::set_if_blank`]                   |
//! | `all ()`                                   | [`Configuration::all`]                            |
//! | `file ()`                                  | [`Configuration::file`]                           |
//! | `dirty ()`                                 | [`Configuration::dirty`]                          |
//! | `operator[]` (autovivifying)               | [`Configuration::value_mut`]                      |
//! | `setVariableInFile`, `unsetVariableInFile` | [`set_variable_in_file`], [`unset_variable_in_file`] |
//!
//! C++ `throw std::string` becomes [`TwError::Message`] with identical text;
//! errors raised by `Path::readable` propagate as they do in C++.
//!
//! # File format (as parsed by [`Configuration::parse`])
//!
//! - Input is split on `'\n'`; everything from the first `#` on a line is a
//!   comment. The rest is trimmed of `" \t\n\f\r"`; blank lines are skipped.
//! - A line containing `=` is `key=value`: both sides are trimmed, the value
//!   is [`Path::expand`]ed (leading `~`, `$VAR`) and then JSON-decoded
//!   (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`; any other
//!   escape is kept verbatim, a trailing `\` stays a `\`). There is no line
//!   continuation.
//! - Otherwise a line containing `include` anywhere names the file after it
//!   (trimmed, [`Path::expand`]ed). A relative name is looked up, in order,
//!   relative to the current directory, to the directory of the (real path
//!   of the) including file, and in each search path. Includes nest up to 10
//!   levels.
//! - Any other non-blank line is an error: `Malformed entry '<line>' in
//!   config file.`
//!
//! Values are decoded with [`crate::shared::json::decode`] (libshared
//! `json::decode`).

use std::collections::BTreeMap;
use std::ffi::CString;
use std::ops::{Deref, DerefMut};

use crate::error::{Result, TwError};
use crate::shared::format::{format, format_double};
use crate::shared::fs::{File, Path};
use crate::shared::json;
use crate::shared::shared::{lower_case, split, trim};

/// An empty search-path list: the C++ default argument `search_paths = {}`.
pub const NO_SEARCH_PATHS: &[&str] = &[];

////////////////////////////////////////////////////////////////////////////////
/// C++ `setVariableInFile`: sets `name=value` in the configuration file
/// `file`. Every line containing `name=` before any `#` is replaced (keeping
/// a trailing comment, separated by one space); if no line matches, the
/// assignment is appended. Returns whether the file was rewritten.
///
/// As in C++ the match is a plain substring search, so `name=` also matches
/// inside a longer name such as `xname=`.
pub fn set_variable_in_file(file: &str, name: &str, value: &str) -> bool {
    // Read the file contents.
    let mut contents = File::read_file_lines(file).unwrap_or_default();

    let mut found = false;
    let mut change = false;

    let pattern = format!("{name}=");
    for line in contents.iter_mut() {
        // If there is a comment on the line, it must follow the pattern.
        let comment = line.find('#');
        let pos = line.find(&pattern);

        if let Some(pos) = pos {
            if comment.is_none_or(|comment| comment > pos) {
                found = true;
                *line = match comment {
                    Some(comment) => format!("{name}={value} {}", &line[comment..]),
                    None => format!("{name}={value}"),
                };

                change = true;
            }
        }
    }

    // Not found, so append instead.
    if !found {
        contents.push(format!("{name}={value}"));
        change = true;
    }

    if change {
        File::write_file_lines(file, &contents, true);
    }

    change
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `unsetVariableInFile`: removes every line of `file` containing
/// `name=` before any `#`. Returns whether the file was rewritten.
pub fn unset_variable_in_file(file: &str, name: &str) -> bool {
    // Read configuration file.
    let mut contents = File::read_file_lines(file).unwrap_or_default();

    let pattern = format!("{name}=");
    let before = contents.len();
    contents.retain(|line| {
        // If there is a comment on the line, it must follow the pattern.
        let comment = line.find('#');
        match line.find(&pattern) {
            Some(pos) => comment.is_some_and(|comment| comment <= pos),
            None => true,
        }
    });

    let change = contents.len() != before;
    if change {
        File::write_file_lines(file, &contents, true);
    }

    change
}

////////////////////////////////////////////////////////////////////////////////
/// The overloads of C++ `Configuration::set`: how a value becomes the stored
/// string.
pub trait ConfigValue {
    /// The text stored for this value.
    fn to_config_string(self) -> String;
}

/// C++ `set (const std::string&, const std::string&)`: stored as is.
impl ConfigValue for &str {
    fn to_config_string(self) -> String {
        self.to_string()
    }
}

/// C++ `set (const std::string&, const std::string&)`: stored as is.
impl ConfigValue for String {
    fn to_config_string(self) -> String {
        self
    }
}

/// C++ `set (const std::string&, const std::string&)`: stored as is.
impl ConfigValue for &String {
    fn to_config_string(self) -> String {
        self.clone()
    }
}

/// C++ `set (const std::string&, const int)`: `format (value)`, the decimal
/// text.
impl ConfigValue for i32 {
    fn to_config_string(self) -> String {
        self.to_string()
    }
}

/// C++ `set (const std::string&, const double)`: `format (value, 1, 8)`,
/// i.e. `%g` at 8 significant digits. As in C++, a value strictly between 0
/// and 1 is first rounded to an integer (width 1 leaves no decimal places),
/// so `0.5` is stored as `"1"` and `0.25` as `"0"`.
impl ConfigValue for f64 {
    fn to_config_string(self) -> String {
        format_double(self, 1, 8)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `Configuration`: the settings map plus the file it was loaded from and
/// whether it has changed since.
#[derive(Debug, Clone, Default)]
pub struct Configuration {
    /// The inherited `std::map<std::string, std::string>`.
    map: BTreeMap<String, String>,
    /// C++ `_original_file`: the top-level file given to [`load`](Self::load).
    original_file: File,
    /// C++ `_dirty`.
    dirty: bool,
}

impl Configuration {
    /// C++ default constructor: empty, not dirty, no file.
    pub fn new() -> Self {
        Configuration::default()
    }

    /// C++ `Configuration::load`: reads `file` and [`parse`](Self::parse)s its
    /// contents. At `nest == 1` the file is remembered as the original file
    /// (see [`file`](Self::file) and [`save`](Self::save)). A file that does
    /// not exist, is not readable or is empty is silently ignored. C++
    /// defaults: `nest = 1`, `search_paths = {}` ([`NO_SEARCH_PATHS`]).
    ///
    /// As in C++, existence is checked on the [`Path::expand`]ed name but the
    /// file is read by the name as given.
    pub fn load<S: AsRef<str>>(&mut self, file: &str, nest: i32, search_paths: &[S]) -> Result<()> {
        if nest > 10 {
            return Err(TwError::Message(
                "Configuration files may only be nested to 10 levels.".to_string(),
            ));
        }

        // Read the file, then parse the contents.
        let config = File::new(file);

        if nest == 1 {
            self.original_file = config.clone();
        }

        if config.exists() && config.readable()? {
            if let Some(contents) = File::read_file(file) {
                if !contents.is_empty() {
                    self.parse(&contents, nest, search_paths, file)?;
                }
            }
        }

        Ok(())
    }

    /// C++ `Configuration::save`: writes every setting as `key=value\n`, in
    /// key order, over the original file and clears the dirty flag. As in
    /// C++, a failure to write is ignored.
    pub fn save(&mut self) {
        let mut contents = String::new();
        for (key, value) in &self.map {
            contents.push_str(key);
            contents.push('=');
            contents.push_str(value);
            contents.push('\n');
        }

        File::write_file(&self.original_file.data, &contents);
        self.dirty = false;
    }

    /// C++ `Configuration::parse`: adds the settings in `input` (see the
    /// module documentation for the format). `nest` is the include depth of
    /// `input`, `search_paths` are tried for relative includes, and
    /// `file_path` is the file `input` came from (`""` if none), whose
    /// directory is also tried. C++ defaults: `nest = 1`, `search_paths = {}`,
    /// `file_path = {}`.
    ///
    /// Empty input changes nothing; any other input marks the configuration
    /// dirty (unless an error is raised first).
    pub fn parse<S: AsRef<str>>(
        &mut self,
        input: &str,
        nest: i32,
        search_paths: &[S],
        file_path: &str,
    ) -> Result<()> {
        // Shortcut case for default constructor.
        if input.is_empty() {
            return Ok(());
        }

        // Parse each line.
        for line in split(input, '\n') {
            // Remove comments.
            let line = match line.find('#') {
                Some(pound) => &line[..pound],
                None => line.as_str(),
            };

            // Skip empty lines.
            let line = trim(line);
            if line.is_empty() {
                continue;
            }

            if let Some(equal) = line.find('=') {
                let key = trim(&line[..equal]);
                let value = trim(&line[equal + 1..]);
                let value = Path::expand(&value);

                self.map.insert(key, json::decode(&value));
            } else if let Some(include) = line.find("include") {
                let mut included = Path::new(&trim(&line[include + 7..]));

                'search: {
                    // 0. Absolute path is not searched.
                    if included.is_absolute() {
                        break 'search;
                    }

                    // 1. Try relative to CWD first, break if exists. This is the legacy behavior.
                    if included.exists() {
                        break 'search;
                    }

                    // 2. Try path relative to the config file itself.
                    if !file_path.is_empty() {
                        let mut file_dir = Path::new(file_path).realpath();
                        let keep = file_dir.rfind('/').map_or(0, |slash| slash + 1); // `/` is kept.
                        file_dir.truncate(keep);
                        let file_relative = Path::new(&(file_dir + &included.data));
                        if file_relative.exists() {
                            included = file_relative;
                            break 'search;
                        }
                    }

                    // 3. Try search paths.
                    for search in search_paths {
                        let concated = Path::new(&format!("{}/{}", search.as_ref(), included.data));
                        if concated.exists() {
                            included = concated;
                            break;
                        }
                    }

                    if !included.exists() {
                        return Err(TwError::Message(format(
                            "Could not find file in CWD, directory of config file or search paths '{1}'.",
                            &[&included.data],
                        )));
                    }
                }

                if !included.readable()? {
                    return Err(TwError::Message(format(
                        "Could not read include file '{1}'.",
                        &[&included.data],
                    )));
                }

                self.load(&included.data, nest + 1, search_paths)?;
            } else {
                return Err(TwError::Message(format(
                    "Malformed entry '{1}' in config file.",
                    &[&line],
                )));
            }
        }

        self.dirty = true;
        Ok(())
    }

    /// The key looked up first when a `context` setting is present: for a
    /// `key` not starting with `context.`, `context.<context>.rc.<key>`.
    fn context_key(&self, key: &str, get_from_context: bool) -> Option<String> {
        match self.map.get("context") {
            Some(context) if get_from_context && !key.starts_with("context.") => {
                Some(format!("context.{context}.rc.{key}"))
            }
            _ => None,
        }
    }

    /// C++ `Configuration::has`: whether `key` is set, either directly or (when
    /// a `context` is set and `key` does not start with `context.`) as
    /// `context.<context>.rc.<key>`.
    pub fn has(&self, key: &str) -> bool {
        if let Some(ckey) = self.context_key(key, true) {
            if self.map.contains_key(&ckey) {
                return true;
            }
        }

        self.map.contains_key(key)
    }

    /// C++ `Configuration::get (key)` (`getFromContext = true`).
    pub fn get(&self, key: &str) -> String {
        self.get_from_context(key, true)
    }

    /// C++ `Configuration::get (key, getFromContext)`: the value of `key`, or
    /// `""` if unset. With `get_from_context`, a set `context` and a `key` not
    /// starting with `context.`, `context.<context>.rc.<key>` wins over the
    /// global value.
    pub fn get_from_context(&self, key: &str, get_from_context: bool) -> String {
        if let Some(ckey) = self.context_key(key, get_from_context) {
            if let Some(found) = self.map.get(&ckey) {
                return found.clone();
            }
        }

        // Fallback - use global config value
        self.map.get(key).cloned().unwrap_or_default()
    }

    /// C++ `Configuration::getInteger (key)` (`getFromContext = true`).
    pub fn get_integer(&self, key: &str) -> i32 {
        self.get_integer_from_context(key, true)
    }

    /// C++ `Configuration::getInteger (key, getFromContext)`: `strtoimax (value,
    /// nullptr, 10)` narrowed to `int` (so the leading integer of the text,
    /// `0` if none or unset).
    pub fn get_integer_from_context(&self, key: &str, get_from_context: bool) -> i32 {
        let val = self.get_from_context(key, get_from_context);
        if !val.is_empty() {
            // The C++ implicit intmax_t -> int conversion keeps the low bits.
            return strtoimax(&val) as i32;
        }

        0
    }

    /// C++ `Configuration::getReal (key)` (`getFromContext = true`).
    pub fn get_real(&self, key: &str) -> f64 {
        self.get_real_from_context(key, true)
    }

    /// C++ `Configuration::getReal (key, getFromContext)`: `strtod (value,
    /// nullptr)` (the leading number of the text, `0.0` if none or unset).
    pub fn get_real_from_context(&self, key: &str, get_from_context: bool) -> f64 {
        let val = self.get_from_context(key, get_from_context);
        if !val.is_empty() {
            return strtod(&val);
        }

        0.0
    }

    /// C++ `Configuration::getBoolean (key)` (`getFromContext = true`).
    pub fn get_boolean(&self, key: &str) -> bool {
        self.get_boolean_from_context(key, true)
    }

    /// C++ `Configuration::getBoolean (key, getFromContext)`: `true` only for
    /// `true`, `1`, `y`, `yes` or `on`, compared case-insensitively.
    pub fn get_boolean_from_context(&self, key: &str, get_from_context: bool) -> bool {
        let val = self.get_from_context(key, get_from_context);
        if !val.is_empty() {
            let value = lower_case(&val);
            if value == "true" || value == "1" || value == "y" || value == "yes" || value == "on" {
                return true;
            }
        }

        false
    }

    /// C++ `Configuration::set` (all three overloads, see [`ConfigValue`]):
    /// stores `value` under `key` and marks the configuration dirty.
    pub fn set<V: ConfigValue>(&mut self, key: &str, value: V) {
        self.map.insert(key.to_string(), value.to_config_string());
        self.dirty = true;
    }

    /// C++ `Configuration::setIfBlank`: stores `value` (and marks dirty) only
    /// if `key` is unset or empty. As in C++ (autovivification), a missing
    /// `key` is created even when `value` is itself empty.
    pub fn set_if_blank(&mut self, key: &str, value: &str) {
        let entry = self.value_mut(key);
        if entry.is_empty() {
            *entry = value.to_string();
            self.dirty = true;
        }
    }

    /// C++ `std::map::operator[]`: the value of `key`, inserting `""` first if
    /// it is missing. Does not touch the dirty flag.
    pub fn value_mut(&mut self, key: &str) -> &mut String {
        self.map.entry(key.to_string()).or_default()
    }

    /// C++ `Configuration::all`: every key, in sorted order.
    pub fn all(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    /// C++ `Configuration::file`: the (expanded) path of the top-level file
    /// given to [`load`](Self::load), or `""`.
    pub fn file(&self) -> String {
        self.original_file.data.clone()
    }

    /// C++ `Configuration::dirty`: whether anything was parsed or set since
    /// construction or the last [`save`](Self::save).
    pub fn dirty(&self) -> bool {
        self.dirty
    }
}

/// The inherited `std::map` interface (read-only members).
impl Deref for Configuration {
    type Target = BTreeMap<String, String>;

    fn deref(&self) -> &BTreeMap<String, String> {
        &self.map
    }
}

/// The inherited `std::map` interface (mutating members). Like direct map
/// access in C++, this does not touch the dirty flag.
impl DerefMut for Configuration {
    fn deref_mut(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.map
    }
}

/// C++ `for (auto& i : config)`: iterates in key order.
impl<'a> IntoIterator for &'a Configuration {
    type Item = (&'a String, &'a String);
    type IntoIter = std::collections::btree_map::Iter<'a, String, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

////////////////////////////////////////////////////////////////////////////////
/// `text` as a C string, truncated at the first NUL like `c_str ()`.
fn c_string(text: &str) -> CString {
    let text = text.find('\0').map_or(text, |nul| &text[..nul]);
    CString::new(text).unwrap_or_default()
}

/// C `strtoimax (s, nullptr, 10)` on a 64-bit `intmax_t`: skips leading
/// `isspace` characters, takes an optional sign and the longest run of
/// decimal digits, saturating at `INTMAX_MIN`/`INTMAX_MAX`; `0` when there
/// are no digits. Text after a NUL is ignored, as `c_str ()` does.
fn strtoimax(text: &str) -> i64 {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }

    let mut negative = false;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        negative = bytes[i] == b'-';
        i += 1;
    }

    // Accumulate towards the sign so INTMAX_MIN is representable.
    let mut value: i64 = 0;
    let mut overflow = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        let digit = (bytes[i] - b'0') as i64;
        if !overflow {
            let next = value.checked_mul(10).and_then(|v| {
                if negative {
                    v.checked_sub(digit)
                } else {
                    v.checked_add(digit)
                }
            });
            match next {
                Some(next) => value = next,
                None => overflow = true,
            }
        }
        i += 1;
    }

    if overflow {
        return if negative { i64::MIN } else { i64::MAX };
    }

    value
}

/// C `strtod (s, nullptr)`, called through libc so every accepted form
/// (decimal, exponent, hexadecimal, `inf`, `nan`) and its rounding match.
/// The process never calls `setlocale`, so the radix character is `.` as in
/// the C++ build.
fn strtod(text: &str) -> f64 {
    let text = c_string(text);
    // SAFETY: `text` is a valid NUL-terminated string and a null `endptr` is
    // permitted.
    unsafe { libc::strtod(text.as_ptr(), std::ptr::null_mut()) }
}

////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
// The C++ tests use the literal 3.14.
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Writes `contents` to `dir/name` and returns the path as a string.
    fn write(dir: &std::path::Path, name: &str, contents: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("write fixture");
        path.to_str().expect("utf-8 path").to_string()
    }

    fn message(result: Result<()>) -> String {
        match result {
            Err(TwError::Message(message)) => message,
            other => panic!("expected TwError::Message, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------------
    // libshared test/configuration.t.cpp (21 assertions, in order).

    #[test]
    fn configuration_t_cpp() {
        // bool has (const std::string&);
        let mut c1 = Configuration::new();
        assert!(!c1.has("missing"), "Configuration 'missing' --> false");

        // void set (const std::string&, const std::string&);
        // std::string get (const std::string&);
        c1.set("name", "value");
        assert!(c1.has("name"), "Configuration 'name' --> true");
        assert_eq!(c1.get("name"), "value", "Configuration 'name' --> true");

        // void set (const std::string&, const int);
        // int getInteger (const std::string&);
        c1.set("integer", 1);
        assert!(
            c1.get_integer("integer") == 1,
            "Configuration 'integer' --> 1"
        );

        // void set (const std::string&, const double);
        // double getReal (const std::string&);
        c1.set("real", 3.14);
        assert!(c1.get_real("real") == 3.14, "Configuration 'real' --> 3.14");

        // bool getBoolean (const std::string&);
        c1.set("boolean1", "true");
        c1.set("boolean2", "1");
        c1.set("boolean3", "y");
        c1.set("boolean4", "yes");
        c1.set("boolean5", "on");
        assert!(
            c1.get_boolean("boolean1"),
            "Configuration 'boolean1' --> true"
        );
        assert!(
            c1.get_boolean("boolean2"),
            "Configuration 'boolean2' --> true"
        );
        assert!(
            c1.get_boolean("boolean3"),
            "Configuration 'boolean3' --> true"
        );
        assert!(
            c1.get_boolean("boolean4"),
            "Configuration 'boolean4' --> true"
        );
        assert!(
            c1.get_boolean("boolean5"),
            "Configuration 'boolean5' --> true"
        );

        // void parse (const std::string&, int nest = 1, const std::vector <std::string>& search_paths = {});
        let mut c2 = Configuration::new();
        c2.parse(
            "\n\
             # Comment\n\
             integer=1\n\
             boolean=true\n\
             real=3.14\n\
             string=hello\n\
             \n \t foo\t=     bar   # Comment\n",
            1,
            NO_SEARCH_PATHS,
            "",
        )
        .expect("parse");
        assert!(
            c2.get_integer("integer") == 1,
            "Configuration 'integer' --> 1"
        );
        assert!(
            c2.get_boolean("boolean"),
            "Configuration 'boolean' --> true"
        );
        assert!(c2.get_real("real") == 3.14, "Configuration 'real' --> 3.14");
        assert!(
            c2.get("string") == "hello",
            "Configuration 'string' --> 'hello'"
        );
        assert_eq!(c2.get("foo"), "bar", "Configuration 'foo' --> 'bar'");

        // std::vector <std::string> all () const;
        assert!(
            c2.all().len() == 5,
            "Configuration::all --> 'integer', 'boolean', 'real', 'string', 'foo'"
        );
        assert!(
            c2.len() == 5,
            "Configuration::size --> 'integer', 'boolean', 'real', 'string', 'foo'"
        );

        // void setIfBlank (const std::string& key, const std::string& value)
        let mut c3 = Configuration::new();
        c3.set("foo", 1);
        c3.set("bar", "");
        c3.set_if_blank("foo", "1plus");
        c3.set_if_blank("bar", "2plus");
        c3.set_if_blank("baz", "3plus");
        assert_eq!(
            c3.get("foo"),
            "1",
            "Configuration::setIfBlank doesn't change non-blank values."
        );
        assert_eq!(
            c3.get("bar"),
            "2plus",
            "Configuration::setIfBlank changes blank values."
        );
        assert_eq!(
            c3.get("baz"),
            "3plus",
            "Configuration::setIfBlank sets missing values."
        );

        // void load (const std::string&, int nest = 1, const std::vector <std::string>& search_paths = {});
        // The fixture tree of libshared test/config/, recreated in a temporary
        // directory. The C++ test uses $FOO; a unique name avoids clashing
        // with other tests running in the same process.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("nest")).expect("mkdir nest");
        let one = write(
            dir.path(),
            "1.conf",
            "# Expands to 2.conf, file-relative search.\ninclude $TW_CONFIGURATION_T_FOO\n",
        );
        write(dir.path(), "2.conf", "# Search path.\ninclude 3.conf\n");
        write(&dir.path().join("nest"), "3.conf", "hello=world\n");
        let nest = dir.path().join("nest").to_str().expect("utf-8").to_string();

        std::env::set_var("TW_CONFIGURATION_T_FOO", "2.conf");
        let mut c4 = Configuration::new();
        c4.load(&one, 1, &[nest]).expect("load");
        assert_eq!(
            c4.get("hello"),
            "world",
            "Configuration::load with search paths"
        );
    }

    // ---------------------------------------------------------------------
    // Construction, dirty flag, file, save.

    #[test]
    fn new_is_empty_clean_and_fileless() {
        let c = Configuration::new();
        assert!(c.is_empty());
        assert!(!c.dirty());
        assert_eq!(c.file(), "");
        assert!(c.all().is_empty());
    }

    #[test]
    fn dirty_tracking() {
        let mut c = Configuration::new();
        c.parse("", 1, NO_SEARCH_PATHS, "").expect("parse");
        assert!(!c.dirty(), "empty input is a shortcut and leaves the flag");

        c.parse("# only a comment\n", 1, NO_SEARCH_PATHS, "")
            .expect("parse");
        assert!(c.dirty(), "any non-empty input marks dirty");

        let mut c = Configuration::new();
        c.set_if_blank("a", "x");
        assert!(c.dirty());

        let mut c = Configuration::new();
        c.set("a", "x");
        let dirty_before = c.dirty();
        c.set_if_blank("a", "y");
        assert!(dirty_before && c.dirty());
        assert_eq!(c.get("a"), "x");

        // Direct map access (C++ operator[]) does not touch the flag.
        let mut c = Configuration::new();
        *c.value_mut("k") = "v".to_string();
        c.insert("k2".to_string(), "v2".to_string());
        assert!(!c.dirty());
        assert_eq!(c.get("k"), "v");
        assert_eq!(c.get("k2"), "v2");
    }

    #[test]
    fn set_if_blank_with_empty_value_autovivifies() {
        let mut c = Configuration::new();
        c.set_if_blank("missing", "");
        assert!(c.contains_key("missing"));
        assert_eq!(c.get("missing"), "");
        assert!(
            c.dirty(),
            "the value was blank, so the C++ assignment branch runs"
        );
    }

    #[test]
    fn load_and_save_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = write(dir.path(), "rc", "zeta=1\nalpha = two # trailing\n");

        let mut c = Configuration::new();
        c.load(&rc, 1, NO_SEARCH_PATHS).expect("load");
        assert_eq!(c.file(), rc);
        assert!(c.dirty());
        assert_eq!(c.all(), vec!["alpha".to_string(), "zeta".to_string()]);

        c.set("middle", 3);
        c.save();
        assert!(!c.dirty());
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read"),
            "alpha=two\nmiddle=3\nzeta=1\n"
        );
    }

    #[test]
    fn load_missing_or_empty_file_is_silent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope").to_str().expect("utf-8").to_string();

        let mut c = Configuration::new();
        c.load(&missing, 1, NO_SEARCH_PATHS)
            .expect("missing file is not an error");
        assert!(c.is_empty());
        assert!(!c.dirty());
        assert_eq!(
            c.file(),
            missing,
            "original file is remembered even if missing"
        );

        let empty = write(dir.path(), "empty", "");
        let mut c = Configuration::new();
        c.load(&empty, 1, NO_SEARCH_PATHS).expect("empty file");
        assert!(c.is_empty());
        assert!(!c.dirty());
    }

    #[test]
    fn load_at_nest_above_one_keeps_original_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = write(dir.path(), "a", "x=1\n");
        let b = write(dir.path(), "b", "y=2\n");

        let mut c = Configuration::new();
        c.load(&a, 1, NO_SEARCH_PATHS).expect("load a");
        c.load(&b, 2, NO_SEARCH_PATHS).expect("load b");
        assert_eq!(c.file(), a);
        assert_eq!(c.get("y"), "2");
    }

    #[test]
    fn iteration_is_sorted() {
        let mut c = Configuration::new();
        c.set("b", "2");
        c.set("a", "1");
        c.set("c", "3");
        let keys: Vec<&str> = (&c).into_iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
        let mut pairs = Vec::new();
        for (k, v) in &c {
            pairs.push(format!("{k}={v}"));
        }
        assert_eq!(pairs, vec!["a=1", "b=2", "c=3"]);
    }

    // ---------------------------------------------------------------------
    // Parse: comments, whitespace, values, malformed lines.

    #[test]
    fn parse_comments_and_whitespace() {
        let mut c = Configuration::new();
        c.parse(
            "#a=1\n   # indented comment\nb=2#c\nc = 3 4 \t\nd=\ne= # empty after comment\n\r\nf=x=y\n",
            1,
            NO_SEARCH_PATHS,
            "",
        )
        .expect("parse");
        assert!(!c.has("a"));
        assert_eq!(c.get("b"), "2");
        assert_eq!(c.get("c"), "3 4", "inner whitespace is kept");
        assert!(c.has("d"));
        assert_eq!(c.get("d"), "");
        assert_eq!(c.get("e"), "");
        assert_eq!(c.get("f"), "x=y", "split at the first '='");
        assert_eq!(c.len(), 5);
    }

    #[test]
    fn parse_later_values_override_earlier() {
        let mut c = Configuration::new();
        c.parse("a=1\na=2\n", 1, NO_SEARCH_PATHS, "")
            .expect("parse");
        assert_eq!(c.get("a"), "2");
    }

    #[test]
    fn parse_crlf_lines() {
        let mut c = Configuration::new();
        c.parse("a=1\r\nb=2\r\n", 1, NO_SEARCH_PATHS, "")
            .expect("parse");
        assert_eq!(c.get("a"), "1");
        assert_eq!(c.get("b"), "2");
    }

    #[test]
    fn parse_no_line_continuation() {
        let mut c = Configuration::new();
        // A trailing backslash is just a JSON-decoded backslash, and the next
        // line is parsed on its own.
        let err = message(c.parse("a=one\\\ntwo\n", 1, NO_SEARCH_PATHS, ""));
        assert_eq!(err, "Malformed entry 'two' in config file.");
        assert_eq!(c.get("a"), "one\\");
    }

    #[test]
    fn parse_malformed_lines() {
        let mut c = Configuration::new();
        let err = message(c.parse(
            "ok=1\n  just some words # comment\n",
            1,
            NO_SEARCH_PATHS,
            "",
        ));
        assert_eq!(err, "Malformed entry 'just some words' in config file.");
        assert_eq!(c.get("ok"), "1", "lines before the error were applied");
        assert!(
            !c.dirty(),
            "the error is raised before the dirty flag is set"
        );
    }

    #[test]
    fn parse_value_expansion() {
        let home = std::env::var("HOME").unwrap_or_default();
        std::env::set_var("TW_CONFIGURATION_VALUE_VAR", "expanded");
        let mut c = Configuration::new();
        c.parse(
            "tilde=~/x\nvar=pre/$TW_CONFIGURATION_VALUE_VAR\nmid=a~b\n",
            1,
            NO_SEARCH_PATHS,
            "",
        )
        .expect("parse");
        assert_eq!(c.get("tilde"), format!("{home}/x"));
        assert_eq!(c.get("var"), "pre/expanded");
        assert_eq!(c.get("mid"), "a~b", "~ only expands at the start");
    }

    #[test]
    fn parse_json_decodes_values() {
        let mut c = Configuration::new();
        c.parse(
            "a=x\\ty\nb=\\u0041\\u00e9\nc=\\\"q\\\"\nd=back\\\\slash\ne=\\/\\b\\f\\n\\r\nf=\\q\ng=end\\\nh=\\u41\n",
            1,
            NO_SEARCH_PATHS,
            "",
        )
        .expect("parse");
        assert_eq!(c.get("a"), "x\ty");
        assert_eq!(c.get("b"), "A\u{e9}");
        assert_eq!(c.get("c"), "\"q\"");
        assert_eq!(c.get("d"), "back\\slash");
        assert_eq!(
            c.get("e"),
            "/\u{8}\u{c}\n\r",
            "decoded after trimming, so nothing trims them"
        );
        assert_eq!(c.get("f"), "\\q", "unknown escapes are kept");
        assert_eq!(c.get("g"), "end\\", "trailing backslash is kept");
        assert_eq!(
            c.get("h"),
            "",
            "short \\u escape is code point 0, which is empty"
        );
    }

    #[test]
    fn json_decode_details() {
        assert_eq!(json::decode(""), "");
        assert_eq!(json::decode("plain"), "plain");
        assert_eq!(json::decode("\\n\\t"), "\n\t");
        assert_eq!(json::decode("\\u20acX"), "\u{20ac}X");
        assert_eq!(
            json::decode("\\uU+0041"),
            "A41",
            "the U+ form of utf8_codepoint, but only 4 bytes are skipped"
        );
        assert_eq!(
            json::decode("a\\\0b"),
            "a\\b",
            "embedded NUL after a backslash is dropped"
        );
    }

    // ---------------------------------------------------------------------
    // Typed getters and setters.

    #[test]
    fn get_integer_strtoimax_semantics() {
        let mut c = Configuration::new();
        for (key, value) in [
            ("lead", " \t42abc"),
            ("neg", "-7"),
            ("plus", "+5"),
            ("none", "abc"),
            ("real", "3.9"),
            ("big", "99999999999"),
            ("huge", "999999999999999999999999"),
        ] {
            c.set(key, value);
        }
        assert_eq!(c.get_integer("lead"), 42);
        assert_eq!(c.get_integer("neg"), -7);
        assert_eq!(c.get_integer("plus"), 5);
        assert_eq!(c.get_integer("none"), 0);
        assert_eq!(c.get_integer("real"), 3);
        assert_eq!(c.get_integer("missing"), 0);
        assert_eq!(
            c.get_integer("big"),
            99_999_999_999_i64 as i32,
            "narrowed to int"
        );
        assert_eq!(
            c.get_integer("huge"),
            i64::MAX as i32,
            "saturates, then narrows"
        );
    }

    #[test]
    fn get_real_strtod_semantics() {
        let mut c = Configuration::new();
        c.set("lead", "3.14xyz");
        c.set("exp", "1e3");
        c.set("dot", " .5");
        c.set("none", "x1");
        c.set("hex", "0x10");
        assert_eq!(c.get_real("lead"), 3.14);
        assert_eq!(c.get_real("exp"), 1000.0);
        assert_eq!(c.get_real("dot"), 0.5);
        assert_eq!(c.get_real("none"), 0.0);
        assert_eq!(c.get_real("hex"), 16.0);
        assert_eq!(c.get_real("missing"), 0.0);
    }

    #[test]
    fn get_boolean_accepted_values() {
        let mut c = Configuration::new();
        for (i, value) in ["true", "1", "y", "yes", "on", "TRUE", "Yes", "ON", "Y"]
            .iter()
            .enumerate()
        {
            c.set(&format!("t{i}"), *value);
            assert!(c.get_boolean(&format!("t{i}")), "{value} is true");
        }
        for (i, value) in ["false", "0", "no", "off", "2", "yes ", "t", "", "enabled"]
            .iter()
            .enumerate()
        {
            c.set(&format!("f{i}"), *value);
            assert!(!c.get_boolean(&format!("f{i}")), "{value:?} is false");
        }
        assert!(!c.get_boolean("missing"));
    }

    #[test]
    fn set_overload_formatting() {
        let mut c = Configuration::new();
        c.set("int", -42);
        c.set("real", 3.14);
        c.set("whole", 2.0);
        c.set("long", 1234567.891);
        c.set("tiny_half", 0.5);
        c.set("tiny_quarter", 0.25);
        c.set("big", 1.0e20);
        c.set("string", String::from("s"));
        let owned = String::from("r");
        c.set("ref", &owned);
        assert_eq!(c.get("int"), "-42");
        assert_eq!(c.get("real"), "3.14");
        assert_eq!(c.get("whole"), "2");
        assert_eq!(c.get("long"), "1234567.9");
        assert_eq!(
            c.get("tiny_half"),
            "1",
            "format (0.5, 1, 8) rounds to an integer"
        );
        assert_eq!(c.get("tiny_quarter"), "0");
        assert_eq!(c.get("big"), "1e+20");
        assert_eq!(c.get("string"), "s");
        assert_eq!(c.get("ref"), "r");
    }

    // ---------------------------------------------------------------------
    // Context-aware lookups.

    #[test]
    fn context_lookups() {
        let mut c = Configuration::new();
        c.set("color", "on");
        c.set("context", "work");
        c.set("context.work.rc.color", "off");
        c.set("context.work.rc.only", "ctx");
        c.set("context.work.rc.number", "7");
        c.set("number", "3");
        c.set("context.work.rc.flag", "yes");
        c.set("flag", "no");
        c.set("context.work.rc.real", "2.5");
        c.set("real", "1.5");

        assert_eq!(c.get("color"), "off");
        assert_eq!(c.get_from_context("color", false), "on");
        assert_eq!(c.get("only"), "ctx");
        assert_eq!(c.get_from_context("only", false), "");
        assert!(c.has("only"), "has always considers the context");
        assert_eq!(c.get_integer("number"), 7);
        assert_eq!(c.get_integer_from_context("number", false), 3);
        assert!(c.get_boolean("flag"));
        assert!(!c.get_boolean_from_context("flag", false));
        assert_eq!(c.get_real("real"), 2.5);
        assert_eq!(c.get_real_from_context("real", false), 1.5);

        // Keys starting with "context." are never redirected.
        c.set("context.work.rc.context.x", "redirected");
        c.set("context.x", "direct");
        assert_eq!(c.get("context.x"), "direct");

        // Without a context setting there is no redirection.
        c.remove("context");
        assert_eq!(c.get("color"), "on");
        assert!(!c.has("only"));
    }

    // ---------------------------------------------------------------------
    // Include resolution.

    #[test]
    fn include_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inc = write(dir.path(), "theme", "color.x=red\n");
        let mut c = Configuration::new();
        c.parse(&format!("include {inc}\nafter=1\n"), 1, NO_SEARCH_PATHS, "")
            .expect("parse");
        assert_eq!(c.get("color.x"), "red");
        assert_eq!(c.get("after"), "1");
        assert_eq!(c.file(), "", "an include does not become the original file");
    }

    #[test]
    fn include_absolute_missing_is_not_searched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir
            .path()
            .join("missing.theme")
            .to_str()
            .expect("utf-8")
            .to_string();
        let mut c = Configuration::new();
        let err = message(c.parse(&format!("include {missing}"), 1, NO_SEARCH_PATHS, ""));
        assert_eq!(err, format!("Could not read include file '{missing}'."));
    }

    #[test]
    fn include_relative_to_cwd() {
        // cargo runs unit tests from the package root; create the fixture
        // beneath it so a relative path resolves against the CWD.
        let dir = tempfile::tempdir_in(".").expect("tempdir in cwd");
        write(dir.path(), "cwd.conf", "from=cwd\n");
        let rel = format!(
            "{}/cwd.conf",
            dir.path()
                .file_name()
                .expect("name")
                .to_str()
                .expect("utf-8")
        );
        let mut c = Configuration::new();
        c.parse(&format!("include {rel}"), 1, NO_SEARCH_PATHS, "")
            .expect("parse");
        assert_eq!(c.get("from"), "cwd");
    }

    #[test]
    fn include_relative_to_config_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
        write(&dir.path().join("sub"), "theme", "from=file-dir\n");
        let rc = write(dir.path(), "rc", "include sub/theme\n");

        let mut c = Configuration::new();
        c.load(&rc, 1, NO_SEARCH_PATHS).expect("load");
        assert_eq!(c.get("from"), "file-dir");
    }

    #[test]
    fn include_relative_uses_real_path_of_config_file() {
        // The including file is a symlink: includes resolve next to its target.
        let real = tempfile::tempdir().expect("tempdir");
        let link = tempfile::tempdir().expect("tempdir");
        let target = write(real.path(), "rc", "include sibling.conf\n");
        write(real.path(), "sibling.conf", "from=target-dir\n");
        write(link.path(), "sibling.conf", "from=link-dir\n");
        let link_rc = link.path().join("rc");
        std::os::unix::fs::symlink(&target, &link_rc).expect("symlink");

        let mut c = Configuration::new();
        c.load(link_rc.to_str().expect("utf-8"), 1, NO_SEARCH_PATHS)
            .expect("load");
        assert_eq!(c.get("from"), "target-dir");
    }

    #[test]
    fn include_file_dir_wins_over_search_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let search = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "x.conf", "from=file-dir\n");
        write(search.path(), "x.conf", "from=search\n");
        let rc = write(dir.path(), "rc", "include x.conf\n");

        let mut c = Configuration::new();
        c.load(&rc, 1, &[search.path().to_str().expect("utf-8")])
            .expect("load");
        assert_eq!(c.get("from"), "file-dir");
    }

    #[test]
    fn include_search_paths_in_order() {
        let first = tempfile::tempdir().expect("tempdir");
        let second = tempfile::tempdir().expect("tempdir");
        write(second.path(), "only-second.conf", "which=second\n");
        write(first.path(), "both.conf", "both=first\n");
        write(second.path(), "both.conf", "both=second\n");
        let paths = vec![
            first.path().to_str().expect("utf-8").to_string(),
            second.path().to_str().expect("utf-8").to_string(),
        ];

        let mut c = Configuration::new();
        c.parse(
            "include only-second.conf\ninclude both.conf\n",
            1,
            &paths,
            "",
        )
        .expect("parse");
        assert_eq!(c.get("which"), "second");
        assert_eq!(
            c.get("both"),
            "first",
            "the first search path that has it wins"
        );
    }

    #[test]
    fn include_search_paths_propagate_to_nested_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let search = tempfile::tempdir().expect("tempdir");
        write(search.path(), "outer.conf", "include inner.conf\n");
        write(search.path(), "inner.conf", "deep=yes\n");
        let rc = write(dir.path(), "rc", "include outer.conf\n");

        let mut c = Configuration::new();
        c.load(&rc, 1, &[search.path().to_str().expect("utf-8")])
            .expect("load");
        assert_eq!(c.get("deep"), "yes");
    }

    #[test]
    fn include_tilde_and_variable_expansion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inc = write(dir.path(), "env.conf", "from=env\n");
        std::env::set_var("TW_CONFIGURATION_INC_VAR", &inc);
        let mut c = Configuration::new();
        c.parse("include $TW_CONFIGURATION_INC_VAR", 1, NO_SEARCH_PATHS, "")
            .expect("parse");
        assert_eq!(c.get("from"), "env");

        // `~` expands to $HOME; the included path is absolute afterwards.
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() && std::path::Path::new(&home).is_dir() {
            let name = "~/tw-configuration-test-surely-missing.conf";
            let mut c = Configuration::new();
            let err = message(c.parse(&format!("include {name}"), 1, NO_SEARCH_PATHS, ""));
            assert_eq!(
                err,
                format!("Could not read include file '{home}/tw-configuration-test-surely-missing.conf'.")
            );
        }
    }

    #[test]
    fn include_missing_relative_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = write(dir.path(), "rc", "include nowhere.conf # comment\n");
        let mut c = Configuration::new();
        let err = message(c.load(&rc, 1, &[dir.path().to_str().expect("utf-8")]));
        assert_eq!(
            err,
            "Could not find file in CWD, directory of config file or search paths 'nowhere.conf'."
        );
    }

    #[test]
    fn include_keyword_anywhere_in_line() {
        // C++ finds "include" anywhere in a line without '='.
        let dir = tempfile::tempdir().expect("tempdir");
        let inc = write(dir.path(), "x.conf", "k=v\n");
        let mut c = Configuration::new();
        c.parse(&format!("please include {inc}"), 1, NO_SEARCH_PATHS, "")
            .expect("parse");
        assert_eq!(c.get("k"), "v");

        let mut c = Configuration::new();
        let err = message(c.parse("include", 1, NO_SEARCH_PATHS, ""));
        assert_eq!(
            err, "Could not find file in CWD, directory of config file or search paths ''.",
            "empty name"
        );
    }

    #[test]
    fn include_unreadable_file() {
        // Root bypasses permission bits, so only check when access() denies.
        let dir = tempfile::tempdir().expect("tempdir");
        let inc = write(dir.path(), "locked.conf", "k=v\n");
        std::fs::set_permissions(&inc, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        if !Path::new(&inc).readable().expect("access") {
            let mut c = Configuration::new();
            let err = message(c.parse(&format!("include {inc}"), 1, NO_SEARCH_PATHS, ""));
            assert_eq!(err, format!("Could not read include file '{inc}'."));
        }
    }

    #[test]
    fn include_nesting_limit() {
        // A file that includes itself recurses until the 11th level.
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = write(dir.path(), "loop.conf", "include loop.conf\n");
        let mut c = Configuration::new();
        let err = message(c.load(&rc, 1, NO_SEARCH_PATHS));
        assert_eq!(err, "Configuration files may only be nested to 10 levels.");

        // A chain exactly 10 files deep is accepted; 11 is not.
        let chain = tempfile::tempdir().expect("tempdir");
        for level in 1..=11 {
            let body = if level == 11 {
                "bottom=reached\n".to_string()
            } else {
                format!("level{level}=1\ninclude f{}.conf\n", level + 1)
            };
            write(chain.path(), &format!("f{level}.conf"), &body);
        }
        let mut c = Configuration::new();
        let err = message(c.load(
            chain.path().join("f1.conf").to_str().expect("utf-8"),
            1,
            NO_SEARCH_PATHS,
        ));
        assert_eq!(err, "Configuration files may only be nested to 10 levels.");
        assert_eq!(c.get("level10"), "1");

        let mut c = Configuration::new();
        c.load(
            chain.path().join("f2.conf").to_str().expect("utf-8"),
            1,
            NO_SEARCH_PATHS,
        )
        .expect("ten levels");
        assert_eq!(c.get("bottom"), "reached");

        // Direct call above the limit.
        let mut c = Configuration::new();
        let err = message(c.load(&rc, 11, NO_SEARCH_PATHS));
        assert_eq!(err, "Configuration files may only be nested to 10 levels.");
    }

    // ---------------------------------------------------------------------
    // setVariableInFile / unsetVariableInFile.

    #[test]
    fn set_variable_in_file_replaces_appends_and_keeps_comments() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = write(dir.path(), "rc", "a=1 # keep me\n# a=commented\nb=2\n");

        assert!(set_variable_in_file(&rc, "a", "3"));
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read"),
            "a=3 # keep me\n# a=commented\nb=2\n"
        );

        assert!(set_variable_in_file(&rc, "c", "new"));
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read"),
            "a=3 # keep me\n# a=commented\nb=2\nc=new\n"
        );

        // Substring match, as in C++.
        assert!(set_variable_in_file(&rc, "b", "9"));
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read"),
            "a=3 # keep me\n# a=commented\nb=9\nc=new\n"
        );
        let rc2 = write(dir.path(), "rc2", "xb=1\n");
        assert!(set_variable_in_file(&rc2, "b", "2"));
        assert_eq!(std::fs::read_to_string(&rc2).expect("read"), "b=2\n");

        // A missing file is created.
        let fresh = dir
            .path()
            .join("fresh")
            .to_str()
            .expect("utf-8")
            .to_string();
        assert!(set_variable_in_file(&fresh, "k", "v"));
        assert_eq!(std::fs::read_to_string(&fresh).expect("read"), "k=v\n");
    }

    #[test]
    fn unset_variable_in_file_removes_matching_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = write(dir.path(), "rc", "a=1\n# a=commented\nb=2 # c\na=again\n");

        assert!(unset_variable_in_file(&rc, "a"));
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read"),
            "# a=commented\nb=2 # c\n"
        );

        assert!(!unset_variable_in_file(&rc, "zzz"));
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read"),
            "# a=commented\nb=2 # c\n"
        );

        assert!(unset_variable_in_file(&rc, "b"));
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read"),
            "# a=commented\n"
        );
    }
}
