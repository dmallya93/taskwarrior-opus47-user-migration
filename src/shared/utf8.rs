//! Rust translation of `libshared src/utf8.cpp` (everything declared in
//! `utf8.h`).
//!
//! The C++ functions operate on `std::string` byte strings and byte offsets;
//! these translations do the same over the UTF-8 bytes of a `&str`. The C++
//! `unsigned int` results stay `u32`, and `wchar_t` stays `i32`.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

use crate::shared::wcwidth::{
    widechar_wcwidth, WIDECHAR_AMBIGUOUS, WIDECHAR_PRIVATE_USE, WIDECHAR_WIDENED_IN_9,
};

/// C++ `std::string::operator[]` on a byte string: index `len` yields the
/// terminating NUL. Indices past the end (undefined behaviour in C++) also
/// yield NUL here.
fn byte_at(input: &[u8], i: usize) -> u8 {
    input.get(i).copied().unwrap_or(0)
}

/// C++ `char` (signed on the reference platforms) promoted to `unsigned int`.
fn char_as_unsigned(byte: u8) -> u32 {
    byte as i8 as i32 as u32
}

/// C++ `XDIGIT` macro: converts a hex digit byte to its value
/// (`'0'` -> 0, `'9'` -> 9, `'a'`/`'A'` -> 10, `'f'`/`'F'` -> 15). Any other
/// byte converts to 0.
fn xdigit(x: u8) -> u32 {
    match x {
        b'0'..=b'9' => (x - b'0') as u32,
        b'a'..=b'f' => (x + 10 - b'a') as u32,
        b'A'..=b'F' => (x + 10 - b'A') as u32,
        _ => 0,
    }
}

/// C++ `utf8_codepoint`: parses a 4-digit hex code point in one of the forms
/// `xxxx`, `\uxxxx` or `U+xxxx`. Inputs shorter than 4 bytes yield 0;
/// non-hex digits count as 0.
pub fn utf8_codepoint(input: &str) -> u32 {
    let input = input.as_bytes();
    let length = input.len();

    // U+xxxx, \uxxxx
    if length >= 6 && ((input[0] == b'U' && input[1] == b'+') || (input[0] == b'\\' && input[1] == b'u')) {
        xdigit(input[2]) << 12 | xdigit(input[3]) << 8 | xdigit(input[4]) << 4 | xdigit(input[5])
    } else if length >= 4 {
        xdigit(input[0]) << 12 | xdigit(input[1]) << 8 | xdigit(input[2]) << 4 | xdigit(input[3])
    } else {
        0
    }
}

/// C++ `utf8_next_char`: iterates along a UTF8 string.
///   - argument `i` counts bytes advanced through the string
///   - returns the next character
///
/// Returns 0 at the end of the string. An embedded NUL also returns 0, but is
/// advanced over.
pub fn utf8_next_char(input: &str, i: &mut usize) -> u32 {
    let input = input.as_bytes();

    // Try to recognize end of string without checking length on every call.
    if byte_at(input, *i) == 0 {
        // If this is not the end of the string, then this is a NUL character
        // embedded in the string, so advance over it.
        if *i != input.len() {
            *i += 1;
        }
        return 0;
    }

    // How many bytes in the sequence?
    let length = utf8_sequence(char_as_unsigned(byte_at(input, *i)));
    *i += length as usize;
    let at = |back: usize| char_as_unsigned(byte_at(input, *i - back));

    match length {
        // 0xxxxxxx -> 0xxxxxxx
        1 => at(1),

        // 110yyyyy 10xxxxxx -> 00000yyy yyxxxxxx
        2 => ((at(2) & 0x1F) << 6) + (at(1) & 0x3F),

        // 1110zzzz 10yyyyyy 10xxxxxx -> zzzzyyyy yyxxxxxx
        3 => ((at(3) & 0xF) << 12) + ((at(2) & 0x3F) << 6) + (at(1) & 0x3F),

        // 11110www 10zzzzzz 10yyyyyy 10xxxxxx -> 000wwwzz zzzzyyyy yyxxxxxx
        4 => {
            ((at(4) & 0x7) << 18) + ((at(3) & 0x3F) << 12) + ((at(2) & 0x3F) << 6) + (at(1) & 0x3F)
        }

        // Default: pretend as though it's a single character.
        _ => at(1),
    }
}

/// C++ `utf8_character`: the UTF-8 encoding of `codepoint`
/// (<http://en.wikipedia.org/wiki/UTF-8>).
///
/// As in C++, code point 0 and code points at or above U+110000 yield an empty
/// string (the C++ result is built from a NUL-terminated buffer). The C++
/// encoder also emits the 3-byte pattern for UTF-16 surrogates
/// (U+D800..U+DFFF), which is not valid UTF-8 and cannot be held in a Rust
/// `String`; those yield U+FFFD REPLACEMENT CHARACTER instead. For every other
/// code point the bytes are identical to the C++ bit-twiddling encoder.
pub fn utf8_character(codepoint: u32) -> String {
    if codepoint == 0 || codepoint >= 0x110000 {
        return String::new();
    }

    char::from_u32(codepoint).unwrap_or(char::REPLACEMENT_CHARACTER).to_string()
}

/// C++ `utf8_sequence`: the number of bytes in the UTF-8 sequence introduced
/// by lead byte `character`.
pub fn utf8_sequence(character: u32) -> i32 {
    if (character & 0xE0) == 0xC0 {
        return 2;
    }

    if (character & 0xF0) == 0xE0 {
        return 3;
    }

    if (character & 0xF8) == 0xF0 {
        return 4;
    }

    1
}

/// C++ `utf8_length`: length of a string in characters.
///
/// Counts every byte except UTF-8 continuation bytes (`0b10xxxxxx`), so only
/// the first byte of any sequence is counted.
pub fn utf8_length(input: &str) -> u32 {
    input.bytes().filter(|&byte| (byte & 0xC0) != 0x80).count() as u32
}

/// C++ `utf8_width`: width of a string in character cells.
///
/// Stops at an embedded NUL, as the C++ `utf8_next_char` loop does.
pub fn utf8_width(input: &str) -> u32 {
    let mut length: u32 = 0;
    let mut i = 0;
    loop {
        let c = utf8_next_char(input, &mut i);
        if c == 0 {
            break;
        }

        // Control characters, and more especially newline characters, make
        // mk_wcwidth() return -1.  Ignore that, thereby "adding zero" to length.
        // Since control characters are not displayed in reports, this is a valid
        // choice.
        let l = mk_wcwidth(c as i32);
        if l != -1 {
            length = length.wrapping_add(l as u32);
        }
    }

    length
}

/// C++ `utf8_text_length`: like [`utf8_length`], but every byte of an escape
/// sequence (from ESC up to and including the next `m`) is not counted, so
/// color codes do not contribute.
pub fn utf8_text_length(input: &str) -> u32 {
    let mut char_length = input.len() as u32;
    let mut in_color = false;

    // Decrement the number of bytes for each byte that matches 0b10??????
    // this way only the first byte of any utf8 sequence is counted.
    for byte in input.bytes() {
        if in_color {
            if byte == b'm' {
                in_color = false;
            }

            char_length = char_length.wrapping_sub(1);
        } else if byte == 0o33 {
            in_color = true;
            char_length = char_length.wrapping_sub(1);
        } else if (byte & 0xC0) == 0x80 {
            // Extract the first two bits and check whether they are 10
            char_length = char_length.wrapping_sub(1);
        }
    }

    char_length
}

/// C++ `utf8_text_width`: like [`utf8_width`], but escape sequences (from ESC
/// up to and including the next `m`) have no width.
pub fn utf8_text_width(input: &str) -> u32 {
    let mut in_color = false;

    let mut length: u32 = 0;
    let mut i = 0;
    loop {
        let c = utf8_next_char(input, &mut i);
        if c == 0 {
            break;
        }

        if in_color {
            if c == 'm' as u32 {
                in_color = false;
            }
        } else if c == 0o33 {
            in_color = true;
        } else {
            length = length.wrapping_add(mk_wcwidth(c as i32) as u32);
        }
    }

    length
}

/// C++ `utf8_substr (input, start, length = 0)`: the substring starting at
/// character `start` and spanning `length` characters. The C++ default
/// argument is explicit here: `length == 0` means "to the end of the string".
/// Character counts beyond the end of the string are clamped to it.
pub fn utf8_substr(input: &str, start: u32, length: u32) -> String {
    // Find the starting index.
    let mut index_start = 0;
    for _ in 0..start {
        utf8_next_char(input, &mut index_start);
    }

    let index_end = if length != 0 {
        let mut index_end = index_start;
        for _ in 0..length {
            utf8_next_char(input, &mut index_end);
        }
        index_end
    } else {
        input.len()
    };

    byte_range(input, index_start, index_end)
}

/// C++ `utf8_truncate_to_width`: truncates a UTF-8 string to fit within a
/// target display width. Unlike [`utf8_substr`], which counts characters,
/// this counts display columns.
pub fn utf8_truncate_to_width(input: &str, target_width: u32) -> String {
    let mut current_width: u32 = 0;
    let mut i = 0;
    let mut last_safe = 0;

    loop {
        let c = utf8_next_char(input, &mut i);
        if c == 0 {
            break;
        }

        let w = mk_wcwidth(c as i32).max(0) as u32;
        if current_width.wrapping_add(w) > target_width {
            break;
        }
        current_width = current_width.wrapping_add(w);
        last_safe = i;
    }

    byte_range(input, 0, last_safe)
}

/// C++ `input.substr (start, end - start)` with both offsets clamped to the
/// string. The offsets come from `utf8_next_char`, so they are on character
/// boundaries of a valid `&str`.
fn byte_range(input: &str, start: usize, end: usize) -> String {
    let end = end.min(input.len());
    let start = start.min(end);
    String::from_utf8_lossy(&input.as_bytes()[start..end]).into_owned()
}

/// C++ `mk_wcwidth(wchar_t)`: display width of a code point in character
/// cells. `wchar_t` is a signed 32-bit integer on the reference platforms.
pub fn mk_wcwidth(ucs: i32) -> i32 {
    let width = widechar_wcwidth(ucs as u32);

    if width >= 0 {
        return width;
    }

    // Interpret widened characters as 2 chars
    if width == WIDECHAR_WIDENED_IN_9 {
        return 2;
    }

    // Interpret ambiguous east-asian characters as 1 char
    // This includes accented characters like á or é
    if width == WIDECHAR_AMBIGUOUS {
        return 1;
    }

    // Interpret Private Use Area (PUA) characters as 1 char for Nerd Fonts
    if width == WIDECHAR_PRIVATE_USE {
        return 1;
    }

    // Emoji pictographs (U+1F300+) — width 2 in modern terminals
    if (0x1F300..=0x1F9FF).contains(&ucs)   // Misc Symbols, Pictographs, Emoticons, Supplemental
        || (0x1FA00..=0x1FAFF).contains(&ucs)
    // Symbols and Pictographs Extended-A
    {
        return 2;
    }

    // Common symbols — width 1 in modern terminals
    if (0x2190..=0x21FF).contains(&ucs)     // Arrows
        || (0x2300..=0x23FF).contains(&ucs) // Misc Technical
        || (0x25A0..=0x25FF).contains(&ucs) // Geometric Shapes
        || (0x2600..=0x26FF).contains(&ucs) // Misc Symbols
        || (0x2700..=0x27BF).contains(&ucs)
    // Dingbats
    {
        return 1;
    }

    // Variation selectors — zero width
    if (0xFE00..=0xFE0F).contains(&ucs) {
        return 0;
    }

    // All other negative values
    0
}

#[cfg(test)]
mod tests {
    //! Port of libshared `test/utf8.t.cpp`. Each C++ `t.is` is one assertion
    //! here, carrying the C++ test description as its message. The C++ file
    //! leaves `utf8_next_char`, `utf8_character` and `utf8_sequence` as TODO;
    //! they (and `utf8_truncate_to_width`) get extra Rust-only tests below.

    use super::*;

    const ASCII_TEXT: &str = "This is a test";
    const UTF8_TEXT: &str = "m\u{e1}s s\u{e1}bado mi\u{e9}rcoles";
    const UTF8_WIDE_TEXT: &str = "\u{6539}\u{53d8}\u{5404}\u{79cd}\u{989c}\u{8272}";

    const ASCII_TEXT_COLOR: &str = "This \x1b[1mis\x1b[0m a test";
    const UTF8_TEXT_COLOR: &str = "m\u{e1}s \x1b[1ms\u{e1}bado\x1b[0m mi\u{e9}rcoles";
    const UTF8_WIDE_TEXT_COLOR: &str = "\u{6539}\x1b[1m\u{53d8}\u{5404}\u{79cd}\x1b[0m\u{989c}\u{8272}";

    #[test]
    fn test_strings_match_cpp() {
        assert_eq!(UTF8_TEXT, "más sábado miércoles");
        assert_eq!(UTF8_WIDE_TEXT, "改变各种颜色");
        assert_eq!(UTF8_WIDE_TEXT_COLOR, "改\x1b[1m变各种\x1b[0m颜色");
    }

    #[test]
    fn codepoint() {
        // unsigned int utf8_codepoint (const std::string&);
        assert_eq!(utf8_codepoint("\\u0020") as i32, 32, "\\u0020 --> ' '");
        assert_eq!(utf8_codepoint("U+0020") as i32, 32, "U+0020 --> ' '");
    }

    #[test]
    fn codepoint_forms() {
        assert_eq!(utf8_codepoint("263a"), 0x263A);
        assert_eq!(utf8_codepoint("263A"), 0x263A);
        assert_eq!(utf8_codepoint("\\u263A"), 0x263A);
        assert_eq!(utf8_codepoint("U+00e9"), 0xE9);
        // Too short for any form.
        assert_eq!(utf8_codepoint("26a"), 0);
        // Five bytes: not long enough for U+xxxx, so parsed as bare xxxx.
        assert_eq!(utf8_codepoint("U+002"), 0x0000);
        // Non-hex digits count as zero.
        assert_eq!(utf8_codepoint("zz1z"), 0x0010);
    }

    #[test]
    fn length() {
        // unsigned int utf8_length (const std::string&);
        assert_eq!(utf8_length(ASCII_TEXT) as i32, 14, "ASCII utf8_length");
        assert_eq!(utf8_length(UTF8_TEXT) as i32, 20, "UTF8 utf8_length");
        assert_eq!(utf8_length(UTF8_WIDE_TEXT) as i32, 6, "UTF8 wide utf8_length");
    }

    #[test]
    fn width() {
        // unsigned int utf8_width (const std::string&);
        assert_eq!(utf8_width(ASCII_TEXT) as i32, 14, "ASCII utf8_width");
        assert_eq!(utf8_width(UTF8_TEXT) as i32, 20, "UTF8 utf8_width");
        assert_eq!(utf8_width(UTF8_WIDE_TEXT) as i32, 12, "UTF8 wide utf8_width");
    }

    #[test]
    fn text_length() {
        // unsigned int utf8_text_length (const std::string&);
        assert_eq!(utf8_text_length(ASCII_TEXT_COLOR) as i32, 14, "ASCII utf8_text_length");
        assert_eq!(utf8_text_length(UTF8_TEXT_COLOR) as i32, 20, "UTF8 utf8_text_length");
        assert_eq!(utf8_text_length(UTF8_WIDE_TEXT_COLOR) as i32, 6, "UTF8 wide utf8_text_length");
    }

    #[test]
    fn text_width() {
        // unsigned int utf8_text_width (const std::string&);
        assert_eq!(utf8_text_width(ASCII_TEXT_COLOR) as i32, 14, "ASCII utf8_text_width");
        assert_eq!(utf8_text_width(UTF8_TEXT_COLOR) as i32, 20, "UTF8 utf8_text_width");
        assert_eq!(utf8_text_width(UTF8_WIDE_TEXT_COLOR) as i32, 12, "UTF8 wide utf8_text_width");
    }

    #[test]
    fn substr() {
        // const std::string utf8_substr (const std::string&, unsigned int, unsigned int length = 0);
        assert_eq!(utf8_substr(ASCII_TEXT, 0, 2), "Th", "ASCII utf8_substr");
        assert_eq!(utf8_substr(UTF8_TEXT, 0, 2), "má", "UTF8 utf8_substr");
        assert_eq!(utf8_substr(UTF8_WIDE_TEXT, 0, 2), "改变", "UTF8 wide utf8_substr");
    }

    #[test]
    fn substr_default_length_and_clamping() {
        assert_eq!(utf8_substr(UTF8_TEXT, 4, 0), "sábado miércoles");
        assert_eq!(utf8_substr(UTF8_WIDE_TEXT, 4, 0), "颜色");
        assert_eq!(utf8_substr(UTF8_WIDE_TEXT, 4, 10), "颜色");
        assert_eq!(utf8_substr(UTF8_WIDE_TEXT, 10, 0), "");
        assert_eq!(utf8_substr("", 0, 3), "");
    }

    #[test]
    fn mk_wcwidth_cases() {
        // int mk_wcwidth (wchar_t);
        let cases: &[(i32, i32, &str)] = &[
            ('\0' as i32, 0, "mk_wcwidth U+0000 --> 0"),
            ('\r' as i32, 0, "mk_wcwidth U+000D --> 0"),
            ('!' as i32, 1, "mk_wcwidth U+0021 '!' --> 1"),
            ('$' as i32, 1, "mk_wcwidth U+0024 '$' --> 1"),
            ('%' as i32, 1, "mk_wcwidth U+0025 '%' --> 1"),
            ('+' as i32, 1, "mk_wcwidth U+002B '+' --> 1"),
            ('0' as i32, 1, "mk_wcwidth U+0030 '0' --> 1"),
            ('A' as i32, 1, "mk_wcwidth U+0041 'A' --> 1"),
            ('Z' as i32, 1, "mk_wcwidth U+005A 'Z' --> 1"),
            ('a' as i32, 1, "mk_wcwidth U+0061 'a' --> 1"),
            ('z' as i32, 1, "mk_wcwidth U+007A 'z' --> 1"),
            ('~' as i32, 1, "mk_wcwidth U+007E '~' --> 1"),
            (0x00A3, 1, "mk_wcwidth U+00A3 '£' --> 1"),
            (0x00AE, 1, "mk_wcwidth U+00AE '®' --> 1"),
            (0x00B5, 1, "mk_wcwidth U+00B5 'µ' --> 1"),
            (0x00C0, 1, "mk_wcwidth U+00C0 'À' --> 1"),
            (0x00C1, 1, "mk_wcwidth U+00C1 'Á' --> 1"),
            (0x00C4, 1, "mk_wcwidth U+00C4 'Ä' --> 1"),
            (0x00C7, 1, "mk_wcwidth U+00C7 'Ç' --> 1"),
            (0x00D6, 1, "mk_wcwidth U+00D6 'Ö' --> 1"),
            (0x00D8, 1, "mk_wcwidth U+00D8 'Ø' --> 1"),
            (0x00DD, 1, "mk_wcwidth U+00DD 'Ý' --> 1"),
            (0x00DF, 1, "mk_wcwidth U+00DF 'ß' --> 1"),
            (0x00E0, 1, "mk_wcwidth U+00E0 'à' --> 1"),
            (0x00E1, 1, "mk_wcwidth U+00E1 'á' --> 1"),
            (0x00E4, 1, "mk_wcwidth U+00E4 'ä' --> 1"),
            (0x00F4, 1, "mk_wcwidth U+00F4 'ô' --> 1"),
            (0x00FC, 1, "mk_wcwidth U+00FC 'ü' --> 1"),
            (0x010C, 1, "mk_wcwidth U+010C 'Č' --> 1"),
            (0x010D, 1, "mk_wcwidth U+010D 'č' --> 1"),
            (0x0110, 1, "mk_wcwidth U+0110 'Đ' --> 1"),
            (0x0118, 1, "mk_wcwidth U+0118 'Ę' --> 1"),
            (0x0122, 1, "mk_wcwidth U+0122 'Ģ' --> 1"),
            (0x0139, 1, "mk_wcwidth U+0139 'Ĺ' --> 1"),
            (0x013A, 1, "mk_wcwidth U+013A 'ĺ' --> 1"),
            (0x013D, 1, "mk_wcwidth U+013D 'Ľ' --> 1"),
            (0x0151, 1, "mk_wcwidth U+0151 'ő' --> 1"),
            (0x0161, 1, "mk_wcwidth U+0161 'š' --> 1"),
            (0x0164, 1, "mk_wcwidth U+0164 'Ť' --> 1"),
            (0x017E, 1, "mk_wcwidth U+017E 'ž' --> 1"),
            (0x018C, 1, "mk_wcwidth U+018C 'ƌ' --> 1"),
            (0x0190, 1, "mk_wcwidth U+0190 'Ɛ' --> 1"),
            (0x019E, 1, "mk_wcwidth U+019E 'ƞ' --> 1"),
            (0x01FA, 1, "mk_wcwidth U+01FA 'Ǻ' --> 1"),
            (0x020F, 1, "mk_wcwidth U+020F 'ȏ' --> 1"),
            (0x0213, 1, "mk_wcwidth U+0213 'ȓ' --> 1"),
            (0x0219, 1, "mk_wcwidth U+0219 'ș' --> 1"),
            (0x022F, 1, "mk_wcwidth U+022F 'ȯ' --> 1"),
            (0x0247, 1, "mk_wcwidth U+0247 'ɇ' --> 1"),
            (0x26C4, 2, "mk_wcwidth U+26C4 '⛄' --> 2"),
            (0x2744, 1, "mk_wcwidth U+2744 '❄' --> 1"),
            (0x1F381, 2, "mk_wcwidth U+1F381 '🎁' --> 2"),
            (0x1F384, 2, "mk_wcwidth U+1F384 '🎄' --> 2"),
            (0x1F4BB, 2, "mk_wcwidth U+1F4BB '💻' --> 2"),
            (0x1F44D, 2, "mk_wcwidth U+1F44D '👍' --> 2"),
            (0x1F602, 2, "mk_wcwidth U+1F602 '😂' --> 2"),
            (0x1F64F, 2, "mk_wcwidth U+1F64F '🙏' --> 2"),
            (0xF023, 1, "mk_wcwidth U+F023'' --> 1"),
            (0xF023, 1, "mk_wcwidth U+F023'' --> 1"),
            (0xF023, 1, "mk_wcwidth U+F023'' --> 1"),
            (0x5149, 2, "mk_wcwidth U+5149 --> 2"),
            (0x9a8c, 2, "mk_wcwidth U+9a8c --> 2"),
            (0x4e70, 2, "mk_wcwidth U+4e70 --> 2"),
            (0x94b1, 2, "mk_wcwidth U+94b1 --> 2"),
            (0x5305, 2, "mk_wcwidth U+5305 --> 2"),
            (0x91cd, 2, "mk_wcwidth U+91cd --> 2"),
            (0x65b0, 2, "mk_wcwidth U+65b0 --> 2"),
            (0x8bbe, 2, "mk_wcwidth U+8bbe --> 2"),
            (0x8ba1, 2, "mk_wcwidth U+8ba1 --> 2"),
            (0x5411, 2, "mk_wcwidth U+5411 --> 2"),
            (0x4e0a, 2, "mk_wcwidth U+4e0a --> 2"),
            (0x4e0b, 2, "mk_wcwidth U+4e0b --> 2"),
            (0x7bad, 2, "mk_wcwidth U+7bad --> 2"),
            (0x5934, 2, "mk_wcwidth U+5934 --> 2"),
            (0xff0c, 2, "mk_wcwidth U+ff0c --> 2"),
        ];
        for &(ucs, expected, description) in cases {
            assert_eq!(mk_wcwidth(ucs), expected, "{description}");
        }
    }

    #[test]
    fn next_char_walks_utf8() {
        let text = "a\u{c7}\u{263a}\u{1f600}";
        let mut i = 0;
        assert_eq!(utf8_next_char(text, &mut i), 'a' as u32);
        assert_eq!(i, 1);
        assert_eq!(utf8_next_char(text, &mut i), 0xC7);
        assert_eq!(i, 3);
        assert_eq!(utf8_next_char(text, &mut i), 0x263A);
        assert_eq!(i, 6);
        assert_eq!(utf8_next_char(text, &mut i), 0x1F600);
        assert_eq!(i, 10);
        assert_eq!(utf8_next_char(text, &mut i), 0);
        assert_eq!(i, 10);
    }

    #[test]
    fn next_char_embedded_nul_advances() {
        let mut i = 1;
        assert_eq!(utf8_next_char("a\0b", &mut i), 0);
        assert_eq!(i, 2);
    }

    #[test]
    fn character_encodes() {
        assert_eq!(utf8_character(0x41), "A");
        assert_eq!(utf8_character(0xE9), "\u{e9}");
        assert_eq!(utf8_character(0x263A), "\u{263a}");
        assert_eq!(utf8_character(0x1F600), "\u{1f600}");
        assert_eq!(utf8_character(0x10FFFF).as_bytes(), [0xF4, 0x8F, 0xBF, 0xBF]);
        // NUL and out-of-range code points give an empty string, as in C++.
        assert_eq!(utf8_character(0), "");
        assert_eq!(utf8_character(0x110000), "");
        // Surrogates cannot be represented (documented deviation).
        assert_eq!(utf8_character(0xD800), "\u{fffd}");
        // Round trip through utf8_next_char.
        for cp in [0x24, 0xA2, 0x20AC, 0x10348] {
            let mut i = 0;
            assert_eq!(utf8_next_char(&utf8_character(cp), &mut i), cp);
        }
    }

    #[test]
    fn sequence_lengths() {
        assert_eq!(utf8_sequence(0x41), 1);
        assert_eq!(utf8_sequence(0xC3), 2);
        assert_eq!(utf8_sequence(0xE2), 3);
        assert_eq!(utf8_sequence(0xF0), 4);
        assert_eq!(utf8_sequence(0x80), 1);
        assert_eq!(utf8_sequence(0xF8), 1);
    }

    #[test]
    fn truncate_to_width() {
        assert_eq!(utf8_truncate_to_width(ASCII_TEXT, 4), "This");
        assert_eq!(utf8_truncate_to_width(ASCII_TEXT, 100), ASCII_TEXT);
        assert_eq!(utf8_truncate_to_width(UTF8_WIDE_TEXT, 5), "改变");
        assert_eq!(utf8_truncate_to_width(UTF8_WIDE_TEXT, 6), "改变各");
        assert_eq!(utf8_truncate_to_width(UTF8_WIDE_TEXT, 0), "");
        // Zero-width characters after the cut-off still fit.
        assert_eq!(utf8_truncate_to_width("ab\u{301}c", 2), "ab\u{301}");
    }

    #[test]
    fn width_stops_at_embedded_nul() {
        assert_eq!(utf8_width("ab\0cd"), 2);
        assert_eq!(utf8_text_width("ab\0cd"), 2);
        assert_eq!(utf8_length("ab\0cd"), 5);
    }

    #[test]
    fn wcwidth_values() {
        assert_eq!(mk_wcwidth('a' as i32), 1);
        assert_eq!(mk_wcwidth(0x4E00), 2);
        assert_eq!(mk_wcwidth(0x263A), 1);
        assert_eq!(mk_wcwidth(0x1F600), 2);
        assert_eq!(mk_wcwidth(0x0301), 0);
        assert_eq!(mk_wcwidth('\n' as i32), 0);
        assert_eq!(mk_wcwidth(0xE000), 1);
    }
}
