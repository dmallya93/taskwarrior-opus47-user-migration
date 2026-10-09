//! Rust translation of `src/commands/CmdBurndown.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdBurndown.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownMonthly` (`burndown.monthly`).
pub struct CmdBurndownMonthly {
    meta: CommandMeta,
}

impl CmdBurndownMonthly {
    /// C++ `CmdBurndownMonthly::CmdBurndownMonthly` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownMonthly {
            meta: CommandMeta {
                keyword: "burndown.monthly".into(),
                usage: "task <filter> burndown.monthly".into(),
                description: "Shows a graphical burndown chart, by monthly".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdBurndownMonthly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownMonthly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownMonthly::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownWeekly` (`burndown.weekly`).
pub struct CmdBurndownWeekly {
    meta: CommandMeta,
}

impl CmdBurndownWeekly {
    /// C++ `CmdBurndownWeekly::CmdBurndownWeekly` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownWeekly {
            meta: CommandMeta {
                keyword: "burndown.weekly".into(),
                usage: "task <filter> burndown.weekly".into(),
                description: "Shows a graphical burndown chart, by weekly".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdBurndownWeekly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownWeekly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownWeekly::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownDaily` (`burndown.daily`).
pub struct CmdBurndownDaily {
    meta: CommandMeta,
}

impl CmdBurndownDaily {
    /// C++ `CmdBurndownDaily::CmdBurndownDaily` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownDaily {
            meta: CommandMeta {
                keyword: "burndown.daily".into(),
                usage: "task <filter> burndown.daily".into(),
                description: "Shows a graphical burndown chart, by daily".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdBurndownDaily {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownDaily {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownDaily::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownAnnual` (`burndown.annual`).
pub struct CmdBurndownAnnual {
    meta: CommandMeta,
}

impl CmdBurndownAnnual {
    /// C++ `CmdBurndownAnnual::CmdBurndownAnnual` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownAnnual {
            meta: CommandMeta {
                keyword: "burndown.annual".into(),
                usage: "task <filter> burndown.annual".into(),
                description: "Shows a graphical burndown chart, by annual".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdBurndownAnnual {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownAnnual {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownAnnual::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
