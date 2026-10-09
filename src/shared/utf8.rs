//! Rust translation of `libshared src/utf8.cpp`.
//!
//! Partially ported: only `utf8_sequence`, `utf8_next_char` and `mk_wcwidth`
//! are translated so far (they are needed by `shared.rs`). The remaining
//! functions (`utf8_codepoint`, `utf8_character`, `utf8_length`,
//! `utf8_text_length`, `utf8_width`, `utf8_text_width`, `utf8_substr`,
//! `utf8_truncate_to_width`) are filled in by the milestone 1 utf8 port.
//!
//! The C++ functions operate on `std::string` byte strings and byte offsets;
//! these translations do the same over the UTF-8 bytes of a `&str`.
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
    use super::*;

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
