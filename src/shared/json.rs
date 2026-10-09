//! Rust translation of `libshared src/JSON.cpp`.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.
//!
//! # What is here now (milestone 1, foundation)
//!
//! Only the free functions `json::encode (const std::string&)` ([`encode`])
//! and `json::decode (const std::string&)` ([`decode`]), needed by
//! `Configuration::parse` and the `config` command.
//!
//! # What milestone 2 (Datetime/Duration/JSON/Lexer) adds
//!
//! The JSON parser and value types (`json::value`, `json::string`,
//! `json::number`, `json::literal`, `json::array`, `json::object`,
//! `json::parse`) and their `dump`s.

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
    if length >= 6 && ((input[0] == b'U' && input[1] == b'+') || (input[0] == b'\\' && input[1] == b'u')) {
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
#[cfg(test)]
mod tests {
    use super::*;

    // The encode/decode assertions of libshared `test/json.t.cpp`.
    #[test]
    fn json_t_encode_decode() {
        assert_eq!(encode("1\"2"), "1\\\"2", "json::encode <quote> -> <backslash><quote>");
        assert_eq!(decode("1\\\"2"), "1\"2", "json::decode <backslash><quote> -> <quote>");

        assert_eq!(encode("1/2"), "1\\/2", "json::encode <slash> -> <backslash><slash>");
        assert_eq!(decode("1\\/2"), "1/2", "json::decode <backslash><slash> -> <slash>");

        assert_eq!(encode("1\u{8}2"), "1\\b2", "json::encode <bell> -> <backslash><backslash><b>");
        assert_eq!(decode("1\\b2"), "1\u{8}2", "json::decode <backslash><backslash><b> -> <bell>");

        assert_eq!(encode("1\u{c}2"), "1\\f2", "json::encode <formfeed> -> <backslash><backslash><f>");
        assert_eq!(decode("1\\f2"), "1\u{c}2", "json::decode <backslash><backslash><f> -> <formfeed>");

        assert_eq!(encode("1\n2"), "1\\n2", "json::encode <newline> -> <backslash><newline>");
        assert_eq!(decode("1\\n2"), "1\n2", "json::decode <backslash><newline> -> <newline>");

        assert_eq!(encode("1\r2"), "1\\r2", "json::encode <cr> -> <backslash><r>");
        assert_eq!(decode("1\\r2"), "1\r2", "json::decode <backslash><r> -> <cr>");

        assert_eq!(encode("1\t2"), "1\\t2", "json::encode <tab> -> <backslash><t>");
        assert_eq!(decode("1\\t2"), "1\t2", "json::decode <backslash><t> -> <tab>");

        assert_eq!(encode("1\\2"), "1\\\\2", "json::encode <backslash> -> <backslash><backslash>");
        assert_eq!(decode("1\\\\2"), "1\\2", "json::decode <backslash><backslash> -> <backslash>");

        // C++ "1\x2" is '1' followed by the byte 0x02.
        assert_eq!(encode("1\x02"), "1\x02", "json::encode <backslash><x> -> <backslash><x>(NOP)");
        assert_eq!(decode("1\x02"), "1\x02", "json::decode <backslash><x> -> <backslash><x>(NOP)");

        assert_eq!(encode("1€2"), "1€2", "json::encode € -> €");
        assert_eq!(decode("1\\u20ac2"), "1€2", "json::decode <backslash>u20ac -> €");

        let encoded = encode("one\\");
        assert_eq!(encoded, "one\\\\", "json::encode one<backslash> -> one<backslash><backslash>");
        assert_eq!(encoded.len(), 5, "json::encode one<backslash> -> length 5");
        assert_eq!(encoded.as_bytes()[0], b'o', "json::encode one<backslash>[0] -> o");
        assert_eq!(encoded.as_bytes()[1], b'n', "json::encode one<backslash>[1] -> n");
        assert_eq!(encoded.as_bytes()[2], b'e', "json::encode one<backslash>[2] -> e");
        assert_eq!(encoded.as_bytes()[3], b'\\', "json::encode one<backslash>[3] -> <backslash>");
        assert_eq!(encoded.as_bytes()[4], b'\\', "json::encode one<backslash>[4] -> <backslash>");

        assert_eq!(decode(&encoded), "one\\", "json::decode one<backslash><backslash> -> one<backslash>");

        // Invalid input, trailing `\`.
        assert_eq!(decode("1\\"), "1\\", "json::decode <backslash> -> <backslash>");

        // Embedded NUL. C++ "\0012" is the byte 0x01 followed by '2'.
        assert_eq!(decode("\x012"), "\x012", "json::decode <nul>12 -> <nul>12");

        // Escaped embedded NUL.
        assert_eq!(decode("\\\x012"), "\\\x012", "json::decode <backslash><nul>12 -> <backslash><nul>12");
    }

    #[test]
    fn encode_only_touches_the_table_entries() {
        assert_eq!(encode(""), "");
        assert_eq!(encode("\x00\x01\x0b\x7f"), "\x00\x01\x0b\x7f");
        assert_eq!(encode("\"\\/\u{8}\u{c}\n\r\t"), "\\\"\\\\\\/\\b\\f\\n\\r\\t");
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
        assert_eq!(decode("\\u41"), "", "short \\u escape is code point 0, which is empty");
        assert_eq!(decode("\\uU+0041"), "A41", "the U+ form of utf8_codepoint, but only 4 bytes are skipped");
        assert_eq!(decode("a\\\0b"), "a\\b", "embedded NUL after a backslash is dropped");
    }
}
