//! Crate-wide error type (spec Design Decision 7).
//!
//! The C++ code throws `std::string` (user-facing messages) and `int` (exit
//! statuses) in many places; both become variants of [`TwError`] and are
//! propagated with [`Result`]. `main.rs` maps them to the exact stderr text and
//! exit codes of `main.cpp`.

use std::fmt;

/// Every error Taskwarrior can raise.
#[derive(Debug)]
pub enum TwError {
    /// `throw std::string`: a user-facing message.
    Message(String),
    /// `throw int`: an exit status with no message.
    Status(i32),
    /// An I/O failure.
    Io(std::io::Error),
    /// An error reported by the taskchampion replica.
    TaskChampion(taskchampion::Error),
    /// A registered command whose port has not landed yet; carries the keyword.
    NotImplemented(String),
}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, TwError>;

impl fmt::Display for TwError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TwError::Message(message) => f.write_str(message),
            TwError::Status(_) => Ok(()),
            TwError::Io(err) => write!(f, "{err}"),
            TwError::TaskChampion(err) => write!(f, "{err}"),
            TwError::NotImplemented(keyword) => {
                write!(f, "Command '{keyword}' is not yet implemented.")
            }
        }
    }
}

impl std::error::Error for TwError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TwError::Io(err) => Some(err),
            TwError::TaskChampion(err) => Some(err),
            TwError::Message(_) | TwError::Status(_) | TwError::NotImplemented(_) => None,
        }
    }
}

impl From<std::io::Error> for TwError {
    fn from(err: std::io::Error) -> Self {
        TwError::Io(err)
    }
}

impl From<taskchampion::Error> for TwError {
    fn from(err: taskchampion::Error) -> Self {
        TwError::TaskChampion(err)
    }
}

impl From<String> for TwError {
    fn from(message: String) -> Self {
        TwError::Message(message)
    }
}

impl From<&str> for TwError {
    fn from(message: &str) -> Self {
        TwError::Message(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_message() {
        assert_eq!(TwError::Message("boom".into()).to_string(), "boom");
    }

    #[test]
    fn display_status_is_empty() {
        assert_eq!(TwError::Status(2).to_string(), "");
    }

    #[test]
    fn display_not_implemented() {
        assert_eq!(
            TwError::NotImplemented("burndown".into()).to_string(),
            "Command 'burndown' is not yet implemented."
        );
    }

    #[test]
    fn from_conversions() {
        assert!(matches!(TwError::from("x"), TwError::Message(m) if m == "x"));
        assert!(matches!(TwError::from(String::from("y")), TwError::Message(m) if m == "y"));
        let io = std::io::Error::other("disk");
        let err = TwError::from(io);
        assert!(matches!(err, TwError::Io(_)));
        assert_eq!(err.to_string(), "disk");
        assert!(std::error::Error::source(&err).is_some());
    }
}
