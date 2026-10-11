//! Rust translation of `libshared src/JSON.h`, `src/JSON.cpp` and
//! `src/SAX.cpp` (namespace `json`).
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6
//! (`d899eba6d1a863f78f15e5e82d45d857278e9471`).
//!
//! # Contents
//!
//! - **Value tree** (`JSON.cpp`). The C++ class hierarchy `json::value` /
//!   `string` / `number` / `literal` / `array` / `object` is the enum
//!   [`JsonValue`] (milestone 2 Design Decision 4, approach 1). Objects are a
//!   `BTreeMap`, which iterates in the same sorted key order as the C++
//!   `std::map`. [`JType`] is `json::jtype`, and [`LiteralValue`] is
//!   `json::literal::literal_value`.
//! - **Parsers**: [`parse`] (`json::parse (const std::string&)`) and the
//!   per-type `parse (Pig&)` functions [`parse_value`], [`parse_object`],
//!   [`parse_pair`], [`parse_array`], [`parse_string`], [`parse_number`] and
//!   [`parse_literal`]. They are built on [`Pig`] exactly like the C++. Each
//!   C++ `throw format (...)` is a `TwError::Message` with the same text.
//! - **Dump**: [`JsonValue::dump`] (each C++ `dump () const`).
//! - **Escaping**: [`encode`] and [`decode`] (`json::encode`/`json::decode`).
//! - **SAX parser** (`SAX.cpp`): [`Sax`] (`json::SAX`) with its event
//!   interface [`SaxSink`] (`json::SAX::Sink`).
//!
//! # C++ behaviour preserved
//!
//! - String values (and object keys) are stored **raw**: the text between
//!   the quotes as `Pig::getQuoted` returns it, with escapes such as `\"` or
//!   `\u20ac` still in place. `dump` writes them back unchanged between
//!   quotes. Callers [`decode`] them when they need the text, as Taskwarrior
//!   does.
//! - Numbers are `f64`. `dump` uses the non-template C++ `format (double)`,
//!   which is `std::fixed` with precision 6, so `1` dumps as `1.000000`.
//! - Duplicate object keys: the C++ uses `std::map::insert`, which keeps
//!   the **first** value. The later duplicates are parsed and then
//!   discarded.
//! - Error positions are byte offsets into the input (`Pig::cursor`).

use std::collections::BTreeMap;

use crate::error::{Result, TwError};
use crate::shared::format::{format, format_value};
use crate::shared::pig::Pig;
use crate::shared::utf8::utf8_character;

////////////////////////////////////////////////////////////////////////////////
/// The non-identity entries of the C++ `json_encode[]` table: the escape
/// sequence for each byte `encode` translates. Every other byte maps to
/// itself.
fn json_encode(c: u8) -> Option<&'static str> {
    match c {
        b'"' => Some("\\\""),
        b'\\' => Some("\\\\"),
        b'/' => Some("\\/"),
        0x08 => Some("\\b"),
        0x0c => Some("\\f"),
        b'\n' => Some("\\n"),
        b'\r' => Some("\\r"),
        b'\t' => Some("\\t"),
        _ => None,
    }
}

////////////////////////////////////////////////////////////////////////////////
/// libshared `json::encode`: escapes `"`, `\`, `/`, backspace, form feed,
/// newline, carriage return and tab as `\"`, `\\`, `\/`, `\b`, `\f`, `\n`,
/// `\r` and `\t`. Everything else (including other control characters and
/// non-ASCII text) is copied unchanged.
pub fn encode(input: &str) -> String {
    let mut output = String::with_capacity((input.len() * 6) / 5); // 20% increase.

    let mut last = 0;
    for (i, c) in input.bytes().enumerate() {
        // Simple translations; default NOP.
        if let Some(escaped) = json_encode(c) {
            output.push_str(&input[last..i]);
            output.push_str(escaped);
            last = i + 1;
        }
    }

    output.push_str(&input[last..]);

    output
}

////////////////////////////////////////////////////////////////////////////////
/// Value of the hex digit `x`; non-hex digits count as 0 (libshared
/// `utf8.cpp` `XDIGIT`).
fn xdigit(x: u8) -> u32 {
    match x {
        b'0'..=b'9' => (x - b'0') as u32,
        b'a'..=b'f' => (x + 10 - b'a') as u32,
        b'A'..=b'F' => (x + 10 - b'A') as u32,
        _ => 0,
    }
}

/// libshared `utf8_codepoint` on raw bytes (the C++ argument is a byte
/// `substr` that need not start on a UTF-8 boundary). Same rules as
/// [`crate::shared::utf8::utf8_codepoint`].
fn utf8_codepoint_bytes(input: &[u8]) -> u32 {
    let length = input.len();
    if length >= 6
        && ((input[0] == b'U' && input[1] == b'+') || (input[0] == b'\\' && input[1] == b'u'))
    {
        xdigit(input[2]) << 12 | xdigit(input[3]) << 8 | xdigit(input[4]) << 4 | xdigit(input[5])
    } else if length >= 4 {
        xdigit(input[0]) << 12 | xdigit(input[1]) << 8 | xdigit(input[2]) << 4 | xdigit(input[3])
    } else {
        0
    }
}

/// libshared `json::decode`: undoes [`encode`] and also composes `\uXXXX`
/// (or `\uU+XXXX`, of which only 4 bytes are skipped) into a UTF-8
/// character. Unrecognised escapes are kept verbatim, and a trailing `\`
/// (or one followed by NUL) stays a `\`.
///
/// Works on bytes like the C++; byte sequences that are not valid UTF-8
/// afterwards (only possible when a `\u` escape swallows part of a
/// multi-byte character) are replaced by U+FFFD.
pub fn decode(input: &str) -> String {
    let input = input.as_bytes();
    let mut output: Vec<u8> = Vec::with_capacity(input.len()); // Same size.

    let mut pos = 0;
    while pos < input.len() {
        if input[pos] == b'\\' {
            pos += 1;
            // End-of-string reads as NUL, as in C++11.
            let c = input.get(pos).copied().unwrap_or(0);
            match c {
                // Simple translations.
                b'"' => output.push(b'"'),
                b'\\' => output.push(b'\\'),
                b'/' => output.push(b'/'),
                b'b' => output.push(0x08),
                b'f' => output.push(0x0c),
                b'n' => output.push(b'\n'),
                b'r' => output.push(b'\r'),
                b't' => output.push(b'\t'),

                // Compose a UTF8 unicode character.
                b'u' => {
                    pos += 1;
                    let rest = &input[pos.min(input.len())..];
                    output.extend_from_slice(utf8_character(utf8_codepoint_bytes(rest)).as_bytes());
                    pos += 3;
                }

                // End-of-string will always have a NUL in C++11.
                0 => output.push(b'\\'),

                // If it is an unrecognized sequence, do nothing.
                other => {
                    output.push(b'\\');
                    output.push(other);
                }
            }
            pos += 1;
        } else {
            let next_backslash = input[pos..]
                .iter()
                .position(|&b| b == b'\\')
                .map_or(input.len(), |p| pos + p);
            output.extend_from_slice(&input[pos..next_backslash]);
            pos = next_backslash;
        }
    }

    match String::from_utf8(output) {
        Ok(text) => text,
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Value tree (JSON.cpp)

/// C++ `json::jtype`: the type tag returned by `value::type ()`. The
/// discriminants match the C++ enumerators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JType {
    /// `j_value` (0): the abstract base `json::value`. No parser ever
    /// produces one, so no [`JsonValue`] has this type.
    Value = 0,
    /// `j_object` (1).
    Object = 1,
    /// `j_array` (2).
    Array = 2,
    /// `j_string` (3).
    String = 3,
    /// `j_number` (4).
    Number = 4,
    /// `j_literal` (5).
    Literal = 5,
}

/// C++ `json::literal::literal_value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LiteralValue {
    /// `none`: what the `json::literal ()` constructor sets. It dumps as
    /// `true`, because the C++ `dump` falls through to its `else`.
    #[default]
    None,
    /// `nullvalue`: `null`.
    Null,
    /// `falsevalue`: `false`.
    False,
    /// `truevalue`: `true`.
    True,
}

/// One node of the parsed JSON tree. The C++ uses `json::value*` with
/// subclasses, and Rust uses an enum instead (milestone 2 Design
/// Decision 4).
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    /// `json::object`: `std::map <std::string, value*> _data`. The keys are
    /// raw (still escaped), as `Pig::getQuoted` returns them.
    Object(BTreeMap<String, JsonValue>),
    /// `json::array`: `std::vector <value*> _data`.
    Array(Vec<JsonValue>),
    /// `json::string`: `std::string _data`, the raw (still escaped) text
    /// between the quotes.
    String(String),
    /// `json::number`: `double _dvalue`.
    Number(f64),
    /// `json::literal`: `literal_value _lvalue`.
    Literal(LiteralValue),
}

impl JsonValue {
    /// C++ `json::string::string (const std::string&)`: a string node holding
    /// `other` as its (raw) `_data`.
    pub fn new_string(other: &str) -> Self {
        JsonValue::String(other.to_string())
    }

    /// C++ `virtual jtype type ()`.
    pub fn jtype(&self) -> JType {
        match self {
            JsonValue::Object(_) => JType::Object,
            JsonValue::Array(_) => JType::Array,
            JsonValue::String(_) => JType::String,
            JsonValue::Number(_) => JType::Number,
            JsonValue::Literal(_) => JType::Literal,
        }
    }

    /// C++ `virtual std::string dump () const`, for each subclass:
    ///
    /// - object: `{"key":value,...}` in key order, keys written raw;
    /// - array: `[value,...]`;
    /// - string: `"` + raw `_data` + `"` (nothing is escaped);
    /// - number: `format (_dvalue)`, i.e. `std::fixed` precision 6;
    /// - literal: `null`, `false`, else `true` (also for `none`).
    pub fn dump(&self) -> String {
        match self {
            JsonValue::Object(data) => {
                let mut output = String::new();
                output.push('{');

                for (i, (name, value)) in data.iter().enumerate() {
                    if i != 0 {
                        output.push(',');
                    }

                    output.push('"');
                    output.push_str(name);
                    output.push_str("\":");
                    output.push_str(&value.dump());
                }

                output.push('}');
                output
            }

            JsonValue::Array(data) => {
                let mut output = String::new();
                output.push('[');

                for (i, value) in data.iter().enumerate() {
                    if i != 0 {
                        output.push(',');
                    }

                    output.push_str(&value.dump());
                }

                output.push(']');
                output
            }

            JsonValue::String(data) => format!("\"{data}\""),

            // C++ `format (_dvalue)`: the non-template `format (double)`.
            JsonValue::Number(dvalue) => format_value(dvalue),

            JsonValue::Literal(lvalue) => match lvalue {
                LiteralValue::Null => "null".to_string(),
                LiteralValue::False => "false".to_string(),
                LiteralValue::None | LiteralValue::True => "true".to_string(),
            },
        }
    }

    // Accessors: the Rust form of the C++ `type () == j_x` check followed by
    // a cast to `json::x*`. None of them converts anything.

    /// The `_data` map of an object node (C++ `((json::object*)v)->_data`).
    pub fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(data) => Some(data),
            _ => None,
        }
    }

    /// Mutable form of [`JsonValue::as_object`].
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(data) => Some(data),
            _ => None,
        }
    }

    /// Consumes an object node and returns its `_data` map.
    pub fn into_object(self) -> Option<BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(data) => Some(data),
            _ => None,
        }
    }

    /// The `_data` vector of an array node (C++ `((json::array*)v)->_data`).
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(data) => Some(data),
            _ => None,
        }
    }

    /// Mutable form of [`JsonValue::as_array`].
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Array(data) => Some(data),
            _ => None,
        }
    }

    /// Consumes an array node and returns its `_data` vector.
    pub fn into_array(self) -> Option<Vec<JsonValue>> {
        match self {
            JsonValue::Array(data) => Some(data),
            _ => None,
        }
    }

    /// The raw (still escaped) `_data` of a string node (C++
    /// `((json::string*)v)->_data`). Use [`decode`] to unescape it.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(data) => Some(data),
            _ => None,
        }
    }

    /// The `_dvalue` of a number node (C++ `json::number::operator double`).
    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsonValue::Number(dvalue) => Some(*dvalue),
            _ => None,
        }
    }

    /// The `_lvalue` of a literal node.
    pub fn as_literal(&self) -> Option<LiteralValue> {
        match self {
            JsonValue::Literal(lvalue) => Some(*lvalue),
            _ => None,
        }
    }

    /// For an object node, the member called `name`. This is C++
    /// `_data.find (name)`, so it never inserts, unlike `_data[name]`.
    /// Other node types return `None`.
    pub fn get(&self, name: &str) -> Option<&JsonValue> {
        self.as_object().and_then(|data| data.get(name))
    }
}

/// C++ `json::value::parse (Pig&)`: tries object, array, string, number and
/// literal, in that order. Returns `Ok (None)` (C++ `nullptr`) if none
/// matches.
pub fn parse_value(pig: &mut Pig) -> Result<Option<JsonValue>> {
    if let Some(v) = parse_object(pig)? {
        return Ok(Some(v));
    }
    if let Some(v) = parse_array(pig)? {
        return Ok(Some(v));
    }
    if let Some(v) = parse_string(pig) {
        return Ok(Some(v));
    }
    if let Some(v) = parse_number(pig) {
        return Ok(Some(v));
    }
    if let Some(v) = parse_literal(pig) {
        return Ok(Some(v));
    }

    Ok(None)
}

/// C++ `json::string::parse (Pig&)`: a `"`-quoted string. Escapes are kept
/// raw (`Pig::getQuoted`), and whitespace is not skipped first.
pub fn parse_string(pig: &mut Pig) -> Option<JsonValue> {
    let mut value = String::new();
    if pig.get_quoted('"', &mut value) {
        return Some(JsonValue::String(value));
    }

    None
}

/// C++ `json::number::parse (Pig&)`: `Pig::getNumber (double&)`.
pub fn parse_number(pig: &mut Pig) -> Option<JsonValue> {
    let mut d = 0.0;
    if pig.get_number_f64(&mut d) {
        return Some(JsonValue::Number(d));
    }

    None
}

/// C++ `json::literal::parse (Pig&)`: `null`, `false` or `true`.
pub fn parse_literal(pig: &mut Pig) -> Option<JsonValue> {
    if pig.skip_literal("null") {
        Some(JsonValue::Literal(LiteralValue::Null))
    } else if pig.skip_literal("false") {
        Some(JsonValue::Literal(LiteralValue::False))
    } else if pig.skip_literal("true") {
        Some(JsonValue::Literal(LiteralValue::True))
    } else {
        None
    }
}

/// C++ `json::array::parse (Pig&)`: `[ value, ... ]`, with whitespace
/// allowed around the tokens.
///
/// Returns `Ok (None)` with the cursor restored if there is no `[`. Once the
/// `[` is consumed, it either returns the array or fails with
/// `Error: missing value after ',' at position N` or
/// `Error: missing ']' at position N`.
pub fn parse_array(pig: &mut Pig) -> Result<Option<JsonValue>> {
    let checkpoint = pig.cursor();

    pig.skip_ws();
    if pig.skip('[') {
        pig.skip_ws();

        let mut arr = Vec::new();

        if let Some(value) = parse_value(pig)? {
            arr.push(value);
            pig.skip_ws();
            while pig.skip(',') {
                pig.skip_ws();

                if let Some(value) = parse_value(pig)? {
                    arr.push(value);
                    pig.skip_ws();
                } else {
                    return Err(TwError::Message(format(
                        "Error: missing value after ',' at position {1}",
                        &[&(pig.cursor() as i32)],
                    )));
                }
            }
        }

        if pig.skip(']') {
            return Ok(Some(JsonValue::Array(arr)));
        } else {
            return Err(TwError::Message(format(
                "Error: missing ']' at position {1}",
                &[&(pig.cursor() as i32)],
            )));
        }
    }

    pig.restore_to(checkpoint);
    Ok(None)
}

/// C++ `json::object::parse (Pig&)`: `{ "name" : value, ... }`, with
/// whitespace allowed around the tokens.
///
/// Returns `Ok (None)` with the cursor restored if there is no `{`. Once the
/// `{` is consumed, it either returns the object or fails with
/// `Error: missing value after ',' at position N`, `Error: missing '}' at
/// position N`, or one of the [`parse_pair`] errors. A duplicate name keeps
/// its first value (C++ `std::map::insert`).
pub fn parse_object(pig: &mut Pig) -> Result<Option<JsonValue>> {
    let checkpoint = pig.cursor();

    pig.skip_ws();
    if pig.skip('{') {
        pig.skip_ws();

        let mut obj: BTreeMap<String, JsonValue> = BTreeMap::new();

        let mut name = String::new();
        let mut value: Option<JsonValue> = None;
        if parse_pair(pig, &mut name, &mut value)? {
            if let Some(value) = value.take() {
                obj.entry(name.clone()).or_insert(value);
            }

            pig.skip_ws();
            while pig.skip(',') {
                pig.skip_ws();

                if parse_pair(pig, &mut name, &mut value)? {
                    if let Some(value) = value.take() {
                        obj.entry(name.clone()).or_insert(value);
                    }
                    pig.skip_ws();
                } else {
                    return Err(TwError::Message(format(
                        "Error: missing value after ',' at position {1}",
                        &[&(pig.cursor() as i32)],
                    )));
                }
            }
        }

        if pig.skip('}') {
            return Ok(Some(JsonValue::Object(obj)));
        } else {
            return Err(TwError::Message(format(
                "Error: missing '}' at position {1}",
                &[&(pig.cursor() as i32)],
            )));
        }
    }

    pig.restore_to(checkpoint);
    Ok(None)
}

/// C++ `json::object::parse_pair (Pig&, std::string& name, value*& val)`:
/// `"name" : value`.
///
/// Returns `Ok (false)` with the cursor restored, and `name`/`val`
/// untouched, if there is no quoted name at the cursor. After a name it
/// either sets `name` and `val` and returns `Ok (true)`, or fails with
/// `Error: missing ':' at position N` or `Error: missing value at position N`.
/// As in C++, `name` is already overwritten when one of those errors occurs.
pub fn parse_pair(pig: &mut Pig, name: &mut String, val: &mut Option<JsonValue>) -> Result<bool> {
    let checkpoint = pig.cursor();

    if pig.get_quoted('"', name) {
        pig.skip_ws();
        if pig.skip(':') {
            pig.skip_ws();
            if let Some(value) = parse_value(pig)? {
                *val = Some(value);
                return Ok(true);
            } else {
                return Err(TwError::Message(format(
                    "Error: missing value at position {1}",
                    &[&(pig.cursor() as i32)],
                )));
            }
        } else {
            return Err(TwError::Message(format(
                "Error: missing ':' at position {1}",
                &[&(pig.cursor() as i32)],
            )));
        }
    }

    pig.restore_to(checkpoint);
    Ok(false)
}

/// C++ `json::parse (const std::string&)`: the parser entry point. The
/// document must be an object or an array, with optional surrounding
/// whitespace.
///
/// Errors (all `TwError::Message`):
/// - `Error: expected '{' or '[' at position N`
/// - `Error: extra characters found at position N`
/// - any error from [`parse_object`], [`parse_array`] or [`parse_pair`].
///
/// The C++ returns a `value*` and Taskwarrior checks `if (root && ...)`.
/// It is never null in practice: once the first character is `{` or `[`,
/// the object or array parser either succeeds or throws. So this function
/// returns the value itself.
pub fn parse(input: &str) -> Result<JsonValue> {
    let mut n = Pig::new(input);
    n.skip_ws();

    let root = if n.peek() == '{' as i32 {
        parse_object(&mut n)?
    } else if n.peek() == '[' as i32 {
        parse_array(&mut n)?
    } else {
        return Err(TwError::Message(format(
            "Error: expected '{' or '[' at position {1}",
            &[&(n.cursor() as i32)],
        )));
    };

    // Check for end condition.
    n.skip_ws();
    if !n.eos() {
        return Err(TwError::Message(format(
            "Error: extra characters found at position {1}",
            &[&(n.cursor() as i32)],
        )));
    }

    // Unreachable `None`: see above. The C++ would return nullptr here.
    root.ok_or_else(|| TwError::Message("Error: expected '{' or '[' at position 0".to_string()))
}

////////////////////////////////////////////////////////////////////////////////
// SAX parser (SAX.cpp)

/// C++ `json::SAX::Sink`: receives the [`Sax`] parse events. Every method
/// does nothing by default, as in the C++ base class.
pub trait SaxSink {
    /// `eventDocStart ()`.
    fn event_doc_start(&mut self) {}
    /// `eventDocEnd ()`.
    fn event_doc_end(&mut self) {}
    /// `eventObjectStart ()`.
    fn event_object_start(&mut self) {}
    /// `eventObjectEnd (int)`: called with the number of pairs.
    fn event_object_end(&mut self, _count: i32) {}
    /// `eventArrayStart ()`.
    fn event_array_start(&mut self) {}
    /// `eventArrayEnd (int)`: called with the number of elements.
    fn event_array_end(&mut self, _count: i32) {}
    /// `eventName (const std::string&)`: an object key, unescaped.
    fn event_name(&mut self, _value: &str) {}
    /// `eventValueNull ()`.
    fn event_value_null(&mut self) {}
    /// `eventValueBool (bool)`.
    fn event_value_bool(&mut self, _value: bool) {}
    /// `eventValueInt (int64_t)`.
    fn event_value_int(&mut self, _value: i64) {}
    /// `eventValueUint (uint64_t)`.
    fn event_value_uint(&mut self, _value: u64) {}
    /// `eventValueDouble (double)`.
    fn event_value_double(&mut self, _value: f64) {}
    /// `eventValueString (const std::string&)`: a string value, unescaped.
    fn event_value_string(&mut self, _value: &str) {}
}

/// C++ `json::SAX`: a recursive-descent JSON parser that reports events to a
/// [`SaxSink`] and builds no tree. Unlike the tree parser, it unescapes
/// strings and tells integers apart from doubles.
///
/// # `errno`
///
/// `SAX::isNumber` checks `errno != ERANGE` after `strtol`/`strtoul`, but
/// it never resets `errno`. So once any conversion has overflowed, every
/// later number skips the integer events and is reported as a double. Rust
/// has no shared `errno` for this, so the sticky flag lives in the `Sax`
/// instance, starts clear, and is set by the same conversions that set
/// ERANGE in glibc. This also means `event_value_uint` only fires if that
/// flag is clear, and `strtol` did not overflow, which never happens for a
/// fully consumed integer. That is the C++ behaviour too.
#[derive(Debug, Default)]
pub struct Sax {
    /// Models the C++ `errno == ERANGE` state (see the type docs).
    erange: bool,
}

/// C++ `input[i]` on a `std::string`: the byte at `i`, or NUL past the end
/// (`input[length ()]` is NUL in C++11; further reads are undefined, and are
/// NUL here too).
fn at(input: &[u8], i: usize) -> u8 {
    input.get(i).copied().unwrap_or(0)
}

/// The byte at `i` read as a C++ `char` widened to `int` (signed on x86-64
/// Linux, unsigned on AArch64 Linux, as `std::ffi::c_char` is).
fn at_char(input: &[u8], i: usize) -> i32 {
    at(input, i) as std::ffi::c_char as i32
}

/// The `-?digits` prefix that `strtol`/`strtoul` convert (SAX numbers never
/// start with '+' or whitespace): (negative, digits).
fn integer_prefix(s: &str) -> (bool, &str) {
    let negative = s.starts_with('-');
    let body = if negative { &s[1..] } else { s };
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    (negative, &body[..digits])
}

/// C `strtol (s, &end, 10)` for SAX number text: (value, `!*end`, ERANGE).
fn c_strtol(s: &str) -> (i64, bool, bool) {
    let (negative, digits) = integer_prefix(s);
    let full = usize::from(negative) + digits.len() == s.len();
    let text = &s[..usize::from(negative) + digits.len()];
    match text.parse::<i64>() {
        Ok(value) => (value, full, false),
        Err(_) => (if negative { i64::MIN } else { i64::MAX }, full, true),
    }
}

/// C `strtoul (s, &end, 10)` for SAX number text: (value, `!*end`, ERANGE).
/// A leading '-' negates the converted value modulo 2^64, as in C.
fn c_strtoul(s: &str) -> (u64, bool, bool) {
    let (negative, digits) = integer_prefix(s);
    let full = usize::from(negative) + digits.len() == s.len();
    match digits.parse::<u64>() {
        Ok(value) => (
            if negative {
                value.wrapping_neg()
            } else {
                value
            },
            full,
            false,
        ),
        Err(_) => (u64::MAX, full, true),
    }
}

/// C `strtod (s, &end)` for SAX number text `int frac exp`: (value, `!*end`,
/// ERANGE).
///
/// `frac` may start with whitespace, because `isFrac` skips it before the
/// '.'. `strtod` then stops after the integer part. Otherwise the whole text
/// is a valid decimal float, which Rust parses with the same correct
/// rounding as glibc. glibc sets ERANGE on overflow to infinity and on
/// inexact underflow. Here that is a subnormal result, or zero from nonzero
/// digits. (A short decimal is never exactly subnormal.)
fn c_strtod(integer_part: &str, combined: &str, fraction_part: &str) -> (f64, bool, bool) {
    let prefix = if fraction_part.is_empty() || fraction_part.starts_with('.') {
        combined
    } else {
        integer_part
    };
    let full = prefix.len() == combined.len();
    let value = prefix.parse::<f64>().unwrap_or(0.0);
    let mantissa = prefix.split(['e', 'E']).next().unwrap_or("");
    let nonzero_digits = mantissa.bytes().any(|b| (b'1'..=b'9').contains(&b));
    let erange = value.is_infinite()
        || (value != 0.0 && value.abs() < f64::MIN_POSITIVE)
        || (value == 0.0 && nonzero_digits);
    (value, full, erange)
}

impl Sax {
    /// A parser with a clear `errno` state.
    pub fn new() -> Self {
        Sax::default()
    }

    /// C++ `json::SAX::parse (const std::string&, SAX::Sink&)`.
    ///
    /// Reports `event_doc_start` and then the document events. Returns
    /// `Ok (true)` after `event_doc_end` if the input is one object or array
    /// with optional surrounding whitespace. Otherwise it fails (C++ `throw`)
    /// with one of these, where N is the byte position:
    /// `Error: extra characters found at position N`,
    /// `Error: Missing '{' or '[' at position N`,
    /// `Error: Missing '}' at position N`, `Error: Missing ']' at position N`,
    /// `Error: Missing value at position N`, `Error: Missing ':' at position
    /// N`, `Error: Missing '"' at position N`. It never returns `Ok (false)`,
    /// just as the C++ `return false` follows a throw and cannot be reached.
    pub fn parse(&mut self, input: &str, sink: &mut dyn SaxSink) -> Result<bool> {
        let input = input.as_bytes();
        sink.event_doc_start();
        let mut cursor = 0;
        Self::ignore_whitespace(input, &mut cursor);
        if self.is_object(input, &mut cursor, sink)? || self.is_array(input, &mut cursor, sink)? {
            Self::ignore_whitespace(input, &mut cursor);
            if cursor < input.len() {
                return Err(Self::error(
                    "Error: extra characters found at position ",
                    cursor,
                ));
            }

            sink.event_doc_end();
            return Ok(true);
        }

        Err(Self::error(
            "Error: Missing '{' or '[' at position ",
            cursor,
        ))
    }

    /// C++ `SAX::ignoreWhitespace`: the "complete Unicode whitespace list"
    /// compared against single `char`s. Signed `char` (x86-64) can only
    /// match the ASCII entries. Unsigned `char` (AArch64) also matches the
    /// bytes 0x85 and 0xA0. The `c_char` conversion reproduces both.
    fn ignore_whitespace(input: &[u8], cursor: &mut usize) {
        let mut c = at_char(input, *cursor);
        while c == 0x0020 ||   // space Common  Separator, space
              c == 0x0009 ||   // Common  Other, control  HT, Horizontal Tab
              c == 0x000A ||   // Common  Other, control  LF, Line feed
              c == 0x000B ||   // Common  Other, control  VT, Vertical Tab
              c == 0x000C ||   // Common  Other, control  FF, Form feed
              c == 0x000D ||   // Common  Other, control  CR, Carriage return
              c == 0x0085 ||   // Common  Other, control  NEL, Next line
              c == 0x00A0 ||   // no-break space  Common  Separator, space
              c == 0x1680 ||   // ogham space mark  Ogham Separator, space
              c == 0x180E ||   // mongolian vowel separator Mongolian Separator, space
              c == 0x2000 ||   // en quad Common  Separator, space
              c == 0x2001 ||   // em quad Common  Separator, space
              c == 0x2002 ||   // en space  Common  Separator, space
              c == 0x2003 ||   // em space  Common  Separator, space
              c == 0x2004 ||   // three-per-em space  Common  Separator, space
              c == 0x2005 ||   // four-per-em space Common  Separator, space
              c == 0x2006 ||   // six-per-em space  Common  Separator, space
              c == 0x2007 ||   // figure space  Common  Separator, space
              c == 0x2008 ||   // punctuation space Common  Separator, space
              c == 0x2009 ||   // thin space  Common  Separator, space
              c == 0x200A ||   // hair space  Common  Separator, space
              c == 0x200B ||   // zero width space
              c == 0x200C ||   // zero width non-joiner
              c == 0x200D ||   // zero width joiner
              c == 0x2028 ||   // line separator  Common  Separator, line
              c == 0x2029 ||   // paragraph separator Common  Separator, paragraph
              c == 0x202F ||   // narrow no-break space Common  Separator, space
              c == 0x205F ||   // medium mathematical space Common  Separator, space
              c == 0x2060 ||   // word joiner
              c == 0x3000
        // ideographic space Common  Separator, space
        {
            *cursor += 1;
            c = at_char(input, *cursor);
        }
    }

    /// C++ `SAX::isObject`: object := '{' [<pair> [, <pair> ...]] '}'
    fn is_object(
        &mut self,
        input: &[u8],
        cursor: &mut usize,
        sink: &mut dyn SaxSink,
    ) -> Result<bool> {
        Self::ignore_whitespace(input, cursor);
        let backup = *cursor;

        if Self::is_literal(input, b'{', cursor) {
            sink.event_object_start();
            let mut counter = 0;

            if self.is_pair(input, cursor, sink)? {
                counter += 1;
                while Self::is_literal(input, b',', cursor) && self.is_pair(input, cursor, sink)? {
                    counter += 1;
                }
            }

            Self::ignore_whitespace(input, cursor);
            if Self::is_literal(input, b'}', cursor) {
                sink.event_object_end(counter);
                return Ok(true);
            } else {
                return Err(Self::error("Error: Missing '}' at position ", *cursor));
            }
        }

        *cursor = backup;
        Ok(false)
    }

    /// C++ `SAX::isArray`: array := '[' [<value> [, <value> ...]] ']'
    fn is_array(
        &mut self,
        input: &[u8],
        cursor: &mut usize,
        sink: &mut dyn SaxSink,
    ) -> Result<bool> {
        Self::ignore_whitespace(input, cursor);
        let backup = *cursor;

        if Self::is_literal(input, b'[', cursor) {
            sink.event_array_start();
            let mut counter = 0;

            if self.is_value(input, cursor, sink)? {
                counter += 1;
                while Self::is_literal(input, b',', cursor) && self.is_value(input, cursor, sink)? {
                    counter += 1;
                }
            }

            Self::ignore_whitespace(input, cursor);
            if Self::is_literal(input, b']', cursor) {
                sink.event_array_end(counter);
                return Ok(true);
            } else {
                return Err(Self::error("Error: Missing ']' at position ", *cursor));
            }
        }

        *cursor = backup;
        Ok(false)
    }

    /// C++ `SAX::isPair`: pair := <string> ':' <value>
    fn is_pair(
        &mut self,
        input: &[u8],
        cursor: &mut usize,
        sink: &mut dyn SaxSink,
    ) -> Result<bool> {
        Self::ignore_whitespace(input, cursor);
        let backup = *cursor;

        if Self::is_key(input, cursor, sink)? {
            if Self::is_literal(input, b':', cursor) {
                if self.is_value(input, cursor, sink)? {
                    return Ok(true);
                }

                return Err(Self::error("Error: Missing value at position ", *cursor));
            } else {
                return Err(Self::error("Error: Missing ':' at position ", *cursor));
            }
        }

        *cursor = backup;
        Ok(false)
    }

    /// C++ `SAX::isValue`: string, number, object, array, `true`/`false`, or
    /// `null`, tried in that order.
    fn is_value(
        &mut self,
        input: &[u8],
        cursor: &mut usize,
        sink: &mut dyn SaxSink,
    ) -> Result<bool> {
        Self::ignore_whitespace(input, cursor);

        Ok(Self::is_string(input, cursor, sink)?
            || self.is_number(input, cursor, sink)
            || self.is_object(input, cursor, sink)?
            || self.is_array(input, cursor, sink)?
            || Self::is_bool(input, cursor, sink)
            || Self::is_null(input, cursor, sink))
    }

    /// C++ `SAX::isKey`.
    fn is_key(input: &[u8], cursor: &mut usize, sink: &mut dyn SaxSink) -> Result<bool> {
        Self::ignore_whitespace(input, cursor);

        let mut value = String::new();
        if Self::is_string_value(input, cursor, &mut value)? {
            sink.event_name(&value);
            return Ok(true);
        }

        Ok(false)
    }

    /// C++ `SAX::isString`.
    fn is_string(input: &[u8], cursor: &mut usize, sink: &mut dyn SaxSink) -> Result<bool> {
        Self::ignore_whitespace(input, cursor);

        let mut value = String::new();
        if Self::is_string_value(input, cursor, &mut value)? {
            sink.event_value_string(&value);
            return Ok(true);
        }

        Ok(false)
    }

    /// C++ `SAX::isStringValue`: a `"`-quoted string, unescaped.
    ///
    /// Recognises `\uXXXX` (exactly four hex digits) and the escapes `\"`,
    /// `\'`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`, `\v`. Any other escaped byte
    /// stands for itself, so `\/` gives `/`. A string still unclosed at the
    /// end of the input (or at an embedded NUL) fails with
    /// `Error: Missing '"' at position N`. As in C++, a lone `\` at the end
    /// moves the cursor one byte past the end.
    ///
    /// The C++ builds the text byte by byte. Here a `\u` escape for a UTF-16
    /// surrogate becomes U+FFFD (see `utf8_character`). The byte sequence
    /// is otherwise always valid UTF-8, because unescaped bytes are copied
    /// in order.
    fn is_string_value(input: &[u8], cursor: &mut usize, value: &mut String) -> Result<bool> {
        let backup = *cursor;

        if Self::is_literal(input, b'"', cursor) {
            let mut word: Vec<u8> = Vec::new();
            loop {
                let c = at(input, *cursor);
                if c == 0 {
                    break;
                }

                // EOS.
                if c == b'"' {
                    *cursor += 1;
                    *value = String::from_utf8_lossy(&word).into_owned();
                    return Ok(true);
                }
                // Unicode \uXXXX codepoint.
                else if at(input, *cursor) == b'\\'
                    && at(input, *cursor + 1) == b'u'
                    && Self::is_hex_digit(at(input, *cursor + 2))
                    && Self::is_hex_digit(at(input, *cursor + 3))
                    && Self::is_hex_digit(at(input, *cursor + 4))
                    && Self::is_hex_digit(at(input, *cursor + 5))
                {
                    let codepoint = Self::hex_to_int4(
                        at(input, *cursor + 2),
                        at(input, *cursor + 3),
                        at(input, *cursor + 4),
                        at(input, *cursor + 5),
                    );
                    word.extend_from_slice(utf8_character(codepoint).as_bytes());
                    *cursor += 6;
                }
                // An escaped thing.
                else if c == b'\\' {
                    *cursor += 1;
                    let c = at(input, *cursor);
                    match c {
                        b'"' => word.push(0x22),
                        b'\'' => word.push(0x27),
                        b'\\' => word.push(0x5C),
                        b'b' => word.push(0x08),
                        b'f' => word.push(0x0C),
                        b'n' => word.push(0x0A),
                        b'r' => word.push(0x0D),
                        b't' => word.push(0x09),
                        b'v' => word.push(0x0B),

                        // This pass-through default case means that anything
                        // can be escaped harmlessly. In particular 'quote' is
                        // included, if it not one of the above characters.
                        other => word.push(other),
                    }
                    *cursor += 1;
                }
                // Ordinary character.
                else {
                    word.push(c);
                    *cursor += 1;
                }
            }

            return Err(Self::error("Error: Missing '\"' at position ", *cursor));
        }

        *cursor = backup;
        Ok(false)
    }

    /// C++ `SAX::isNumber`: number := <int> [<frac>] [<exp>]
    ///
    /// Reported as `event_value_int` if `strtol` takes the whole text (and
    /// `errno` is not ERANGE), else `event_value_uint` under the same
    /// conditions for `strtoul`, else `event_value_double` if `strtod` takes
    /// the whole text. Otherwise the cursor is restored and it returns false.
    fn is_number(&mut self, input: &[u8], cursor: &mut usize, sink: &mut dyn SaxSink) -> bool {
        Self::ignore_whitespace(input, cursor);
        let backup = *cursor;

        let mut integer_part = String::new();
        if Self::is_int(input, cursor, &mut integer_part) {
            let mut fractional_part = String::new();
            Self::is_frac(input, cursor, &mut fractional_part);

            let mut exponent_part = String::new();
            Self::is_exp(input, cursor, &mut exponent_part);

            // Does it fit in a long?
            let combined = format!("{integer_part}{fractional_part}{exponent_part}");
            let (long_value, full, erange) = c_strtol(&combined);
            self.erange |= erange;
            if full && !self.erange {
                sink.event_value_int(long_value);
                return true;
            }

            // Does it fit in an unsigned long?
            let (ulong_value, full, erange) = c_strtoul(&combined);
            self.erange |= erange;
            if full && !self.erange {
                sink.event_value_uint(ulong_value);
                return true;
            }

            // If the above fail, allow this one to be capped at imax.
            let (double_value, full, erange) = c_strtod(&integer_part, &combined, &fractional_part);
            self.erange |= erange;
            if full {
                sink.event_value_double(double_value);
                return true;
            }
        }

        *cursor = backup;
        false
    }

    /// C++ `SAX::isInt`: int := ['-'] <digits>. On failure, the cursor may
    /// stay past the '-' ("No restore necessary": `is_number` restores it).
    fn is_int(input: &[u8], cursor: &mut usize, value: &mut String) -> bool {
        let backup = *cursor;

        Self::is_literal(input, b'-', cursor);
        if Self::is_digits(input, cursor) {
            *value = String::from_utf8_lossy(&input[backup..*cursor]).into_owned();
            return true;
        }

        // No restore necessary.
        false
    }

    /// C++ `SAX::isFrac`: frac := '.' <digits>. As in C++, whitespace before
    /// the '.' is skipped and becomes part of `value`.
    fn is_frac(input: &[u8], cursor: &mut usize, value: &mut String) -> bool {
        let backup = *cursor;

        if Self::is_literal(input, b'.', cursor) && Self::is_digits(input, cursor) {
            *value = String::from_utf8_lossy(&input[backup..*cursor]).into_owned();
            return true;
        }

        *cursor = backup;
        false
    }

    /// C++ `SAX::isDigits`: digits := <digit> [<digit> ...]
    fn is_digits(input: &[u8], cursor: &mut usize) -> bool {
        let mut c = at(input, *cursor);
        if Self::is_dec_digit(c) {
            *cursor += 1;
            c = at(input, *cursor);

            while Self::is_dec_digit(c) {
                *cursor += 1;
                c = at(input, *cursor);
            }

            return true;
        }

        false
    }

    /// C++ `SAX::isDecDigit`: digit := 0x30 ('0') .. 0x39 ('9')
    fn is_dec_digit(c: u8) -> bool {
        (0x30..=0x39).contains(&c)
    }

    /// C++ `SAX::isHexDigit`: `0-9`, `a-f`, `A-F`.
    fn is_hex_digit(c: u8) -> bool {
        (0x30..=0x39).contains(&c) || (0x61..=0x66).contains(&c) || (0x41..=0x46).contains(&c)
    }

    /// C++ `SAX::isExp`: exp := <e> <digits>
    fn is_exp(input: &[u8], cursor: &mut usize, value: &mut String) -> bool {
        let backup = *cursor;

        if Self::is_e(input, cursor) && Self::is_digits(input, cursor) {
            *value = String::from_utf8_lossy(&input[backup..*cursor]).into_owned();
            return true;
        }

        *cursor = backup;
        false
    }

    /// C++ `SAX::isE`: e := e | e+ | e- | E | E+ | E-
    fn is_e(input: &[u8], cursor: &mut usize) -> bool {
        let c = at(input, *cursor);
        if c == b'e' || c == b'E' {
            *cursor += 1;
            let c = at(input, *cursor);

            if c == b'+' || c == b'-' {
                *cursor += 1;
            }

            return true;
        }

        false
    }

    /// C++ `SAX::isBool`: `true` or `false`, no word-boundary check.
    fn is_bool(input: &[u8], cursor: &mut usize, sink: &mut dyn SaxSink) -> bool {
        Self::ignore_whitespace(input, cursor);

        let c = *cursor;
        if at(input, c) == b't'
            && at(input, c + 1) == b'r'
            && at(input, c + 2) == b'u'
            && at(input, c + 3) == b'e'
        {
            *cursor += 4;
            sink.event_value_bool(true);
            return true;
        } else if at(input, c) == b'f'
            && at(input, c + 1) == b'a'
            && at(input, c + 2) == b'l'
            && at(input, c + 3) == b's'
            && at(input, c + 4) == b'e'
        {
            *cursor += 5;
            sink.event_value_bool(false);
            return true;
        }

        false
    }

    /// C++ `SAX::isNull`: `null`, no word-boundary check.
    fn is_null(input: &[u8], cursor: &mut usize, sink: &mut dyn SaxSink) -> bool {
        Self::ignore_whitespace(input, cursor);

        let c = *cursor;
        if at(input, c) == b'n'
            && at(input, c + 1) == b'u'
            && at(input, c + 2) == b'l'
            && at(input, c + 3) == b'l'
        {
            *cursor += 4;
            sink.event_value_null();
            return true;
        }

        false
    }

    /// C++ `SAX::isLiteral`: skips whitespace, then the byte `literal`.
    fn is_literal(input: &[u8], literal: u8, cursor: &mut usize) -> bool {
        Self::ignore_whitespace(input, cursor);

        if at(input, *cursor) == literal {
            *cursor += 1;
            return true;
        }

        false
    }

    /// C++ `SAX::hexToInt (int)`: '0'-'9' -> 0-9, 'A'-'F' -> 10-15, anything
    /// else as a lower-case hex digit ('a'-'f' -> 10-15).
    fn hex_to_int(c: u8) -> u32 {
        if (0x30..=0x39).contains(&c) {
            (c - 0x30) as u32
        } else if (0x41..=0x46).contains(&c) {
            (c - 0x41 + 10) as u32
        } else {
            (c as u32).wrapping_sub(0x61).wrapping_add(10)
        }
    }

    /// C++ `SAX::hexToInt (int, int, int, int)`: four hex digits as a 16-bit
    /// value.
    fn hex_to_int4(c0: u8, c1: u8, c2: u8, c3: u8) -> u32 {
        (Self::hex_to_int(c0) << 12)
            + (Self::hex_to_int(c1) << 8)
            + (Self::hex_to_int(c2) << 4)
            + Self::hex_to_int(c3)
    }

    /// C++ `SAX::error`: `message` followed by the cursor position, thrown.
    fn error(message: &str, cursor: usize) -> TwError {
        TwError::Message(format!("{message}{cursor}"))
    }
}

////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;

    // The encode/decode assertions of libshared `test/json.t.cpp`.
    #[test]
    fn json_t_encode_decode() {
        assert_eq!(
            encode("1\"2"),
            "1\\\"2",
            "json::encode <quote> -> <backslash><quote>"
        );
        assert_eq!(
            decode("1\\\"2"),
            "1\"2",
            "json::decode <backslash><quote> -> <quote>"
        );

        assert_eq!(
            encode("1/2"),
            "1\\/2",
            "json::encode <slash> -> <backslash><slash>"
        );
        assert_eq!(
            decode("1\\/2"),
            "1/2",
            "json::decode <backslash><slash> -> <slash>"
        );

        assert_eq!(
            encode("1\u{8}2"),
            "1\\b2",
            "json::encode <bell> -> <backslash><backslash><b>"
        );
        assert_eq!(
            decode("1\\b2"),
            "1\u{8}2",
            "json::decode <backslash><backslash><b> -> <bell>"
        );

        assert_eq!(
            encode("1\u{c}2"),
            "1\\f2",
            "json::encode <formfeed> -> <backslash><backslash><f>"
        );
        assert_eq!(
            decode("1\\f2"),
            "1\u{c}2",
            "json::decode <backslash><backslash><f> -> <formfeed>"
        );

        assert_eq!(
            encode("1\n2"),
            "1\\n2",
            "json::encode <newline> -> <backslash><newline>"
        );
        assert_eq!(
            decode("1\\n2"),
            "1\n2",
            "json::decode <backslash><newline> -> <newline>"
        );

        assert_eq!(
            encode("1\r2"),
            "1\\r2",
            "json::encode <cr> -> <backslash><r>"
        );
        assert_eq!(
            decode("1\\r2"),
            "1\r2",
            "json::decode <backslash><r> -> <cr>"
        );

        assert_eq!(
            encode("1\t2"),
            "1\\t2",
            "json::encode <tab> -> <backslash><t>"
        );
        assert_eq!(
            decode("1\\t2"),
            "1\t2",
            "json::decode <backslash><t> -> <tab>"
        );

        assert_eq!(
            encode("1\\2"),
            "1\\\\2",
            "json::encode <backslash> -> <backslash><backslash>"
        );
        assert_eq!(
            decode("1\\\\2"),
            "1\\2",
            "json::decode <backslash><backslash> -> <backslash>"
        );

        // C++ "1\x2" is '1' followed by the byte 0x02.
        assert_eq!(
            encode("1\x02"),
            "1\x02",
            "json::encode <backslash><x> -> <backslash><x>(NOP)"
        );
        assert_eq!(
            decode("1\x02"),
            "1\x02",
            "json::decode <backslash><x> -> <backslash><x>(NOP)"
        );

        assert_eq!(encode("1€2"), "1€2", "json::encode € -> €");
        assert_eq!(
            decode("1\\u20ac2"),
            "1€2",
            "json::decode <backslash>u20ac -> €"
        );

        let encoded = encode("one\\");
        assert_eq!(
            encoded, "one\\\\",
            "json::encode one<backslash> -> one<backslash><backslash>"
        );
        assert_eq!(encoded.len(), 5, "json::encode one<backslash> -> length 5");
        assert_eq!(
            encoded.as_bytes()[0],
            b'o',
            "json::encode one<backslash>[0] -> o"
        );
        assert_eq!(
            encoded.as_bytes()[1],
            b'n',
            "json::encode one<backslash>[1] -> n"
        );
        assert_eq!(
            encoded.as_bytes()[2],
            b'e',
            "json::encode one<backslash>[2] -> e"
        );
        assert_eq!(
            encoded.as_bytes()[3],
            b'\\',
            "json::encode one<backslash>[3] -> <backslash>"
        );
        assert_eq!(
            encoded.as_bytes()[4],
            b'\\',
            "json::encode one<backslash>[4] -> <backslash>"
        );

        assert_eq!(
            decode(&encoded),
            "one\\",
            "json::decode one<backslash><backslash> -> one<backslash>"
        );

        // Invalid input, trailing `\`.
        assert_eq!(
            decode("1\\"),
            "1\\",
            "json::decode <backslash> -> <backslash>"
        );

        // Embedded NUL. C++ "\0012" is the byte 0x01 followed by '2'.
        assert_eq!(decode("\x012"), "\x012", "json::decode <nul>12 -> <nul>12");

        // Escaped embedded NUL.
        assert_eq!(
            decode("\\\x012"),
            "\\\x012",
            "json::decode <backslash><nul>12 -> <backslash><nul>12"
        );
    }

    #[test]
    fn encode_only_touches_the_table_entries() {
        assert_eq!(encode(""), "");
        assert_eq!(encode("\x00\x01\x0b\x7f"), "\x00\x01\x0b\x7f");
        assert_eq!(
            encode("\"\\/\u{8}\u{c}\n\r\t"),
            "\\\"\\\\\\/\\b\\f\\n\\r\\t"
        );
        assert_eq!(encode("a\"b\"c"), "a\\\"b\\\"c");
        let all = "x\"\\/\u{8}\u{c}\n\r\ty € z";
        assert_eq!(decode(&encode(all)), all, "decode inverts encode");
    }

    #[test]
    fn decode_details() {
        assert_eq!(decode(""), "");
        assert_eq!(decode("plain"), "plain");
        assert_eq!(decode("\\u0041\\u00e9"), "A\u{e9}");
        assert_eq!(decode("\\q"), "\\q", "unknown escapes are kept");
        assert_eq!(
            decode("\\u41"),
            "",
            "short \\u escape is code point 0, which is empty"
        );
        assert_eq!(
            decode("\\uU+0041"),
            "A41",
            "the U+ form of utf8_codepoint, but only 4 bytes are skipped"
        );
        assert_eq!(
            decode("a\\\0b"),
            "a\\b",
            "embedded NUL after a backslash is dropped"
        );
    }

    ////////////////////////////////////////////////////////////////////////////
    // json.t.cpp: tree parser

    const POSITIVE_TESTS: &[&str] = &[
        "{}",
        "    {    }    ",
        "[]",
        "{\"one\":1}",
        "{\n\"one\"\n:\n1\n}\n",
        "  {  \"one\"  :  1  }  ",
        "{\"name\":123, \"array\":[1,2,3.4], \"object\":{\"m1\":\"v1\", \"m2\":\"v2\"}}",
        "{\"name\":\"value\",\"array\":[\"one\",\"two\"],\"object\":{\"name2\":123,\"literal\":false}}",
        concat!(
            "{\n",
            "\"ticket\": { \"type\":\"add\", \"client\":\"taskwarrior 2.x\"},\n",
            "\"auth\":   { \"user\":\"paul\", \"org\":\"gbf\", \"key\":\".........\",\n",
            "            \"locale\":\"en-US\" },\n",
            "\n",
            "\"add\":    { \"description\":\"Wash the dog\",\n",
            "            \"project\":\"home\",\n",
            "            \"due\":\"20101101T000000Z\" }\n",
            "}"
        ),
        concat!(
            "{",
            "\"ticket\":{",
            "\"type\":\"synch\",",
            "\"client\":\"taskd-test-suite 1.0\"",
            "},",
            "\"synch\":{",
            "\"user\":{",
            "\"data\":[",
            "{",
            "\"uuid\":\"11111111-1111-1111-1111-111111111111\",",
            "\"status\":\"pending\",",
            "\"description\":\"This is a test\",",
            "\"entry\":\"20110111T124000Z\"",
            "}",
            "],",
            "\"synch\":\"key\"",
            "}",
            "},",
            "\"auth\":{",
            "\"org\":\"gbf\",",
            "\"user\":\"Paul Beckingham\",",
            "\"key\":\"K\",",
            "\"locale\":\"en-US\"",
            "}",
            "}"
        ),
    ];

    const NEGATIVE_TESTS: &[&str] = &["", "{", "}", "[", "]", "foo", "[?]"];

    #[test]
    fn json_t_positive() {
        // C++: t.ok (root, "positive: " + test); t.diag (root->dump ());
        // and t.fail (e) on an exception.
        for test in POSITIVE_TESTS {
            match parse(test) {
                Ok(root) => {
                    let dump = root.dump();
                    assert!(!dump.is_empty(), "positive: {test}");
                }
                Err(e) => panic!("positive: {test}: {e}"),
            }
        }
    }

    #[test]
    fn json_t_negative() {
        // C++: t.is (root, nullptr, "negative: " + test), or t.pass (e) on
        // an exception. Every negative input throws.
        for test in NEGATIVE_TESTS {
            assert!(parse(test).is_err(), "negative: {test}");
        }
    }

    /// Messages of the exceptions `json_t_negative` passes on (the C++
    /// `t.pass (e)` text).
    #[test]
    fn json_t_negative_messages() {
        let expected = [
            ("", "Error: expected '{' or '[' at position 0"),
            ("{", "Error: missing '}' at position 1"),
            ("}", "Error: expected '{' or '[' at position 0"),
            ("[", "Error: missing ']' at position 1"),
            ("]", "Error: expected '{' or '[' at position 0"),
            ("foo", "Error: expected '{' or '[' at position 0"),
            ("[?]", "Error: missing ']' at position 1"),
        ];
        for (input, message) in expected {
            assert_eq!(
                parse(input).unwrap_err().to_string(),
                message,
                "negative: {input}"
            );
        }
    }

    /// The `t.diag (root->dump ())` output of the positive tests.
    #[test]
    fn json_t_positive_dumps() {
        let expected = [
            "{}",
            "{}",
            "[]",
            "{\"one\":1.000000}",
            "{\"one\":1.000000}",
            "{\"one\":1.000000}",
            "{\"array\":[1.000000,2.000000,3.400000],\"name\":123.000000,\"object\":{\"m1\":\"v1\",\"m2\":\"v2\"}}",
            "{\"array\":[\"one\",\"two\"],\"name\":\"value\",\"object\":{\"literal\":false,\"name2\":123.000000}}",
            "{\"add\":{\"description\":\"Wash the dog\",\"due\":\"20101101T000000Z\",\"project\":\"home\"},\
             \"auth\":{\"key\":\".........\",\"locale\":\"en-US\",\"org\":\"gbf\",\"user\":\"paul\"},\
             \"ticket\":{\"client\":\"taskwarrior 2.x\",\"type\":\"add\"}}",
            "{\"auth\":{\"key\":\"K\",\"locale\":\"en-US\",\"org\":\"gbf\",\"user\":\"Paul Beckingham\"},\
             \"synch\":{\"user\":{\"data\":[{\"description\":\"This is a test\",\"entry\":\"20110111T124000Z\",\
             \"status\":\"pending\",\"uuid\":\"11111111-1111-1111-1111-111111111111\"}],\"synch\":\"key\"}},\
             \"ticket\":{\"client\":\"taskd-test-suite 1.0\",\"type\":\"synch\"}}",
        ];
        for (test, dump) in POSITIVE_TESTS.iter().zip(expected) {
            assert_eq!(parse(test).unwrap().dump(), dump, "dump: {test}");
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    // Tree parser: C++ behaviour beyond json.t.cpp

    #[test]
    fn parse_error_messages() {
        let cases = [
            ("[1,]", "Error: missing value after ',' at position 3"),
            ("{\"a\"}", "Error: missing ':' at position 4"),
            ("{\"a\":}", "Error: missing value at position 5"),
            ("{\"a\":1,}", "Error: missing value after ',' at position 7"),
            ("{\"a\":1 \"b\":2}", "Error: missing '}' at position 7"),
            ("[1 2]", "Error: missing ']' at position 3"),
            ("{} x", "Error: extra characters found at position 3"),
            ("[][]", "Error: extra characters found at position 2"),
            ("[1e]", "Error: missing ']' at position 1"),
            ("[\"abc]", "Error: missing ']' at position 1"),
            ("  x", "Error: expected '{' or '[' at position 2"),
            ("\"a\"", "Error: expected '{' or '[' at position 0"),
            ("[[1,]]", "Error: missing value after ',' at position 4"),
            ("[{\"a\" 1}]", "Error: missing ':' at position 6"),
        ];
        for (input, message) in cases {
            match parse(input) {
                Ok(v) => panic!("{input} parsed as {}", v.dump()),
                Err(e) => assert_eq!(e.to_string(), message, "{input}"),
            }
        }
    }

    #[test]
    fn strings_are_stored_and_dumped_raw() {
        let root = parse(r#"["a\"b\u20ac\/\n", ""]"#).unwrap();
        let arr = root.as_array().unwrap();
        assert_eq!(
            arr[0].as_str(),
            Some(r#"a\"b\u20ac\/\n"#),
            "escapes are kept raw"
        );
        assert_eq!(arr[1].as_str(), Some(""), "empty string");
        assert_eq!(
            root.dump(),
            r#"["a\"b\u20ac\/\n",""]"#,
            "dump writes raw text back"
        );
        assert_eq!(
            decode(arr[0].as_str().unwrap()),
            "a\"b\u{20ac}/\n",
            "decode unescapes"
        );

        let root = parse(r#"{"k\"ey":"v"}"#).unwrap();
        assert!(root.get(r#"k\"ey"#).is_some(), "keys are raw too");
        assert_eq!(root.dump(), r#"{"k\"ey":"v"}"#);
    }

    #[test]
    fn duplicate_keys_keep_first_value() {
        // C++ `_data.insert (std::pair (name, value))` keeps the existing value.
        let root = parse(r#"{"a":1,"b":2,"a":3}"#).unwrap();
        assert_eq!(root.get("a").and_then(JsonValue::as_number), Some(1.0));
        assert_eq!(root.dump(), "{\"a\":1.000000,\"b\":2.000000}");
    }

    #[test]
    fn numbers_and_literals() {
        let root = parse("[-1.5e2, +3, 0.1, 1., 1e400, true, false, null]").unwrap();
        assert_eq!(
            root.dump(),
            "[-150.000000,3.000000,0.100000,1.000000,inf,true,false,null]",
            "numbers dump as std::fixed precision 6"
        );
        let arr = root.as_array().unwrap();
        assert_eq!(arr[0].as_number(), Some(-150.0));
        assert_eq!(arr[0].jtype(), JType::Number);
        assert_eq!(arr[5].as_literal(), Some(LiteralValue::True));
        assert_eq!(arr[6].as_literal(), Some(LiteralValue::False));
        assert_eq!(arr[7].as_literal(), Some(LiteralValue::Null));
        assert_eq!(arr[7].jtype(), JType::Literal);

        // Literal `none` (the default constructor) dumps as "true".
        assert_eq!(JsonValue::Literal(LiteralValue::default()).dump(), "true");
        assert_eq!(LiteralValue::default(), LiteralValue::None);

        // No word boundary after a literal or number: "truex" stops after
        // "true", which leaves 'x' where ',' or ']' is expected.
        assert_eq!(
            parse("[truex]").unwrap_err().to_string(),
            "Error: missing ']' at position 5"
        );
    }

    #[test]
    fn types_and_accessors() {
        let root = parse(r#"{"o":{},"a":[1],"s":"x","n":2,"l":null}"#).unwrap();
        assert_eq!(root.jtype(), JType::Object);
        assert_eq!(root.get("o").unwrap().jtype(), JType::Object);
        assert_eq!(root.get("a").unwrap().jtype(), JType::Array);
        assert_eq!(root.get("s").unwrap().jtype(), JType::String);
        assert_eq!(root.get("n").unwrap().jtype(), JType::Number);
        assert_eq!(root.get("l").unwrap().jtype(), JType::Literal);
        assert!(root.get("missing").is_none());
        assert!(
            root.get("s").unwrap().get("x").is_none(),
            "get on a non-object"
        );
        assert!(root.as_array().is_none());
        assert!(root.as_str().is_none());
        assert!(root.as_number().is_none());
        assert_eq!(root.as_object().unwrap().len(), 5);
        assert_eq!(
            root.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["a", "l", "n", "o", "s"],
            "std::map key order"
        );

        assert_eq!(JType::Value as i32, 0);
        assert_eq!(JType::Object as i32, 1);
        assert_eq!(JType::Array as i32, 2);
        assert_eq!(JType::String as i32, 3);
        assert_eq!(JType::Number as i32, 4);
        assert_eq!(JType::Literal as i32, 5);

        assert_eq!(
            JsonValue::new_string("a\\\"b").dump(),
            "\"a\\\"b\"",
            "string constructor stores raw text"
        );

        let mut root = root;
        root.as_object_mut()
            .unwrap()
            .insert("z".to_string(), JsonValue::Number(1.0));
        assert_eq!(root.clone().into_object().unwrap().len(), 6);
        let mut arr = parse("[1]").unwrap();
        arr.as_array_mut()
            .unwrap()
            .push(JsonValue::Literal(LiteralValue::Null));
        assert_eq!(arr.dump(), "[1.000000,null]");
        assert_eq!(arr.into_array().unwrap().len(), 2);
    }

    #[test]
    fn per_type_parsers() {
        // string::parse does not skip whitespace; object/array restore the
        // cursor when they do not match.
        let mut pig = Pig::new("  \"x\"");
        assert!(parse_string(&mut pig).is_none());
        assert_eq!(pig.cursor(), 0);

        let mut pig = Pig::new("  5");
        assert!(parse_array(&mut pig).unwrap().is_none());
        assert_eq!(pig.cursor(), 0, "array::parse restores the checkpoint");
        assert!(parse_object(&mut pig).unwrap().is_none());
        assert_eq!(pig.cursor(), 0, "object::parse restores the checkpoint");

        let mut pig = Pig::new("  [ ] ");
        assert_eq!(
            parse_value(&mut pig).unwrap(),
            Some(JsonValue::Array(vec![]))
        );
        assert_eq!(pig.cursor(), 5);

        let mut pig = Pig::new("12.5x");
        assert_eq!(parse_number(&mut pig), Some(JsonValue::Number(12.5)));
        assert_eq!(pig.cursor(), 4);

        let mut pig = Pig::new("nul");
        assert!(parse_literal(&mut pig).is_none());
        assert!(parse_value(&mut pig).unwrap().is_none());
        assert_eq!(pig.cursor(), 0);

        // parse_pair: no name leaves everything untouched.
        let mut pig = Pig::new("  1");
        let mut name = "keep".to_string();
        let mut value = None;
        assert!(!parse_pair(&mut pig, &mut name, &mut value).unwrap());
        assert_eq!((name.as_str(), &value, pig.cursor()), ("keep", &None, 0));

        let mut pig = Pig::new("\"k\" : \"v\"");
        assert!(parse_pair(&mut pig, &mut name, &mut value).unwrap());
        assert_eq!(name, "k");
        assert_eq!(value, Some(JsonValue::new_string("v")));
    }

    #[test]
    fn unicode_whitespace_around_document() {
        // Pig::skipWS skips Unicode whitespace.
        let root = parse("\u{a0}\u{3000}{ \"a\" :\u{2003}[ ] }\u{2028}").unwrap();
        assert_eq!(root.dump(), "{\"a\":[]}");
    }

    ////////////////////////////////////////////////////////////////////////////
    // json_test.cpp + json_test.t: every test/json/*.json parses without an
    // "Error" in the json_test output.

    fn fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json")
    }

    fn fixture_files() -> Vec<std::path::PathBuf> {
        let mut files: Vec<_> = std::fs::read_dir(fixture_dir())
            .expect("fixture directory")
            .map(|entry| entry.expect("fixture entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        files
    }

    /// The stdout of the C++ `json_test` program for `args` (argv[1..]).
    fn json_test(args: &[&str]) -> String {
        let mut out = String::new();
        if args.is_empty() {
            out.push_str("\nUsage: json_test [-q] <file | JSON> ...\n");
            out.push('\n');
            out.push_str("      -q        quiet, no JSON dump\n");
            out.push_str("      <file>    file containing JSON\n");
            out.push_str("      <JSON>    JSON string, may need to be quoted\n");
            out.push('\n');
            return out;
        }

        let quiet = args.contains(&"-q");

        for arg in args {
            if *arg != "-q" {
                let path = std::path::Path::new(arg);
                let result = if path.exists() {
                    let contents = std::fs::read_to_string(path).expect("readable fixture");
                    parse(&contents)
                } else {
                    parse(arg)
                };

                match result {
                    Ok(root) => {
                        if !quiet {
                            out.push_str(&root.dump());
                            out.push('\n');
                        }
                    }
                    Err(e) => {
                        out.push_str(&e.to_string());
                        out.push('\n');
                    }
                }
            }
        }

        out
    }

    #[test]
    fn json_test_fixtures() {
        let files = fixture_files();
        assert_eq!(
            files.len(),
            20,
            "all 20 libshared test/json/*.json fixtures are present"
        );

        for file in &files {
            let path = file.to_str().unwrap();
            let out = json_test(&[path]);
            let name = file.file_name().unwrap().to_string_lossy();
            assert!(!out.contains("Error"), "TestJsonParsing {name}: {out}");
            assert!(
                out.starts_with('{') || out.starts_with('['),
                "TestJsonParsing {name} dumps: {out}"
            );
        }
    }

    #[test]
    fn json_test_program() {
        assert!(json_test(&[]).starts_with("\nUsage: json_test [-q] <file | JSON> ...\n"));
        assert_eq!(
            json_test(&["{\"a\":1}"]),
            "{\"a\":1.000000}\n",
            "JSON given on the command line"
        );
        assert_eq!(json_test(&["-q", "[1]"]), "", "-q suppresses the dump");
        assert_eq!(
            json_test(&["[1", "-q"]),
            "Error: missing ']' at position 2\n"
        );

        let e3 = fixture_dir().join("e3.json");
        assert_eq!(
            json_test(&[e3.to_str().unwrap()]),
            "{\"color\":\"red\",\"value\":\"#f00\"}\n"
        );
    }

    ////////////////////////////////////////////////////////////////////////////
    // json.t.cpp: SAX

    /// json.t.cpp `EventSink`: appends to `combined` with C++ stream
    /// formatting (`bool` as 1/0, `double` as `%g`).
    #[derive(Default)]
    struct CombinedSink {
        combined: String,
    }

    impl SaxSink for CombinedSink {
        fn event_doc_start(&mut self) {
            self.combined.push_str("<doc>");
        }
        fn event_doc_end(&mut self) {
            self.combined.push_str("</doc>");
        }
        fn event_object_start(&mut self) {
            self.combined.push_str("<object>");
        }
        fn event_object_end(&mut self, _: i32) {
            self.combined.push_str("</object>");
        }
        fn event_array_start(&mut self) {
            self.combined.push_str("<array>");
        }
        fn event_array_end(&mut self, _: i32) {
            self.combined.push_str("</array>");
        }
        fn event_name(&mut self, value: &str) {
            self.combined.push_str(&format!("<name>{value}</name>"));
        }
        fn event_value_null(&mut self) {
            self.combined.push_str("<null />");
        }
        fn event_value_bool(&mut self, value: bool) {
            self.combined
                .push_str(&format!("<bool>{}</bool>", i32::from(value)));
        }
        fn event_value_int(&mut self, value: i64) {
            self.combined.push_str(&format!("<int>{value}</int>"));
        }
        fn event_value_uint(&mut self, value: u64) {
            self.combined.push_str(&format!("<uint>{value}</uint>"));
        }
        fn event_value_double(&mut self, value: f64) {
            let text = crate::shared::format::stream_double(value);
            self.combined.push_str(&format!("<double>{text}</double>"));
        }
        fn event_value_string(&mut self, value: &str) {
            self.combined.push_str(&format!("<string>{value}</string>"));
        }
    }

    /// C++ `saxTest`: `t.ok (sax.parse (input, sink), "sax: '" + input +
    /// "' --> '" + expected + "'")`, or `t.pass (e)` on an exception. The
    /// C++ never compares `combined` with `expected`. Returns `combined`.
    fn sax_test(input: &str, expected: &str) -> String {
        let mut sink = CombinedSink::default();
        let mut sax = Sax::new();
        match sax.parse(input, &mut sink) {
            Ok(ok) => assert!(ok, "sax: '{input}' --> '{expected}'"),
            Err(e) => eprintln!("{e}"), // t.pass (e)
        }
        sink.combined
    }

    #[test]
    fn json_t_sax() {
        sax_test("{}", "<doc><object></object></doc>");
        sax_test(
            "{\"name\":\"value\"}",
            "<doc><object><name>name</name><string>value</string></object></doc>",
        );
        sax_test(
            "\n\t {\n\t \"name\"\n\t :\n\t \"value\"\n\t }\n\t ",
            "<doc><object><name>name</name><string>value</string></object></doc>",
        );
        sax_test(
            "[1,\"2\",true,null]",
            "<doc><array><int>1</int><string>2</string><bool>true</bool><null /></array></doc>",
        );
    }

    /// What the json.t.cpp `EventSink` actually accumulates for the SAX
    /// tests. The fourth differs from the C++ test's `expected` text,
    /// because `std::stringstream << bool` writes `1`, not `true`.
    #[test]
    fn json_t_sax_combined() {
        assert_eq!(sax_test("{}", ""), "<doc><object></object></doc>");
        assert_eq!(
            sax_test("{\"name\":\"value\"}", ""),
            "<doc><object><name>name</name><string>value</string></object></doc>"
        );
        assert_eq!(
            sax_test("\n\t {\n\t \"name\"\n\t :\n\t \"value\"\n\t }\n\t ", ""),
            "<doc><object><name>name</name><string>value</string></object></doc>"
        );
        assert_eq!(
            sax_test("[1,\"2\",true,null]", ""),
            "<doc><array><int>1</int><string>2</string><bool>1</bool><null /></array></doc>"
        );
    }

    ////////////////////////////////////////////////////////////////////////////
    // SAX: C++ behaviour beyond json.t.cpp

    #[derive(Debug, PartialEq)]
    enum Event {
        Name(String),
        Str(String),
        Int(i64),
        Uint(u64),
        Double(f64),
        Bool(bool),
        Null,
        ObjectEnd(i32),
        ArrayEnd(i32),
    }

    #[derive(Default)]
    struct EventLog {
        events: Vec<Event>,
    }

    impl SaxSink for EventLog {
        fn event_object_end(&mut self, count: i32) {
            self.events.push(Event::ObjectEnd(count));
        }
        fn event_array_end(&mut self, count: i32) {
            self.events.push(Event::ArrayEnd(count));
        }
        fn event_name(&mut self, value: &str) {
            self.events.push(Event::Name(value.to_string()));
        }
        fn event_value_null(&mut self) {
            self.events.push(Event::Null);
        }
        fn event_value_bool(&mut self, value: bool) {
            self.events.push(Event::Bool(value));
        }
        fn event_value_int(&mut self, value: i64) {
            self.events.push(Event::Int(value));
        }
        fn event_value_uint(&mut self, value: u64) {
            self.events.push(Event::Uint(value));
        }
        fn event_value_double(&mut self, value: f64) {
            self.events.push(Event::Double(value));
        }
        fn event_value_string(&mut self, value: &str) {
            self.events.push(Event::Str(value.to_string()));
        }
    }

    fn sax_events(input: &str) -> Vec<Event> {
        let mut log = EventLog::default();
        assert!(Sax::new().parse(input, &mut log).unwrap(), "{input}");
        log.events
    }

    #[test]
    fn sax_numbers() {
        assert_eq!(
            sax_events("[1.5,-2,1e3,0]"),
            [
                Event::Double(1.5),
                Event::Int(-2),
                Event::Double(1000.0),
                Event::Int(0),
                Event::ArrayEnd(4)
            ]
        );
        assert_eq!(
            sax_events("[9223372036854775807,-9223372036854775808]"),
            [
                Event::Int(i64::MAX),
                Event::Int(i64::MIN),
                Event::ArrayEnd(2)
            ]
        );

        // errno is never reset: after strtol overflows, strtoul's success is
        // ignored too, and every later number is a double.
        assert_eq!(
            sax_events("[7,18446744073709551615,7]"),
            [
                Event::Int(7),
                Event::Double(18446744073709551615.0),
                Event::Double(7.0),
                Event::ArrayEnd(3)
            ]
        );
        assert_eq!(
            sax_events("[1e999,7]"),
            [
                Event::Double(f64::INFINITY),
                Event::Double(7.0),
                Event::ArrayEnd(2)
            ],
            "strtod overflow sets ERANGE as well"
        );
        assert_eq!(
            sax_events("[1e-999,7]"),
            [Event::Double(0.0), Event::Double(7.0), Event::ArrayEnd(2)],
            "strtod underflow sets ERANGE as well"
        );
        // A new parser starts with a clear errno state.
        assert_eq!(sax_events("[7]"), [Event::Int(7), Event::ArrayEnd(1)]);

        // Whitespace before the fraction is taken into the number text, which
        // strtod then does not fully consume, so it is not a number (and the
        // cursor is restored to the '1').
        let mut log = EventLog::default();
        assert_eq!(
            Sax::new()
                .parse("[1 .5]", &mut log)
                .unwrap_err()
                .to_string(),
            "Error: Missing ']' at position 1"
        );
    }

    #[test]
    fn sax_strings() {
        assert_eq!(
            sax_events(r#"{"k\u0041":"a\/b\u20AC\v\q\'\"\\\b\f\n\r\t"}"#),
            [
                Event::Name("kA".to_string()),
                Event::Str("a/b\u{20ac}\u{b}q'\"\\\u{8}\u{c}\n\r\t".to_string()),
                Event::ObjectEnd(1),
            ]
        );
        // A \u escape needs four hex digits, else 'u' is an ordinary escape.
        assert_eq!(
            sax_events(r#"["\u12"]"#),
            [Event::Str("u12".to_string()), Event::ArrayEnd(1)]
        );
        assert_eq!(
            sax_events("[\"\u{e9}\"]"),
            [Event::Str("\u{e9}".to_string()), Event::ArrayEnd(1)]
        );
    }

    #[test]
    fn sax_structure() {
        assert_eq!(
            sax_events("{\"a\":[true,false,null,{}],\"b\":[]}"),
            [
                Event::Name("a".to_string()),
                Event::Bool(true),
                Event::Bool(false),
                Event::Null,
                Event::ObjectEnd(0),
                Event::ArrayEnd(4),
                Event::Name("b".to_string()),
                Event::ArrayEnd(0),
                Event::ObjectEnd(2),
            ]
        );
        // A trailing ',' is accepted: `isLiteral (',') && isPair/isValue`
        // consumes the ',' even when no pair/value follows.
        assert_eq!(sax_events("[1,]"), [Event::Int(1), Event::ArrayEnd(1)]);
        assert_eq!(
            sax_events("{\"a\":1,}"),
            [
                Event::Name("a".to_string()),
                Event::Int(1),
                Event::ObjectEnd(1)
            ]
        );

        // Duplicate names are all reported.
        assert_eq!(
            sax_events("{\"a\":1,\"a\":2}"),
            [
                Event::Name("a".to_string()),
                Event::Int(1),
                Event::Name("a".to_string()),
                Event::Int(2),
                Event::ObjectEnd(2)
            ]
        );
    }

    #[test]
    fn sax_errors() {
        let cases = [
            ("", "Error: Missing '{' or '[' at position 0"),
            ("  x", "Error: Missing '{' or '[' at position 2"),
            ("[1", "Error: Missing ']' at position 2"),
            ("{", "Error: Missing '}' at position 1"),
            ("{\"a\" 1}", "Error: Missing ':' at position 5"),
            ("{\"a\":}", "Error: Missing value at position 5"),
            ("[\"abc", "Error: Missing '\"' at position 5"),
            ("[\"abc\\", "Error: Missing '\"' at position 7"),
            ("[] x", "Error: extra characters found at position 3"),
            ("[truex]", "Error: Missing ']' at position 5"),
            ("[1,,2]", "Error: Missing ']' at position 3"),
        ];
        for (input, message) in cases {
            let mut log = EventLog::default();
            assert_eq!(
                Sax::new().parse(input, &mut log).unwrap_err().to_string(),
                message,
                "{input}"
            );
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    // sax_test.cpp: SAX-parse every fixture with the event-printing sink.

    #[derive(Default)]
    struct PrintSink {
        out: String,
    }

    impl SaxSink for PrintSink {
        fn event_object_start(&mut self) {
            self.out.push_str("# object start\n");
        }
        fn event_object_end(&mut self, count: i32) {
            self.out
                .push_str(&format!("# object end ({count} items)\n"));
        }
        fn event_array_start(&mut self) {
            self.out.push_str("# array start\n");
        }
        fn event_array_end(&mut self, count: i32) {
            self.out.push_str(&format!("# array end ({count} items)\n"));
        }
        fn event_name(&mut self, value: &str) {
            self.out.push_str(&format!("# name '{value}'\n"));
        }
        fn event_value_null(&mut self) {
            self.out.push_str("# value 'null'\n");
        }
        fn event_value_bool(&mut self, value: bool) {
            self.out.push_str(&format!(
                "# value '{}'\n",
                if value { "true" } else { "false" }
            ));
        }
        fn event_value_int(&mut self, value: i64) {
            self.out.push_str(&format!("# value '{value}'\n"));
        }
        fn event_value_uint(&mut self, value: u64) {
            self.out.push_str(&format!("# value '{value}'\n"));
        }
        fn event_value_double(&mut self, value: f64) {
            let text = crate::shared::format::stream_double(value);
            self.out.push_str(&format!("# value '{text}'\n"));
        }
        fn event_value_string(&mut self, value: &str) {
            self.out.push_str(&format!("# value '{value}'\n"));
        }
    }

    /// The C++ `sax_test` program for the files `args`: (stdout, status).
    fn sax_test_program(args: &[&std::path::Path]) -> (String, i32) {
        let mut status = 0;
        let mut out = String::new();
        if args.is_empty() {
            out.push_str(
                "\nUsage: json2_test <file> ...\n\n      <file>    file containing JSON\n\n",
            );
        } else {
            for arg in args {
                // std::ifstream of a missing file reads as empty.
                let input = std::fs::read_to_string(arg).unwrap_or_default();
                let mut sink = PrintSink::default();
                match Sax::new().parse(&input, &mut sink) {
                    Ok(ok) => {
                        if !ok {
                            status = 1;
                        }
                    }
                    Err(e) => {
                        sink.out.push_str(&e.to_string());
                        sink.out.push('\n');
                    }
                }
                out.push_str(&sink.out);
            }
        }
        (out, status)
    }

    #[test]
    fn sax_test_fixtures() {
        assert!(sax_test_program(&[])
            .0
            .starts_with("\nUsage: json2_test <file> ...\n"));

        for file in fixture_files() {
            let (out, status) = sax_test_program(&[&file]);
            let name = file.file_name().unwrap().to_string_lossy();
            assert_eq!(status, 0, "sax_test {name}");
            assert!(!out.contains("Error"), "sax_test {name}: {out}");
        }

        let (out, _) = sax_test_program(&[&fixture_dir().join("e3.json")]);
        assert_eq!(
            out,
            "# object start\n# name 'color'\n# value 'red'\n# name 'value'\n# value '#f00'\n# object end (2 items)\n"
        );
    }
}
