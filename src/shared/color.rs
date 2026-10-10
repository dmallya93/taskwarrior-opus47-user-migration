//! Rust translation of `libshared src/Color.cpp` / `Color.h`.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.
//!
//! Ported in full ahead of milestone 3 (Color/Table/Variant/Eval), because
//! `version` (milestone 1) needs `Color ("bold").colorize ()` and libshared
//! `Table` byte-exact; audited line by line against Color.cpp in milestone 3.
//! Every member of the C++ class is here, including the quirks (e.g.
//! `operator==` compares the low 32 bits only, `upgrade24b` clears the
//! foreground index rather than the background one when converting a
//! background, `code ()` emits no bold in 256/24-bit mode, and `index2truecolor`
//! maps 8, i.e. bright black, to `0x000000`). The complete `libshared
//! test/color.t.cpp` is ported, assertion for assertion and in order, in the
//! tests below, with an assertion-count check (1046).
//!
//! C++ API to Rust:
//! - `Color ()` -> [`Color::new`] / `Color::default ()`
//! - `Color (const std::string&)` -> [`Color::parse`] (also `str::parse`,
//!   `TryFrom<&str>`, `TryFrom<&String>`)
//! - `Color (unsigned long)` -> [`Color::from_value`] / `From<u64>`
//! - `Color (color_id)` -> [`Color::from_id`] / `From<ColorId>`
//! - `Color (fg, bg, underline, bold, bright)` -> [`Color::from_ids`]
//! - `Color::nocolor`, `Color::red`, ... -> [`ColorId::Nocolor`], [`ColorId::Red`], ...
//! - `operator std::string ()` -> `Display` (`to_string ()`), `From<Color> for String`
//! - `operator int ()` -> [`Color::to_int`], `From<Color> for i32`
//! - `colorize (input)` -> [`Color::colorize`]; `_colorize (result, input)` ->
//!   [`Color::colorize_into`]
//! - static `Color::colorize (input, spec)` -> [`Color::colorize_with`]
//!   (Rust has no overloading)
//! - `strip`, `blend`, `upgrade`, `upgrade24b`, `index2truecolor`, `code`,
//!   `end`, `nontrivial`, `operator==` -> same names / `PartialEq`
//!
//! Deviation: C++ `throw format ("The color '{1}' is not recognized.", word)`
//! becomes `Err (`[`TwError::Message`]`)` with the identical text.

use std::fmt;

use crate::error::{Result, TwError};
use crate::shared::format::format;
use crate::shared::shared::{lower_case, split, trim};

/// `_COLOR_24BIT`: 24-bit mode.
pub const COLOR_24BIT: u64 = 0x0000_0000_8000_0000;
/// `_COLOR_INVERSE`: inverse attribute.
pub const COLOR_INVERSE: u64 = 0x0000_0000_4000_0000;
/// `_COLOR_256`: 256-color mode.
pub const COLOR_256: u64 = 0x0000_0000_2000_0000;
/// `_COLOR_HASBG`: has background color (all values taken).
pub const COLOR_HASBG: u64 = 0x0000_0000_1000_0000;
/// `_COLOR_HASFG`: has foreground color (all values taken).
pub const COLOR_HASFG: u64 = 0x0000_0000_0800_0000;
/// `_COLOR_UNDERLINE`: general underline attribute.
pub const COLOR_UNDERLINE: u64 = 0x0000_0000_0400_0000;
/// `_COLOR_BOLD`: 16-color bold attribute.
pub const COLOR_BOLD: u64 = 0x0000_0000_0200_0000;
/// `_COLOR_BRIGHT`: 16-color bright background attribute.
pub const COLOR_BRIGHT: u64 = 0x0000_0000_0100_0000;
/// `_COLOR_BG`: 8-bit background color index.
pub const COLOR_BG: u64 = 0x0000_0000_0000_FF00;
/// `_COLOR_FG`: 8-bit foreground color index.
pub const COLOR_FG: u64 = 0x0000_0000_0000_00FF;
/// `_COLOR_24BIT_BG`: 24-bit background color index.
pub const COLOR_24BIT_BG: u64 = 0x00FF_FFFF_0000_0000;
/// `_COLOR_24BIT_FG`: 24-bit foreground color index.
pub const COLOR_24BIT_FG: u64 = 0x0000_0000_00FF_FFFF;

/// C++ `Color::color_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum ColorId {
    Nocolor = 0,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

/// C++ `allColors[]`: English name and index (offset red=3, therefore fg=33,
/// bg=43), in `color_id` order.
const ALL_COLORS: [(&str, u64); 9] = [
    ("none", 0),
    ("black", 1), // fg 29+0  bg 39+0
    ("red", 2),
    ("green", 3),
    ("yellow", 4),
    ("blue", 5),
    ("magenta", 6),
    ("cyan", 7),
    ("white", 8),
];

/// C++ `strtol (text, nullptr, base)` for base 10 or 16: leading whitespace,
/// optional sign, optional `0x`/`0X` (base 16), then digits; saturating at
/// the `long` range; 0 when nothing parses.
fn strtol(text: &str, base: u32) -> i64 {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut negative = false;
    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
        negative = bytes[i] == b'-';
        i += 1;
    }
    if base == 16
        && bytes.get(i) == Some(&b'0')
        && matches!(bytes.get(i + 1), Some(b'x' | b'X'))
        && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
    {
        i += 2;
    }
    let mut value: i64 = 0;
    while let Some(digit) = bytes.get(i).and_then(|&b| (b as char).to_digit(base)) {
        value = if negative {
            value.saturating_mul(i64::from(base)).saturating_sub(i64::from(digit))
        } else {
            value.saturating_mul(i64::from(base)).saturating_add(i64::from(digit))
        };
        i += 1;
    }
    value
}

/// `std::string::compare (0, n, prefix, n) == 0`: the first `n` bytes of
/// `word` equal `prefix`.
fn starts_with(word: &str, prefix: &str) -> bool {
    word.as_bytes().starts_with(prefix.as_bytes())
}

/// `std::string::substr (pos, len)` on bytes (empty past the end).
fn substr(word: &str, pos: usize, len: usize) -> String {
    let bytes = word.as_bytes();
    if pos >= bytes.len() {
        return String::new();
    }
    let end = pos.saturating_add(len).min(bytes.len());
    String::from_utf8_lossy(&bytes[pos..end]).into_owned()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class Color`: a terminal color packed into one `unsigned long`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Color {
    value: u64,
}

impl Color {
    /// `Color ()`: no color.
    pub fn new() -> Self {
        Color { value: 0 }
    }

    /// `Color (unsigned long)`: `256 | INVERSE | UNDERLINE | BOLD | BRIGHT |
    /// (BG << 8) | FG`, masked to the bits valid for its mode.
    pub fn from_value(c: u64) -> Self {
        let value = if c & COLOR_24BIT != 0 {
            c & (COLOR_24BIT
                | COLOR_HASBG
                | COLOR_HASFG
                | COLOR_UNDERLINE
                | COLOR_INVERSE
                | COLOR_BOLD
                | COLOR_BRIGHT
                | COLOR_24BIT_BG
                | COLOR_24BIT_FG)
        } else {
            c & (COLOR_256
                | COLOR_HASBG
                | COLOR_HASFG
                | COLOR_UNDERLINE
                | COLOR_INVERSE
                | COLOR_BOLD
                | COLOR_BRIGHT
                | COLOR_BG
                | COLOR_FG)
        };
        Color { value }
    }

    /// `Color (color_id)`: a foreground color.
    pub fn from_id(fg: ColorId) -> Self {
        let mut value = 0;
        if fg != ColorId::Nocolor {
            value |= COLOR_HASFG;
            value |= fg as u64;
        }
        Color { value }
    }

    /// `Color (color_id fg, color_id bg, bool underline, bool bold, bool
    /// bright)`.
    pub fn from_ids(fg: ColorId, bg: ColorId, underline: bool, bold: bool, bright: bool) -> Self {
        let mut value = (if underline { COLOR_UNDERLINE } else { 0 })
            | (if bold { COLOR_BOLD } else { 0 })
            | (if bright { COLOR_BRIGHT } else { 0 });

        if bg != ColorId::Nocolor {
            value |= COLOR_HASBG;
            value |= (bg as u64) << 8;
        }

        if fg != ColorId::Nocolor {
            value |= COLOR_HASFG;
            value |= fg as u64;
        }

        Color { value }
    }

    /// `Color (const std::string&)`. Supports the following constructs:
    ///   `[bright] [color] [on color] [bright] [underline]`
    ///
    /// Where `[color]` is one of:
    /// - `black`, `red`, ...
    /// - `grayN`    0 <= N <= 23     fg 38;5;232 + N              bg 48;5;232 + N
    /// - `greyN`    0 <= N <= 23     fg 38;5;232 + N              bg 48;5;232 + N
    /// - `colorN`   0 <= N <= 255    fg 38;5;N                    bg 48;5;N
    /// - `rgbRGB`   0 <= R,G,B <= 5  fg 38;5;16 + R*36 + G*6 + B  bg 48;5;16 + R*36 + G*6 + B
    /// - `0xRRGGBB` 0 <= R,G,B <= FF fg 38;2;R;G;B                bg 48;2;R;G;B
    ///
    /// Deviation: an unrecognized word is returned as `Err` instead of
    /// thrown; the message is the C++ one, byte for byte.
    pub fn parse(spec: &str) -> Result<Self> {
        let mut this = Color::new();

        // Split spec into words.
        let words = split(spec, ' ');

        // Construct the color as two separate colors, then blend them later.  This
        // make it possible to declare a color such as "color1 on black", and have
        // the upgrade work properly.
        let mut fg_value: u64 = 0;
        let mut bg_value: u64 = 0;

        let not_recognized = |word: &str| TwError::Message(format("The color '{1}' is not recognized.", &[&word]));

        let mut bg = false;
        for word in words {
            let word = lower_case(&trim(&word));

            if word == "bold" {
                fg_value |= COLOR_BOLD;
            } else if word == "bright" {
                bg_value |= COLOR_BRIGHT;
            } else if word == "underline" {
                fg_value |= COLOR_UNDERLINE;
            } else if word == "inverse" {
                fg_value |= COLOR_INVERSE;
            } else if word == "on" {
                bg = true;
            }
            // X where X is one of black, red, blue ...
            else if let Some(index) = Color::find(&word) {
                if index != 0 {
                    if bg {
                        bg_value |= COLOR_HASBG;
                        bg_value |= (index as u64) << 8;
                    } else {
                        fg_value |= COLOR_HASFG;
                        fg_value |= index as u64;
                    }
                }
            }
            // greyN/grayN, where 0 <= N <= 23.
            else if starts_with(&word, "grey") || starts_with(&word, "gray") {
                let index = strtol(&substr(&word, 4, usize::MAX), 10) as i32;
                if !(0..=23).contains(&index) {
                    return Err(not_recognized(&word));
                }

                if bg {
                    bg_value |= COLOR_HASBG;
                    bg_value |= ((index + 232) as u64) << 8;
                    bg_value |= COLOR_256;
                } else {
                    fg_value |= COLOR_HASFG;
                    fg_value |= (index + 232) as u64;
                    fg_value |= COLOR_256;
                }
            }
            // rgbRGB, where 0 <= R,G,B <= 5.
            else if starts_with(&word, "rgb") {
                let index = strtol(&substr(&word, 3, usize::MAX), 10) as i32;
                if word.len() != 6 || !(0..=555).contains(&index) {
                    return Err(not_recognized(&word));
                }

                let r = strtol(&substr(&word, 3, 1), 10) as i32;
                let g = strtol(&substr(&word, 4, 1), 10) as i32;
                let b = strtol(&substr(&word, 5, 1), 10) as i32;
                if !(0..=5).contains(&r) || !(0..=5).contains(&g) || !(0..=5).contains(&b) {
                    return Err(not_recognized(&word));
                }

                let index = 16 + r * 36 + g * 6 + b;

                if bg {
                    bg_value |= COLOR_HASBG;
                    bg_value |= (index as u64) << 8;
                    bg_value |= COLOR_256;
                } else {
                    fg_value |= COLOR_HASFG;
                    fg_value |= index as u64;
                    fg_value |= COLOR_256;
                }
            }
            // colorN, where 0 <= N <= 255.
            else if starts_with(&word, "color") {
                let index = strtol(&substr(&word, 5, usize::MAX), 10) as i32;
                if !(0..=255).contains(&index) {
                    return Err(not_recognized(&word));
                }

                // As in C++: upgrades `this`, which is overwritten below.
                this.upgrade();

                if bg {
                    bg_value |= COLOR_HASBG;
                    bg_value |= (index as u64) << 8;
                    bg_value |= COLOR_256;
                } else {
                    fg_value |= COLOR_HASFG;
                    fg_value |= index as u64;
                    fg_value |= COLOR_256;
                }
            }
            // 0xX where 0 <= X <= 0xFFFFFF.
            else if starts_with(&word, "0x") {
                let color = strtol(&word, 16) as u64;
                if color > 0x00FF_FFFF {
                    return Err(not_recognized(&word));
                }

                // As in C++: upgrades `this`, which is overwritten below.
                this.upgrade24b();

                if bg {
                    bg_value |= COLOR_HASBG;
                    bg_value |= color << 32;
                    bg_value |= COLOR_24BIT;
                } else {
                    fg_value |= COLOR_HASFG;
                    fg_value |= color;
                    fg_value |= COLOR_24BIT;
                }
            } else if !word.is_empty() {
                return Err(not_recognized(&word));
            }
        }

        // Now combine the fg and bg into a single color.
        this.value = fg_value;
        this.blend(&Color::from_value(bg_value));
        Ok(this)
    }

    /// `operator int ()`: the low 32 bits of the value, as a C++ `int`.
    pub fn to_int(&self) -> i32 {
        self.value as i32
    }

    /// The packed value (C++ `_value`).
    pub fn value(&self) -> u64 {
        self.value
    }

    /// `Color::blend`: if `other` has styles that are compatible, merge them
    /// into this. Colors in `other` take precedence.
    pub fn blend(&mut self, other: &Color) {
        if !other.nontrivial() || *self == *other {
            return;
        }

        let mut c = *other;
        self.value |= c.value & COLOR_UNDERLINE; // Always inherit underline.
        self.value |= c.value & COLOR_INVERSE; // Always inherit inverse.
        self.value |= c.value & COLOR_BOLD; // Always inherit bold.

        // 24 bit <-- 24 bit.
        if (self.value & COLOR_24BIT) != 0 || (c.value & COLOR_24BIT) != 0 {
            // Upgrade either color, if necessary.
            if self.value & COLOR_24BIT == 0 {
                self.upgrade24b();
            }
            if c.value & COLOR_24BIT == 0 {
                c.upgrade24b();
            }

            if c.value & COLOR_HASFG != 0 {
                self.value |= COLOR_HASFG; // There is now a color.
                self.value &= !COLOR_24BIT_FG; // Remove previous color.
                self.value |= c.value & COLOR_24BIT_FG; // Apply other color.
            }

            if c.value & COLOR_HASBG != 0 {
                self.value |= COLOR_HASBG; // There is now a color.
                self.value &= !COLOR_24BIT_BG; // Remove previous color.
                self.value |= c.value & COLOR_24BIT_BG; // Apply other color.
            }
        }
        // 16 <-- 16.
        else if self.value & COLOR_256 == 0 && c.value & COLOR_256 == 0 {
            self.value |= c.value & COLOR_BRIGHT; // Inherit bright.

            if c.value & COLOR_HASFG != 0 {
                self.value |= COLOR_HASFG; // There is now a color.
                self.value &= !COLOR_FG; // Remove previous color.
                self.value |= c.value & COLOR_FG; // Apply other color.
            }

            if c.value & COLOR_HASBG != 0 {
                self.value |= COLOR_HASBG; // There is now a color.
                self.value &= !COLOR_BG; // Remove previous color.
                self.value |= c.value & COLOR_BG; // Apply other color.
            }
        } else {
            // Upgrade either color, if necessary.
            if self.value & COLOR_256 == 0 {
                self.upgrade();
            }
            if c.value & COLOR_256 == 0 {
                c.upgrade();
            }

            // 256 <-- 256.
            if c.value & COLOR_HASFG != 0 {
                self.value |= COLOR_HASFG; // There is now a color.
                self.value &= !COLOR_FG; // Remove previous color.
                self.value |= c.value & COLOR_FG; // Apply other color.
            }

            if c.value & COLOR_HASBG != 0 {
                self.value |= COLOR_HASBG; // There is now a color.
                self.value &= !COLOR_BG; // Remove previous color.
                self.value |= c.value & COLOR_BG; // Apply other color.
            }
        }
    }

    /// `Color::upgrade`: 16-color to 256-color.
    pub fn upgrade(&mut self) {
        if (self.value & COLOR_256) != 0 || (self.value & COLOR_24BIT) != 0 {
            return;
        }

        if self.value & COLOR_HASFG != 0 {
            // C++ `unsigned int` arithmetic.
            let fg = (self.value & COLOR_FG) as u32;
            self.value &= !COLOR_FG;
            self.value |= u64::from(fg.wrapping_sub(1));
        }

        if self.value & COLOR_HASBG != 0 {
            let bright = self.value & COLOR_BRIGHT != 0;
            let bg = ((self.value & COLOR_BG) >> 8) as u32;
            self.value &= !COLOR_BG;
            self.value &= !COLOR_BRIGHT;
            let index = if bright { bg.wrapping_add(7) } else { bg.wrapping_sub(1) };
            self.value |= u64::from(index.wrapping_shl(8));
        }

        self.value |= COLOR_256;
    }

    /// `Color::upgrade24b`: 16/256-color to 24-bit.
    pub fn upgrade24b(&mut self) {
        if self.value & COLOR_24BIT != 0 {
            return;
        }

        self.upgrade();

        self.value &= !COLOR_256;

        if self.value & COLOR_HASFG != 0 {
            let fg = (self.value & COLOR_FG) as u32;
            self.value &= !COLOR_FG;
            self.value |= u64::from(Color::index2truecolor(fg));
        }

        if self.value & COLOR_HASBG != 0 {
            let bg = ((self.value & COLOR_BG) >> 8) as u32;
            // As in C++, this clears the foreground index, not the background.
            self.value &= !COLOR_FG;
            self.value |= u64::from(Color::index2truecolor(bg)) << 32;
        }

        self.value |= COLOR_24BIT;
    }

    /// `Color::index2truecolor`: a 256-color index as `0xRRGGBB`.
    pub fn index2truecolor(index: u32) -> u32 {
        let mut color = 0x0;

        if index <= 8 {
            color |= (index & 1) * 0x80_0000;
            color |= (index & 2) * 0x00_4000;
            color |= (index & 4) * 0x00_0020;
        } else if index <= 16 {
            color |= (index & 1) * 0xFF_0000;
            color |= ((index >> 1) & 1) * 0x00_FF00;
            color |= ((index >> 2) & 1) * 0x00_00FF;
        } else if index <= 231 {
            let index = index - 16;
            color |= (index / 36) * 0x2a_0000;
            color |= ((index % 36) / 6) * 0x00_2a00;
            color |= (index % 6) * 0x00_002a;
        } else {
            color |= (index - 231).wrapping_mul(0x0a_0a0a);
        }

        color
    }

    /// `Color::colorize (const std::string&)`: `input` wrapped in this
    /// color's escape sequence and the reset `\033[0m` (unchanged when there
    /// is no color).
    pub fn colorize(&self, input: &str) -> String {
        let mut result = String::new();
        self.colorize_into(&mut result, input);
        result
    }

    /// `Color::_colorize`: appends the colorized `input` to `result`.
    ///
    /// Sample color codes:
    /// - red                  `\033[31m`
    /// - bold red             `\033[91m`
    /// - underline red        `\033[4;31m`
    /// - bold underline red   `\033[1;4;31m`
    /// - on red               `\033[41m`
    /// - on bright red        `\033[101m`
    /// - 256 fg               `\033[38;5;Nm`
    /// - 256 bg               `\033[48;5;Nm`
    /// - 24 bit fg            `\033[38;2;R;G;Bm`
    /// - 24 bit bg            `\033[48;2;R;G;Bm`
    ///
    /// Deviation: C++ looks the numbers up in the 256-entry `colorstring`
    /// table, which is read out of bounds (undefined behaviour) for a
    /// 16-color foreground index above 226 or background index above 216
    /// (156 with bright); such values are only reachable through
    /// `Color (unsigned long)`. Rust formats the number in decimal, which is
    /// what the table holds for every in-range index.
    pub fn colorize_into(&self, result: &mut String, input: &str) {
        if !self.nontrivial() {
            result.push_str(input);
            return;
        }

        let mut count = 0;

        result.push_str("\x1b[");

        if self.value & COLOR_BOLD != 0 {
            result.push('1');
            count += 1;
        }

        let mut separator = |result: &mut String| {
            if count > 0 {
                result.push(';');
            }
            count += 1;
        };

        if self.value & COLOR_UNDERLINE != 0 {
            separator(result);
            result.push('4');
        }

        if self.value & COLOR_INVERSE != 0 {
            separator(result);
            result.push('7');
        }

        if self.value & COLOR_HASFG != 0 {
            separator(result);
            if self.value & COLOR_24BIT != 0 {
                result.push_str("38;2;");
                result.push_str(&format!(
                    "{};{};{}",
                    (self.value >> 16) & 0xFF,
                    (self.value >> 8) & 0xFF,
                    self.value & 0xFF
                ));
            } else if self.value & COLOR_256 != 0 {
                result.push_str("38;5;");
                result.push_str(&(self.value & COLOR_FG).to_string());
            } else {
                result.push_str(&(29 + (self.value & COLOR_FG)).to_string());
            }
        }

        if self.value & COLOR_HASBG != 0 {
            separator(result);
            if self.value & COLOR_24BIT != 0 {
                result.push_str("48;2;");
                result.push_str(&format!(
                    "{};{};{}",
                    (self.value >> 48) & 0xFF,
                    (self.value >> 40) & 0xFF,
                    (self.value >> 32) & 0xFF
                ));
            } else if self.value & COLOR_256 != 0 {
                result.push_str("48;5;");
                result.push_str(&((self.value & COLOR_BG) >> 8).to_string());
            } else {
                let base = if self.value & COLOR_BRIGHT != 0 { 99 } else { 39 };
                result.push_str(&(base + ((self.value & COLOR_BG) >> 8)).to_string());
            }
        }

        result.push('m');
        result.push_str(input);
        result.push_str("\x1b[0m");
    }

    /// `Color::strip`: removes color codes (`ESC` up to and including the
    /// next `m`) from a string.
    pub fn strip(input: &str) -> String {
        let mut inside = false;
        let mut output = Vec::with_capacity(input.len());
        for &b in input.as_bytes() {
            if inside {
                if b == b'm' {
                    inside = false;
                }
            } else if b == 0x1b {
                inside = true;
            } else {
                output.push(b);
            }
        }

        // Only ASCII bytes end an escape sequence, so the result is valid UTF-8.
        String::from_utf8(output).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
    }

    /// Static `Color::colorize (input, spec)`.
    ///
    /// Deviation: named `colorize_with` (no overloading in Rust); an
    /// unrecognized `spec` is returned as `Err` instead of thrown.
    pub fn colorize_with(input: &str, spec: &str) -> Result<String> {
        let c = Color::parse(spec)?;
        Ok(c.colorize(input))
    }

    /// `Color::code`: the escape sequence(s) that start this color, without
    /// the reset. As in C++, bold is only emitted in 16-color mode.
    ///
    /// Deviation: `colorstring` out-of-range lookups, as in
    /// [`Color::colorize_into`].
    pub fn code(&self) -> String {
        if !self.nontrivial() {
            return String::new();
        }

        let mut result = String::new();

        // 24 bit color
        if self.value & COLOR_24BIT != 0 {
            if self.value & COLOR_UNDERLINE != 0 {
                result.push_str("\x1b[4m");
            }

            if self.value & COLOR_INVERSE != 0 {
                result.push_str("\x1b[7m");
            }

            if self.value & COLOR_HASFG != 0 {
                result.push_str(&format!(
                    "\x1b[38;2;{};{};{}m",
                    (self.value >> 16) & 0xFF,
                    (self.value >> 8) & 0xFF,
                    self.value & 0xFF
                ));
            }

            if self.value & COLOR_HASBG != 0 {
                result.push_str(&format!(
                    "\x1b[48;2;{};{};{}m",
                    (self.value >> 48) & 0xFF,
                    (self.value >> 40) & 0xFF,
                    (self.value >> 32) & 0xFF
                ));
            }
        }
        // 256 color
        else if self.value & COLOR_256 != 0 {
            if self.value & COLOR_UNDERLINE != 0 {
                result.push_str("\x1b[4m");
            }

            if self.value & COLOR_INVERSE != 0 {
                result.push_str("\x1b[7m");
            }

            if self.value & COLOR_HASFG != 0 {
                result.push_str(&format!("\x1b[38;5;{}m", self.value & COLOR_FG));
            }

            if self.value & COLOR_HASBG != 0 {
                result.push_str(&format!("\x1b[48;5;{}m", (self.value & COLOR_BG) >> 8));
            }
        }
        // 16 color
        else {
            let mut count = 0;
            result.push_str("\x1b[");

            let mut separator = |result: &mut String| {
                if count > 0 {
                    result.push(';');
                }
                count += 1;
            };

            if self.value & COLOR_BOLD != 0 {
                separator(&mut result);
                result.push('1');
            }

            if self.value & COLOR_UNDERLINE != 0 {
                separator(&mut result);
                result.push('4');
            }

            if self.value & COLOR_INVERSE != 0 {
                separator(&mut result);
                result.push('7');
            }

            if self.value & COLOR_HASFG != 0 {
                separator(&mut result);
                result.push_str(&(29 + (self.value & COLOR_FG)).to_string());
            }

            if self.value & COLOR_HASBG != 0 {
                separator(&mut result);
                let base = if self.value & COLOR_BRIGHT != 0 { 99 } else { 39 };
                result.push_str(&(base + ((self.value & COLOR_BG) >> 8)).to_string());
            }

            result.push('m');
        }

        result
    }

    /// `Color::end`: the reset sequence, if there is a color.
    pub fn end(&self) -> String {
        if self.nontrivial() {
            return "\x1b[0m".to_string();
        }

        String::new()
    }

    /// `Color::nontrivial`: whether there is any color or attribute.
    pub fn nontrivial(&self) -> bool {
        self.value != 0
    }

    /// `Color::find`: the `allColors` index of an English color name.
    fn find(input: &str) -> Option<usize> {
        ALL_COLORS.iter().position(|(name, _)| *name == input)
    }

    /// `Color::fg`: the foreground's name.
    fn fg(&self) -> String {
        if self.value & COLOR_24BIT != 0 {
            if self.value & COLOR_HASFG != 0 {
                return format!("0x{:06x}", self.value & COLOR_24BIT_FG);
            }
        } else if self.value & COLOR_256 != 0 {
            if self.value & COLOR_HASFG != 0 {
                return format!("color{}", self.value & COLOR_FG);
            }
        } else {
            let index = self.value & COLOR_FG;
            if let Some((name, _)) = ALL_COLORS.iter().find(|(_, i)| *i == index) {
                return name.to_string();
            }
        }

        String::new()
    }

    /// `Color::bg`: the background's name.
    fn bg(&self) -> String {
        if self.value & COLOR_24BIT != 0 {
            if self.value & COLOR_HASBG != 0 {
                return format!("0x{:06x}", (self.value & COLOR_24BIT_BG) >> 32);
            }
        } else if self.value & COLOR_256 != 0 {
            if self.value & COLOR_HASBG != 0 {
                return format!("color{}", (self.value & COLOR_BG) >> 8);
            }
        } else {
            let index = (self.value & COLOR_BG) >> 8;
            if let Some((name, _)) = ALL_COLORS.iter().find(|(_, i)| *i == index) {
                return name.to_string();
            }
        }

        String::new()
    }
}

impl PartialEq for Color {
    /// `Color::operator==`: compares `int (_value)`, i.e. the low 32 bits.
    fn eq(&self, other: &Self) -> bool {
        self.to_int() == other.to_int()
    }
}

impl Eq for Color {}

impl fmt::Display for Color {
    /// `operator std::string ()`: the color's description, e.g.
    /// `bold underline red on bright white`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sep = |description: &String| if description.is_empty() { "" } else { " " };

        let mut description = String::new();
        if self.value & COLOR_BOLD != 0 {
            description.push_str("bold");
        }

        if self.value & COLOR_UNDERLINE != 0 {
            description = format!("{}{}underline", description, sep(&description));
        }

        if self.value & COLOR_INVERSE != 0 {
            description = format!("{}{}inverse", description, sep(&description));
        }

        if self.value & COLOR_HASFG != 0 {
            description = format!("{}{}{}", description, sep(&description), self.fg());
        }

        if self.value & COLOR_HASBG != 0 {
            description = format!("{}{}on", description, sep(&description));

            if self.value & COLOR_BRIGHT != 0 {
                description = format!("{}{}bright", description, sep(&description));
            }

            description.push(' ');
            description.push_str(&self.bg());
        }

        f.write_str(&description)
    }
}

impl From<Color> for String {
    /// `(std::string) color`.
    fn from(color: Color) -> Self {
        color.to_string()
    }
}

impl From<Color> for i32 {
    /// `(int) color`.
    fn from(color: Color) -> Self {
        color.to_int()
    }
}

impl From<u64> for Color {
    /// `Color (unsigned long)`.
    fn from(value: u64) -> Self {
        Color::from_value(value)
    }
}

impl From<ColorId> for Color {
    /// `Color (color_id)`.
    fn from(fg: ColorId) -> Self {
        Color::from_id(fg)
    }
}

impl std::str::FromStr for Color {
    type Err = TwError;

    /// `Color (const std::string&)`; see [`Color::parse`].
    fn from_str(spec: &str) -> Result<Self> {
        Color::parse(spec)
    }
}

impl TryFrom<&str> for Color {
    type Error = TwError;

    /// `Color (const std::string&)`; see [`Color::parse`].
    fn try_from(spec: &str) -> Result<Self> {
        Color::parse(spec)
    }
}

impl TryFrom<&String> for Color {
    type Error = TwError;

    /// `Color (const std::string&)`; see [`Color::parse`].
    fn try_from(spec: &String) -> Result<Self> {
        Color::parse(spec)
    }
}

#[cfg(test)]
mod tests {
    //! Line-by-line port of libshared (d899eba6) `test/color.t.cpp`.
    //!
    //! The C++ `UnitTest` harness is mirrored by [`UnitTest`]: every `t.is`
    //! becomes a call with the identical message, in the same order, so each
    //! upstream assertion has exactly one counterpart here. The run collects
    //! all failures and then checks the number of executed assertions against
    //! the static count of the C++ file, [`COLOR_T_ASSERTIONS`].
    //!
    //! Deviation: a C++ `Color (spec)` that throws would abort the program;
    //! here it panics through `unwrap`, failing the test the same way.
    use super::*;
    use std::fmt::Debug;

    /// Static count of the assertions executed by color.t.cpp: 9 names + 4
    /// upgrades + 5 blends + 4 attribute codes + 9 foregrounds + 9
    /// backgrounds (= 40), 256 + 256 `colorN`, 6*6*6 + 6*6*6 `rgbRGB`, 1
    /// `grey0`, 24 + 24 `grayN`, 7 24-bit, 3 `strip` + 2 `code` + 1 `end`.
    /// Equal to its `UnitTest t (...)` plan.
    const COLOR_T_ASSERTIONS: usize = 40 + 256 + 256 + 6 * 6 * 6 + 6 * 6 * 6 + 1 + 24 + 24 + 7 + 6;

    /// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
    struct UnitTest {
        counter: usize,
        failures: Vec<String>,
    }

    impl UnitTest {
        fn new() -> Self {
            UnitTest { counter: 0, failures: Vec::new() }
        }

        fn is<A: PartialEq<E> + Debug, E: Debug>(&mut self, actual: A, expected: E, name: impl AsRef<str>) {
            self.counter += 1;
            if actual != expected {
                self.failures.push(format!(
                    "not ok {} - {}\n#   expected: {:?}\n#        got: {:?}",
                    self.counter,
                    name.as_ref(),
                    expected,
                    actual
                ));
            }
        }
    }

    /// `Color (spec)`.
    fn color(spec: &str) -> Color {
        Color::parse(spec).unwrap()
    }

    /// Static `Color::colorize (input, spec)`.
    fn colorize(input: &str, spec: &str) -> String {
        Color::colorize_with(input, spec).unwrap()
    }

    /// `int main ()` of color.t.cpp.
    fn color_t(t: &mut UnitTest) {
        // Names matched to values.
        t.is(color("").to_int(), Color::from_id(ColorId::Nocolor).to_int(), "''        == Color::nocolor");
        t.is(color("black").to_int(), Color::from_id(ColorId::Black).to_int(), "'black'   == Color::black");
        t.is(color("red").to_int(), Color::from_id(ColorId::Red).to_int(), "'red'     == Color::red");
        t.is(color("green").to_int(), Color::from_id(ColorId::Green).to_int(), "'green'   == Color::green");
        t.is(color("yellow").to_int(), Color::from_id(ColorId::Yellow).to_int(), "'yellow'  == Color::yellow");
        t.is(color("blue").to_int(), Color::from_id(ColorId::Blue).to_int(), "'blue'    == Color::blue");
        t.is(color("magenta").to_int(), Color::from_id(ColorId::Magenta).to_int(), "'magenta' == Color::magenta");
        t.is(color("cyan").to_int(), Color::from_id(ColorId::Cyan).to_int(), "'cyan'    == Color::cyan");
        t.is(color("white").to_int(), Color::from_id(ColorId::White).to_int(), "'white'   == Color::white");

        // Auto upgrades.
        let mut c = color("red on color0");
        t.is(c.to_string(), "color1 on color0", "upgrade red on color0 -> color1 on color0");

        c = color("color1 on black");
        t.is(c.to_string(), "color1 on color0", "upgrade color1 on black -> color1 on color0");

        c = color("bold red on color0");
        t.is(c.to_string(), "bold color1 on color0", "upgrade bold red on color0 -> bold color1 on color0");

        c = color("color1 on bright black");
        t.is(c.to_string(), "color1 on color8", "upgrade color1 on bright black -> color1 on color8");

        // Simple blending.
        c = color("red");
        c.blend(&color("on white"));
        t.is(c.to_string(), "red on white", "red + on white -> red on white");

        c = color("bold underline red");
        c.blend(&color("on bright white"));
        t.is(
            c.to_string(),
            "bold underline red on bright white",
            "bold underline red + on bright white -> bold underline red on bright white",
        );

        // Blending with conflicts.
        c = color("red on white");
        c.blend(&color("on blue"));
        t.is(c.to_string(), "red on blue", "red on white + on blue -> red on blue");

        c = color("red on white");
        c.blend(&color("blue on magenta"));
        t.is(c.to_string(), "blue on magenta", "red on white + blue on magenta -> blue on magenta");

        // Blending with upgrades.
        c = color("color1 on color0");
        c.blend(&color("blue"));
        t.is(c.to_string(), "color4 on color0", "color1 on color0 + blue -> color4 on color0");

        // Now the dumb show of every color and its code.
        t.is(colorize("foo", "red"), "\x1b[31mfoo\x1b[0m", "red                -> ^[[31m");
        t.is(colorize("foo", "bold red"), "\x1b[1;31mfoo\x1b[0m", "bold red           -> ^[[1;31m");
        t.is(colorize("foo", "underline red"), "\x1b[4;31mfoo\x1b[0m", "underline red      -> ^[[4;31m");
        t.is(colorize("foo", "underline bold red"), "\x1b[1;4;31mfoo\x1b[0m", "underline bold red -> ^[[1;4;31m");

        // 16-color foregrounds.
        t.is(colorize("foo", ""), "foo", "''                 -> ''");

        t.is(colorize("foo", "black"), "\x1b[30mfoo\x1b[0m", "black              -> ^[[30m");
        t.is(colorize("foo", "red"), "\x1b[31mfoo\x1b[0m", "red                -> ^[[31m");
        t.is(colorize("foo", "green"), "\x1b[32mfoo\x1b[0m", "green              -> ^[[32m");
        t.is(colorize("foo", "yellow"), "\x1b[33mfoo\x1b[0m", "yellow             -> ^[[33m");
        t.is(colorize("foo", "blue"), "\x1b[34mfoo\x1b[0m", "blue               -> ^[[34m");
        t.is(colorize("foo", "magenta"), "\x1b[35mfoo\x1b[0m", "magenta            -> ^[[35m");
        t.is(colorize("foo", "cyan"), "\x1b[36mfoo\x1b[0m", "cyan               -> ^[[36m");
        t.is(colorize("foo", "white"), "\x1b[37mfoo\x1b[0m", "white              -> ^[[37m");

        // 16-color backgrounds.
        t.is(colorize("foo", "on bright black"), "\x1b[100mfoo\x1b[0m", "on bright black    -> ^[[100m");

        t.is(colorize("foo", "on black"), "\x1b[40mfoo\x1b[0m", "on black           -> ^[[40m");
        t.is(colorize("foo", "on red"), "\x1b[41mfoo\x1b[0m", "on red             -> ^[[41m");
        t.is(colorize("foo", "on green"), "\x1b[42mfoo\x1b[0m", "on green           -> ^[[42m");
        t.is(colorize("foo", "on yellow"), "\x1b[43mfoo\x1b[0m", "on yellow          -> ^[[43m");
        t.is(colorize("foo", "on blue"), "\x1b[44mfoo\x1b[0m", "on blue            -> ^[[44m");
        t.is(colorize("foo", "on magenta"), "\x1b[45mfoo\x1b[0m", "on magenta         -> ^[[45m");
        t.is(colorize("foo", "on cyan"), "\x1b[46mfoo\x1b[0m", "on cyan            -> ^[[46m");
        t.is(colorize("foo", "on white"), "\x1b[47mfoo\x1b[0m", "on white           -> ^[[47m");

        // 256-color, basic colors.
        for i in 0..256 {
            let color = format!("color{i}");
            let codes = format!("\x1b[38;5;{i}mfoo\x1b[0m");
            let description = format!("\x1b[38;5;{i}mfoo\x1b[0m: color{i} -> ^[[38;5;{i}m");

            t.is(colorize("foo", &color), codes, description);
        }

        for i in 0..256 {
            let color = format!("on color{i}");
            let codes = format!("\x1b[48;5;{i}mfoo\x1b[0m");
            let description = format!("\x1b[48;5;{i}mfoo\x1b[0m: on color{i} -> ^[[48;5;{i}m");

            t.is(colorize("foo", &color), codes, description);
        }

        // RGB Color Cube.
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    let code = 16 + (r * 36 + g * 6 + b);
                    let color = format!("rgb{r}{g}{b}");
                    let codes = format!("\x1b[38;5;{code}mfoo\x1b[0m");
                    let description = format!("\x1b[38;5;{code}mfoo\x1b[0m: rgb{r}{g}{b} -> ^[[38;5;{code}m");

                    t.is(colorize("foo", &color), codes, description);
                }
            }
        }

        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    let code = 16 + (r * 36 + g * 6 + b);
                    let color = format!("on rgb{r}{g}{b}");
                    let codes = format!("\x1b[48;5;{code}mfoo\x1b[0m");
                    let description = format!("\x1b[48;5;{code}mfoo\x1b[0m: on rgb{r}{g}{b} -> ^[[48;5;{code}m");

                    t.is(colorize("foo", &color), codes, description);
                }
            }
        }

        // 256-color, grays.
        // grey == gray.
        t.is(colorize("foo", "grey0"), "\x1b[38;5;232mfoo\x1b[0m", "\x1b[38;5;232mfoo\x1b[0m: grey0 -> ^[[38;5;232m");

        for i in 0..24 {
            let color = format!("gray{i}");
            let codes = format!("\x1b[38;5;{}mfoo\x1b[0m", i + 232);
            let description = format!("\x1b[38;5;{}mfoo\x1b[0m: gray{} -> ^[[38;5;{}m", i + 232, i, i + 232);

            t.is(colorize("foo", &color), codes, description);
        }

        for i in 0..24 {
            let color = format!("on gray{i}");
            let codes = format!("\x1b[48;5;{}mfoo\x1b[0m", i + 232);
            let description = format!("\x1b[48;5;{}mfoo\x1b[0m: on gray{} -> ^[[48;5;{}m", i + 232, i, i + 232);

            t.is(colorize("foo", &color), codes, description);
        }

        // 24-bit color
        t.is(
            colorize("foo", "0x0a1b2c"),
            "\x1b[38;2;10;27;44mfoo\x1b[0m",
            "\x1b[38;2;10;27;44mfoo\x1b[0m: 0x0a1b2c               -> ^[[38;2;10;27;44m",
        );
        t.is(
            colorize("foo", "0x0a1b2c on color127"),
            "\x1b[38;2;10;27;44;48;2;126;0;126mfoo\x1b[0m",
            "\x1b[38;2;10;27;44;48;2;126;0;126mfoo\x1b[0m: 0x0a1b2c on color127   -> ^[[38;2;10;27;44;48;2;126;0;126m",
        );
        t.is(
            colorize("foo", "red on 0x0a1b2c"),
            "\x1b[38;2;128;0;0;48;2;10;27;44mfoo\x1b[0m",
            "\x1b[38;2;128;0;0;48;2;10;27;44mfoo\x1b[0m: red on 0x0a1b2c        -> ^[[38;2;128;0;0;48;2;10;27;44m",
        );
        t.is(
            colorize("foo", "bold red on 0x0a1b2c"),
            "\x1b[1;38;2;128;0;0;48;2;10;27;44mfoo\x1b[0m",
            "\x1b[1;38;2;128;0;0;48;2;10;27;44mfoo\x1b[0m: bold on 0x0a1b2c       -> ^[[1;38;2;255;0;0;48;2;10;27;44m",
        );
        t.is(
            colorize("foo", "0x0a1b2c on bright red"),
            "\x1b[38;2;10;27;44;48;2;255;0;0mfoo\x1b[0m",
            "\x1b[38;2;10;27;44;48;2;255;0;0mfoo\x1b[0m: 0x0a1b2c on bright red -> ^[[38;2;10;27;44;48;2;255;0;0m",
        );
        t.is(
            colorize("foo", "0x010101 on grey0"),
            "\x1b[38;2;1;1;1;48;2;10;10;10mfoo\x1b[0m",
            "\x1b[38;2;1;1;1;48;2;10;10;10mfoo\x1b[0m: 0x010101 on grey0      -> ^[[38;2;1;1;1;48;2;10;10;10m",
        );
        t.is(
            colorize("foo", "0x101010 on grey23"),
            "\x1b[38;2;16;16;16;48;2;240;240;240mfoo\x1b[0m",
            "\x1b[38;2;16;16;16;48;2;240;240;240mfoo\x1b[0m: 0x101010 on grey23     -> ^[[38;2;16;16;16;48;2;240;240;240m",
        );

        // std::string Color::strip (const std::string&);
        t.is(Color::strip(""), "", "Color::strip '' -> ''");
        t.is(Color::strip("foo"), "foo", "Color::strip 'foo' -> 'foo'");
        t.is(Color::strip("f\x1b[1mo\x1b[0mo"), "foo", "Color::strip 'f<b>o</b>o' -> 'foo'");

        // std::string Color::code () const;
        t.is(
            color("rgb012 on bright yellow").code(),
            "\x1b[38;5;24m\x1b[48;5;11m",
            "Color::code 'rgb0123 on bright yellow' --> ^[[38;5;24m^[[48;5;11m",
        );
        t.is(color("0x0F1E2D").code(), "\x1b[38;2;15;30;45m", "Color::code '0x0F1E2D' --> ^[[38;2;15;30;45m");

        // std::string Color::end () const;
        t.is(color("red").end(), "\x1b[0m", "Color::end --> ^[[0m");
    }

    #[test]
    fn color_t_cpp() {
        let mut t = UnitTest::new();
        color_t(&mut t);
        assert!(
            t.failures.is_empty(),
            "{} of {} assertions failed:\n{}",
            t.failures.len(),
            t.counter,
            t.failures.join("\n")
        );
        assert_eq!(t.counter, COLOR_T_ASSERTIONS, "number of assertions executed");
    }

    // Rust-only tests below (not part of color.t.cpp).

    #[test]
    fn bold_alone() {
        assert_eq!(colorize("task", "bold"), "\x1b[1mtask\x1b[0m");
        assert_eq!(color("bold").to_string(), "bold");
        assert_eq!(color("bold").code(), "\x1b[1m");
    }

    #[test]
    fn attribute_only_and_inverse() {
        assert_eq!(colorize("x", "inverse"), "\x1b[7mx\x1b[0m");
        assert_eq!(colorize("x", "bold underline inverse red on bright green"), "\x1b[1;4;7;31;102mx\x1b[0m");
        assert_eq!(color("bold underline inverse red on bright green").code(), "\x1b[1;4;7;31;102m");
        assert_eq!(color("bold underline inverse red on bright green").to_string(), "bold underline inverse red on bright green");
        // "bright" without a background: C++ emits an empty SGR sequence.
        assert_eq!(colorize("x", "bright"), "\x1b[mx\x1b[0m");
        assert_eq!(color("bright").to_string(), "");
        // Bold is not emitted by code () in 256/24-bit mode.
        assert_eq!(color("bold underline color3").code(), "\x1b[4m\x1b[38;5;3m");
        assert_eq!(color("inverse 0x010203 on 0x040506").code(), "\x1b[7m\x1b[38;2;1;2;3m\x1b[48;2;4;5;6m");
        assert_eq!(color("0x0a1b2c on 0xFFFFFF").to_string(), "0x0a1b2c on 0xffffff");
        assert_eq!(color("  RED   On  Blue ").to_string(), "red on blue");
        assert_eq!(color("none").to_int(), 0);
    }

    #[test]
    fn strtol_quirks() {
        // strtol stops at the first non-digit and returns 0 for no digits.
        assert_eq!(color("grey").to_string(), "color232");
        assert_eq!(color("grayx").to_string(), "color232");
        assert_eq!(color("color").to_string(), "color0");
        assert_eq!(color("rgb+12").to_string(), "color24");
        assert_eq!(color("0x").to_string(), "0x000000");
    }

    #[test]
    fn unrecognized() {
        for bad in ["purple", "gray24", "gray-1", "rgb600", "rgb12", "rgb1234", "color256", "color-1", "0x1000000"] {
            let err = Color::parse(bad).unwrap_err();
            assert_eq!(err.to_string(), format!("The color '{bad}' is not recognized."));
        }
        assert_eq!(Color::parse("Purple").unwrap_err().to_string(), "The color 'purple' is not recognized.");
        assert_eq!(Color::colorize_with("x", "red on mauve").unwrap_err().to_string(), "The color 'mauve' is not recognized.");
    }

    #[test]
    fn value_constructor_masks() {
        assert_eq!(Color::from_value(0).value(), 0);
        assert_eq!(Color::from_value(COLOR_HASFG | 2 | COLOR_24BIT_BG).value(), COLOR_HASFG | 2);
        assert_eq!(
            Color::from_value(COLOR_24BIT | COLOR_HASFG | COLOR_256 | 0x123456).value(),
            COLOR_24BIT | COLOR_HASFG | 0x123456
        );
        assert!(!Color::default().nontrivial());
        assert_eq!(
            Color::from_ids(ColorId::Red, ColorId::Blue, true, true, true).to_string(),
            "bold underline red on bright blue"
        );
        assert_eq!(Color::from_ids(ColorId::Nocolor, ColorId::Nocolor, true, false, false).code(), "\x1b[4m");
    }

    #[test]
    fn equality_and_blend_short_circuit() {
        // operator== compares the low 32 bits only.
        assert_eq!(color("0x000001 on 0x00ff00"), color("0x000001 on 0x0000ff"));
        assert_ne!(color("0x000001"), color("0x000001 on 0x0000ff"));
        let mut c = color("red");
        c.blend(&Color::new());
        assert_eq!(c.to_string(), "red");
        c.blend(&color("red"));
        assert_eq!(c.to_string(), "red");
    }

    #[test]
    fn upgrades_and_truecolor() {
        let mut c = color("red on bright blue");
        c.upgrade();
        assert_eq!(c.to_string(), "color1 on color12");
        c.upgrade24b();
        // As in C++: the 256-color background index (12 = 0x0c) is left in
        // bits 8-15, inside the 24-bit foreground.
        assert_eq!(c.to_string(), "0x800c00 on 0x0000ff");
        assert_eq!(Color::index2truecolor(1), 0x80_0000);
        assert_eq!(Color::index2truecolor(8), 0);
        assert_eq!(Color::index2truecolor(15), 0xFF_FFFF);
        assert_eq!(Color::index2truecolor(16), 0);
        assert_eq!(Color::index2truecolor(231), 0xD2_D2D2);
        assert_eq!(Color::index2truecolor(255), 0xF0_F0F0);
    }

    #[test]
    fn colorize_into_appends() {
        let mut out = String::from(">");
        color("red").colorize_into(&mut out, "a");
        Color::new().colorize_into(&mut out, "b");
        assert_eq!(out, ">\x1b[31ma\x1b[0mb");
    }

    #[test]
    fn conversions() {
        let c: Color = "bold red".parse().unwrap();
        assert_eq!(String::from(c), "bold red");
        assert_eq!(Color::try_from("on blue").unwrap().to_string(), "on blue");
        assert_eq!(Color::try_from(&String::from("green")).unwrap().to_string(), "green");
        assert!("nope".parse::<Color>().is_err());
        assert_eq!(Color::from(ColorId::Red), Color::from_id(ColorId::Red));
        assert_eq!(Color::from(COLOR_HASFG | 3).to_string(), "green");
        assert_eq!(i32::from(color("red")), (COLOR_HASFG | 2) as i32);
    }
}
