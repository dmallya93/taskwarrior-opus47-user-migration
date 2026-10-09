//! Rust translation of `src/Version.cpp` plus the package identification
//! macros from `cmake.h.in` / `CMakeLists.txt`.

use std::fmt;

/// `PACKAGE` (CMake `PROJECT_NAME`).
pub const PACKAGE: &str = "task";
/// `VERSION` (CMake `PROJECT_VERSION`).
pub const VERSION: &str = "3.5.0";
/// `PACKAGE_BUGREPORT`.
pub const PACKAGE_BUGREPORT: &str = "support@gothenburgbitfactory.org";
/// `PACKAGE_NAME`.
pub const PACKAGE_NAME: &str = PACKAGE;
/// `PACKAGE_TARNAME`.
pub const PACKAGE_TARNAME: &str = PACKAGE;
/// `PACKAGE_VERSION`.
pub const PACKAGE_VERSION: &str = VERSION;
/// `PACKAGE_STRING` (`"${PACKAGE} ${VERSION}"`).
pub const PACKAGE_STRING: &str = "task 3.5.0";
/// CMake `HOMEPAGE_URL` of the `project()` declaration.
pub const PACKAGE_URL: &str = "https://taskwarrior.org/";

/// A Taskwarrior version of the form `major.minor.patch`.
///
/// An invalid version (C++ default constructor, or a failed parse) has all
/// three components set to -1. Ordering compares `(major, minor, patch)`
/// lexicographically, as `std::tie` does in the C++ operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    major: i32,
    minor: i32,
    patch: i32,
}

impl Default for Version {
    /// `Version()`: an invalid version.
    fn default() -> Self {
        Version {
            major: -1,
            minor: -1,
            patch: -1,
        }
    }
}

/// Mimics `std::stringstream(part) >> value` for an `int`: skips leading
/// whitespace, accepts an optional sign and at least one digit, ignores any
/// trailing characters, and fails on overflow.
fn extract_int(part: &str) -> Option<i32> {
    let s = part.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\u{0b}');
    let bytes = s.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits_start {
        return None;
    }
    s[..end].parse::<i32>().ok()
}

impl Version {
    /// `Version(std::string)`: parse `digits.digits.digits`; anything else
    /// yields an invalid version.
    pub fn new(version: &str) -> Self {
        let mut parts = Vec::new();
        // std::getline does not yield a trailing empty segment at end of input.
        let mut segments: Vec<&str> = version.split('.').collect();
        if segments.last() == Some(&"") {
            segments.pop();
        }
        for part in segments {
            match extract_int(part) {
                Some(value) if value >= 0 => parts.push(value),
                _ => return Version::default(),
            }
        }
        if parts.len() != 3 {
            return Version::default();
        }
        Version {
            major: parts[0],
            minor: parts[1],
            patch: parts[2],
        }
    }

    /// `Version::Current()`: the release being built.
    pub fn current() -> Self {
        Version::new(PACKAGE_VERSION)
    }

    /// `Version::is_valid()`.
    pub fn is_valid(&self) -> bool {
        self.major >= 0
    }
}

impl fmt::Display for Version {
    /// `operator std::string()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_valid() {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        } else {
            f.write_str("(invalid version)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid() {
        assert_eq!(Version::new("3.5.0").to_string(), "3.5.0");
        assert!(Version::new("1.2.3").is_valid());
        assert_eq!(Version::current().to_string(), VERSION);
    }

    #[test]
    fn parse_invalid() {
        for bad in ["", "1.2", "1.2.3.4", "a.b.c", "1..3", "1.-2.3", "-1.0.0"] {
            assert!(!Version::new(bad).is_valid(), "{bad}");
            assert_eq!(Version::new(bad).to_string(), "(invalid version)");
        }
        assert!(!Version::default().is_valid());
    }

    #[test]
    fn parse_like_iostream() {
        // Trailing garbage after the digits is ignored by operator>>.
        assert_eq!(Version::new("1.2.3beta").to_string(), "1.2.3");
        // A trailing '.' produces no extra getline segment.
        assert_eq!(Version::new("1.2.3.").to_string(), "1.2.3");
    }

    #[test]
    fn ordering() {
        let v = Version::new;
        assert!(v("2.6.0") < v("3.0.0"));
        assert!(v("3.0.0") <= v("3.0.0"));
        assert!(v("3.1.0") > v("3.0.9"));
        assert!(v("3.0.10") >= v("3.0.9"));
        assert_eq!(v("3.5.0"), Version::current());
        assert_ne!(v("3.5.1"), Version::current());
    }
}
