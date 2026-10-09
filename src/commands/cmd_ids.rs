//! Rust translation of `src/commands/CmdIDs.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 5 (CLI2/Filter/DOM/dispatch).
//!
//! Each struct below is one C++ class from `CmdIDs.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdIDs` (`ids`).
pub struct CmdIDs {
    meta: CommandMeta,
}

impl CmdIDs {
    /// C++ `CmdIDs::CmdIDs`.
    pub fn new() -> Self {
        CmdIDs {
            meta: CommandMeta {
                keyword: "ids".into(),
                usage: "task <filter> ids".into(),
                description: "Shows the IDs of matching tasks, as a range".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdIDs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdIDs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdIDs::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionIds` (`_ids`).
pub struct CmdCompletionIds {
    meta: CommandMeta,
}

impl CmdCompletionIds {
    /// C++ `CmdCompletionIds::CmdCompletionIds`.
    pub fn new() -> Self {
        CmdCompletionIds {
            meta: CommandMeta {
                keyword: "_ids".into(),
                usage: "task <filter> _ids".into(),
                description: "Shows the IDs of matching tasks, in the form of a list".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionIds {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionIds {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionIds::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshCompletionIds` (`_zshids`).
pub struct CmdZshCompletionIds {
    meta: CommandMeta,
}

impl CmdZshCompletionIds {
    /// C++ `CmdZshCompletionIds::CmdZshCompletionIds`.
    pub fn new() -> Self {
        CmdZshCompletionIds {
            meta: CommandMeta {
                keyword: "_zshids".into(),
                usage: "task <filter> _zshids".into(),
                description: "Shows the IDs and descriptions of matching tasks".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdZshCompletionIds {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshCompletionIds {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshCompletionIds::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUUIDs` (`uuids`).
pub struct CmdUUIDs {
    meta: CommandMeta,
}

impl CmdUUIDs {
    /// C++ `CmdUUIDs::CmdUUIDs`.
    pub fn new() -> Self {
        CmdUUIDs {
            meta: CommandMeta {
                keyword: "uuids".into(),
                usage: "task <filter> uuids".into(),
                description: "Shows the UUIDs of matching tasks, as a space-separated list".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUUIDs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUUIDs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUUIDs::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionUuids` (`_uuids`).
pub struct CmdCompletionUuids {
    meta: CommandMeta,
}

impl CmdCompletionUuids {
    /// C++ `CmdCompletionUuids::CmdCompletionUuids`.
    pub fn new() -> Self {
        CmdCompletionUuids {
            meta: CommandMeta {
                keyword: "_uuids".into(),
                usage: "task <filter> _uuids".into(),
                description: "Shows the UUIDs of matching tasks, as a list".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionUuids {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionUuids {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionUuids::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshCompletionUuids` (`_zshuuids`).
pub struct CmdZshCompletionUuids {
    meta: CommandMeta,
}

impl CmdZshCompletionUuids {
    /// C++ `CmdZshCompletionUuids::CmdZshCompletionUuids`.
    pub fn new() -> Self {
        CmdZshCompletionUuids {
            meta: CommandMeta {
                keyword: "_zshuuids".into(),
                usage: "task <filter> _zshuuids".into(),
                description: "Shows the UUIDs and descriptions of matching tasks".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdZshCompletionUuids {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshCompletionUuids {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshCompletionUuids::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
